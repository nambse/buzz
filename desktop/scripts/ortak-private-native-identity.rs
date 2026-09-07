//! Small harness around the real native identity modules; no Tauri application,
//! filesystem migration, OS keyring access, network or Cargo build is involved.
extern crate self as dirs;

pub fn config_dir() -> Option<std::path::PathBuf> {
    Some(std::env::temp_dir().join("ortak-native-identity-config-resolver"))
}

#[path = "../src-tauri/src/app_state_keyring.rs"]
mod app_state_keyring;
#[path = "../src-tauri/src/build_identity.rs"]
mod build_identity;
#[path = "../src-tauri/src/private_build_flags.rs"]
mod private_build_flags;
#[path = "../src-tauri/src/private_native.rs"]
mod private_native;

#[test]
fn actual_private_build_disables_legacy_even_without_runtime_configuration() {
    assert!(private_native::PRIVATE);
    assert!(!private_native::legacy_enabled());
    assert!(private_native::require_legacy().is_err());
}

#[test]
fn private_workspace_reconnect_preserves_fixed_company_and_identity_recovery() {
    let selected = private_native::selected_company_relay().expect("compiled company relay");
    assert_eq!(selected, "ws://localhost:3038");
    assert!(private_native::require_workspace_apply(selected, None).is_ok());
    assert!(private_native::require_workspace_apply(&format!("{selected}/"), None).is_ok());
    for candidate in ["ws://localhost:3000", "ws://127.0.0.1:3038", "wss://other.example", "ws://localhost:3038/other", "ws://localhost:3038?next=other"] {
        assert!(private_native::require_workspace_apply(candidate, None).is_err());
    }
    assert!(private_native::require_workspace_apply(selected, Some("replacement-key")).is_err());
    assert!(private_native::require_workspace_apply(selected, Some("")).is_err());
    assert!(private_native::command_allowed("import_identity"));
    assert!(private_native::command_allowed("get_identity"));
    assert!(private_native::command_allowed("create_ncryptsec_backup"));
}

#[test]
fn actual_native_identity_is_private_in_debug_and_release_paths() {
    assert!(build_identity::is_demo_build());
    assert_eq!(build_identity::demo_slug(), Some("ortak-private-20260905"));
    assert_eq!(
        app_state_keyring::keyring_service(),
        "buzz-desktop-demo.ortak-private-20260905"
    );
    assert_eq!(
        build_identity::nest_name(true),
        ".buzz-demo-ortak-private-20260905"
    );
    assert_eq!(
        build_identity::nest_name(false),
        ".buzz-demo-ortak-private-20260905"
    );
    assert_eq!(
        build_identity::cli_name(true),
        "buzz-demo-ortak-private-20260905"
    );
    assert!(build_identity::is_deep_link_for_build(
        "buzz-demo-ortak-private-20260905://message?id=1"
    ));
    assert!(!build_identity::is_deep_link_for_build(
        "buzz://message?id=1"
    ));
    let expected = config_dir()
        .unwrap()
        .join("buzz-demo-ortak-private-20260905");
    assert_eq!(
        build_identity::demo_config_home().unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        build_identity::demo_agent_oauth_cache_dir().unwrap(),
        Some(expected.join("buzz-agent/oauth"))
    );
}
