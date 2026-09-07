use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Spy {
    calls: Mutex<Vec<String>>,
    value: Mutex<Option<OsString>>,
}
impl EnvironmentLookup for Spy {
    fn read(&self, name: &str) -> Option<OsString> {
        self.calls.lock().unwrap().push(name.to_owned());
        self.value.lock().unwrap().clone()
    }
}
fn binding(reference: &str, name: &str) -> EnvCredentialBinding {
    EnvCredentialBinding {
        credential_ref: CredentialRef::parse(reference).unwrap(),
        environment_variable: name.to_owned(),
    }
}
fn selected() -> EnvCredentialBinding {
    binding("credential://fresh/employee/runtime", "ORTAK_TEST_SELECTED")
}

#[test]
fn validates_entire_allowlist_without_looking_up_any_value() {
    let duplicate_reference = vec![
        selected(),
        binding("credential://fresh/employee/runtime", "OTHER"),
    ];
    let duplicate_environment = vec![
        selected(),
        binding("secret://fresh/employee/memory", "ORTAK_TEST_SELECTED"),
    ];
    let oversized = (0..=MAX_BINDINGS)
        .map(|i| binding(&format!("credential://fresh/{i}"), &format!("VAR_{i}")))
        .collect();
    let mut invalid = vec![duplicate_reference, duplicate_environment, oversized];
    for name in [
        "",
        "9NAME",
        "A=B",
        "A B",
        "A\nB",
        "A\0B",
        "ÄBC",
        "A-B",
        &"A".repeat(129),
    ] {
        invalid.push(vec![selected(), binding("secret://fresh/other", name)]);
    }
    for mappings in invalid {
        let spy = Arc::new(Spy::default());
        let result = EnvCredentialResolver::with_lookup(mappings, spy.clone());
        assert!(matches!(result, Err(CredentialError::Unavailable { .. })));
        assert!(
            spy.calls.lock().unwrap().is_empty(),
            "not even an earlier valid mapping may be probed"
        );
    }
    let spy = Arc::new(Spy::default());
    let maximum = (0..MAX_BINDINGS)
        .map(|i| binding(&format!("secret://fresh/{i}"), &format!("_var_{i}")))
        .collect();
    assert!(EnvCredentialResolver::with_lookup(maximum, spy.clone()).is_ok());
    assert!(spy.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unauthorized_and_empty_allowlists_refuse_before_lookup() {
    for mappings in [vec![], vec![selected()]] {
        let spy = Arc::new(Spy::default());
        *spy.value.lock().unwrap() = Some("synthetic-present-value".into());
        let resolver = EnvCredentialResolver::with_lookup(mappings, spy.clone()).unwrap();
        let foreign = CredentialRef::parse("credential://different-caller/runtime").unwrap();
        assert!(matches!(
            resolver.verify_reference(&foreign).await,
            Err(CredentialError::Unauthorized { .. })
        ));
        assert!(spy.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn selected_reference_checks_fresh_presence_each_time_without_exposing_values() {
    let spy = Arc::new(Spy::default());
    let mapping = selected();
    let reference = mapping.credential_ref.clone();
    let resolver = EnvCredentialResolver::with_lookup(vec![mapping], spy.clone()).unwrap();
    assert!(spy.calls.lock().unwrap().is_empty());
    for (value, expected) in [
        (None, CredentialReferenceStatus::Missing),
        (Some(""), CredentialReferenceStatus::Missing),
        (
            Some("synthetic-value-never-returned"),
            CredentialReferenceStatus::Resolvable,
        ),
        (Some("  "), CredentialReferenceStatus::Resolvable),
        (None, CredentialReferenceStatus::Missing),
    ] {
        *spy.value.lock().unwrap() = value.map(OsString::from);
        let result = resolver.verify_reference(&reference).await;
        assert_eq!(result, Ok(expected));
        assert!(!format!("{result:?}").contains("synthetic-value-never-returned"));
    }
    assert_eq!(*spy.calls.lock().unwrap(), vec!["ORTAK_TEST_SELECTED"; 5]);
}

#[cfg(unix)]
#[tokio::test]
async fn non_unicode_selected_value_fails_with_sanitized_error() {
    use std::os::unix::ffi::OsStringExt;
    let spy = Arc::new(Spy::default());
    let mut bytes = b"synthetic-secret-prefix-".to_vec();
    bytes.push(0xff);
    *spy.value.lock().unwrap() = Some(OsString::from_vec(bytes));
    let mapping = selected();
    let reference = mapping.credential_ref.clone();
    let resolver = EnvCredentialResolver::with_lookup(vec![mapping], spy).unwrap();
    let error = resolver.verify_reference(&reference).await.unwrap_err();
    assert!(matches!(error, CredentialError::Unavailable { .. }));
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains("synthetic-secret-prefix"));
        assert!(!text.contains("ORTAK_TEST_SELECTED"));
    }
}

#[test]
fn malformed_reference_is_rejected_by_the_public_configuration_type() {
    for value in [
        "plain-secret-value",
        "credential://../other",
        "credential://",
    ] {
        let result = serde_json::from_value::<EnvCredentialBinding>(serde_json::json!({
            "credential_ref": value, "environment_variable": "ORTAK_TEST_SELECTED"
        }));
        assert!(result.is_err());
    }
    assert!(EnvCredentialResolver::new(vec![]).is_ok());
}

#[tokio::test]
async fn public_resolver_reads_only_explicit_subprocess_environment() {
    const MODE: &str = "ORTAK_CREDENTIAL_RESOLVER_TEST_CHILD";
    if let Ok(mode) = std::env::var(MODE) {
        let mapping = selected();
        let reference = mapping.credential_ref.clone();
        let resolver = EnvCredentialResolver::new(vec![mapping]).unwrap();
        let result = resolver.verify_reference(&reference).await;
        match mode.as_str() {
            "present" => assert_eq!(result, Ok(CredentialReferenceStatus::Resolvable)),
            "absent" | "empty" => assert_eq!(result, Ok(CredentialReferenceStatus::Missing)),
            "nonunicode" => assert!(matches!(result, Err(CredentialError::Unavailable { .. }))),
            _ => panic!("unknown private child test mode"),
        }
        let foreign = CredentialRef::parse("secret://not-selected").unwrap();
        assert!(matches!(
            resolver.verify_reference(&foreign).await,
            Err(CredentialError::Unauthorized { .. })
        ));
        return;
    }
    // Each child gets a new environment rather than mutating this multithreaded
    // process. Output is discarded; test values are synthetic and never logged.
    for mode in ["present", "absent", "empty", "nonunicode"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg("credentials::environment::tests::public_resolver_reads_only_explicit_subprocess_environment")
            .env_clear()
            .env(MODE, mode)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        match mode {
            "present" => {
                command.env("ORTAK_TEST_SELECTED", "synthetic-child-presence");
            }
            "empty" => {
                command.env("ORTAK_TEST_SELECTED", "");
            }
            "nonunicode" => {
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStringExt;
                    command.env("ORTAK_TEST_SELECTED", OsString::from_vec(vec![0xff]));
                }
                #[cfg(not(unix))]
                continue;
            }
            _ => {}
        }
        let mut child = command.spawn().expect("spawn isolated resolver test");
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().expect("check child") {
                assert!(
                    status.success(),
                    "public environment resolver child failed for mode {mode}"
                );
                break;
            }
            if started.elapsed() >= std::time::Duration::from_secs(5) {
                child.kill().expect("terminate bounded child");
                child.wait().expect("reap bounded child");
                panic!("public environment resolver child exceeded deadline");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
