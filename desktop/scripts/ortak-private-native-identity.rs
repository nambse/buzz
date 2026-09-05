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
