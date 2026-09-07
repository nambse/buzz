//! Compiled Ortak desktop admission, independent of webview or runtime settings.

pub(crate) const PRIVATE: bool = cfg!(ortak_private_desktop);
pub(crate) const REFUSAL: &str =
    "This native capability is unavailable in the private Ortak desktop.";

pub(crate) const fn legacy_enabled() -> bool {
    !PRIVATE
}

pub(crate) fn require_legacy() -> Result<(), String> {
    if legacy_enabled() {
        Ok(())
    } else {
        Err(REFUSAL.into())
    }
}

/// Fixed company destination baked into the private artifact, never runtime env.
pub(crate) fn selected_company_relay() -> Option<&'static str> {
    option_env!("BUZZ_DESKTOP_BUILD_RELAY_URL").filter(|value| !value.is_empty())
}

/// Keep reconnect/bootstrap on the selected company and identity recovery on
/// `import_identity`, which owns persisted key changes and their recovery flags.
pub(crate) fn require_workspace_apply(relay_url: &str, nsec: Option<&str>) -> Result<(), String> {
    if !PRIVATE {
        return Ok(());
    }
    let selected = selected_company_relay()
        .ok_or_else(|| "The company connection is unavailable.".to_string())?;
    if relay_url.strip_suffix('/').unwrap_or(relay_url)
        != selected.strip_suffix('/').unwrap_or(selected)
        || nsec.is_some()
    {
        return Err(
            "This app is connected to one company. Use identity recovery to restore your key."
                .into(),
        );
    }
    Ok(())
}

// Deliberately enumerate the retained human Office surface. Unknown commands,
// including future additions to generate_handler!, receive no new authority.
// Agent listing/stopping is omitted: inherited implementations reconcile disk
// records and may stop remote or previously recorded processes. The private
// control plane owns Employee recovery; local media/terminal cancellation below
// only releases handles owned by this desktop instance.
const OFFICE_COMMANDS: &[&str] = &[
    "take_pending_community_deep_link",
    "acknowledge_pending_community_deep_link",
    "take_pending_navigation_deep_link",
    "acknowledge_pending_navigation_deep_link",
    "clear_pending_navigation_deep_links",
    "take_pending_entity_deep_link",
    "acknowledge_pending_entity_deep_link",
    "title_bar_double_click",
    "get_identity",
    "get_nsec",
    "generate_backup_passphrase",
    "create_ncryptsec_backup",
    "verify_ncryptsec_backup",
    "save_ncryptsec_copy",
    "import_identity",
    "persist_current_identity",
    "get_profile",
    "update_profile",
    "update_profile_at_relay",
    "get_user_profile",
    "get_users_batch",
    "get_user_notes",
    "search_users",
    "get_presence",
    "get_os_idle_seconds",
    "get_default_relay_url",
    "auto_connect_default_relay_enabled",
    "get_legacy_workspace_storage",
    "is_shared_identity",
    "get_relay_ws_url",
    "get_relay_http_url",
    "get_media_proxy_port",
    "fetch_link_preview_metadata",
    "sign_event",
    "sign_nostr_identity_binding",
    "sign_out",
    "decrypt_observer_event",
    "create_auth_event",
    "nip44_encrypt_to_self",
    "nip44_decrypt_from_self",
    "encrypted_dm_begin",
    "encrypted_dm_close",
    "encrypted_dm_authority",
    "encrypted_dm_open",
    "encrypted_dm_save_draft",
    "encrypted_dm_prepare",
    "encrypted_dm_publish",
    "encrypted_dm_retire",
    "get_channels",
    "get_open_channel_directory",
    "create_channel",
    "ensure_starter_channels",
    "open_dm",
    "hide_dm",
    "get_channel_details",
    "get_channel_members",
    "update_channel",
    "set_channel_topic",
    "set_channel_purpose",
    "archive_channel",
    "unarchive_channel",
    "delete_channel",
    "add_channel_members",
    "remove_channel_member",
    "change_channel_member_role",
    "join_channel",
    "leave_channel",
    "get_canvas",
    "set_canvas",
    "get_feed",
    "search_messages",
    "send_channel_message",
    "get_forum_posts",
    "get_forum_thread",
    "get_thread_replies",
    "get_channel_reconnect_repair",
    "get_channel_window",
    "get_channel_messages_before",
    "edit_message",
    "delete_message",
    "add_reaction",
    "remove_reaction",
    "get_event",
    "get_events",
    "show_native_notification",
    "take_pending_activations",
    "notification_permission_state",
    "request_notification_access",
    "upload_media",
    "pick_and_upload_media",
    "pick_and_upload_image",
    "upload_media_bytes",
    "upload_media_bytes_raw",
    "cancel_media_upload",
    "release_media_upload",
    "download_image",
    "save_png_data_url",
    "download_file",
    "fetch_media_bytes",
    "cancel_media_fetch",
    "release_media_fetch",
    "copy_image_to_clipboard",
    "copy_text_to_clipboard",
    "read_clipboard_text",
    "relay_requires_membership",
    "list_relay_members",
    "get_my_relay_membership",
    "add_relay_member",
    "remove_relay_member",
    "change_relay_member_role",
    "list_archived_identities",
    "get_relay_self",
    "resolve_oa_owner",
    "list_relay_agents",
    "unread_catch_up",
    "observed_unread_open_scope",
    "observed_unread_ingest",
    "channel_head_cache_load",
    "channel_head_cache_store",
    "channel_head_cache_clear",
    "get_contact_list",
    "get_huddle_state",
    "get_voice_input_mode",
    "get_tts_settings",
    "get_huddle_agent_pubkeys",
    "get_audio_output_device",
    "leave_huddle",
    "close_huddle_companion",
    "interrupt_huddle_speech",
    "terminal_detach",
    "terminal_close",
    "cancel_pairing",
    "apply_workspace",
    "get_active_workspace",
    "fetch_workspace_icon",
    "fetch_join_policy",
    "set_prevent_sleep_active",
    "observer_archive_default_enabled",
    "agent_metric_archive_default_enabled",
    "archive_events",
    "create_save_subscription",
    "merge_save_subscription_kinds",
    "remove_save_subscription_kind",
    "list_save_subscriptions",
    "delete_save_subscription",
    "read_archived_events",
    "read_archived_observer_events_for_channel",
    "index_observer_channel_id",
    "read_unindexed_observer_rows",
    "get_agent_usage_series",
    "get_observer_retention_days",
    "set_observer_retention_days",
    "archive_size_stats",
    "stop_archive_sync",
    "is_auto_update_supported",
    "set_window_vibrancy",
    "clear_tray_agent_activity",
    "requeue_tray_actions",
    "take_tray_actions",
    "update_tray_agent_activity",
];

pub(crate) fn command_allowed(command: &str) -> bool {
    legacy_enabled() || OFFICE_COMMANDS.contains(&command)
}

fn probe_requested(mut arguments: impl Iterator<Item = std::ffi::OsString>) -> bool {
    arguments.next().as_deref() == Some(std::ffi::OsStr::new("--ortak-private-policy-probe"))
        && arguments.next().is_none()
}

fn probe_json() -> String {
    format!(
        "{{\"probe\":\"ortak-private-native-policy-v1\",\"compiled_private\":{},\"legacy_startup_enabled\":{},\"sign_event_admitted\":{},\"legacy_start_admitted\":{},\"unknown_command_admitted\":{}}}",
        PRIVATE, legacy_enabled(), command_allowed("sign_event"),
        command_allowed("start_managed_agent"), command_allowed("future_gateway_command")
    )
}

/// Emit public compile-policy evidence only for the exact standalone probe CLI.
/// This runs before app, keyring, webview, runtime or network initialization.
#[doc(hidden)]
pub fn print_private_policy_probe_if_requested() -> bool {
    if !probe_requested(std::env::args_os().skip(1)) {
        return false;
    }
    println!("{}", probe_json());
    true
}

/// One admission seam used before Tauri's generated handler sees the request.
pub(crate) fn dispatch<T>(
    command: &str,
    request: T,
    handle: impl FnOnce(T) -> bool,
    reject: impl FnOnce(T, &'static str) -> bool,
) -> bool {
    if command_allowed(command) {
        handle(request)
    } else {
        reject(request, REFUSAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn only_exact_probe_arguments_skip_normal_startup() {
        for (args, expected) in [
            (vec!["--ortak-private-policy-probe"], true),
            (vec![], false),
            (vec!["--ortak-private-policy-probe=1"], false),
            (vec!["--ortak-private-policy-probe", "extra"], false),
            (vec!["--other", "--ortak-private-policy-probe"], false),
            (vec!["ORTAK_PRIVATE_DESKTOP=1"], false),
        ] {
            assert_eq!(probe_requested(args.into_iter().map(Into::into)), expected);
        }
        assert!(probe_json().contains(&format!("\"compiled_private\":{PRIVATE}")));
    }

    #[test]
    fn production_dispatch_refuses_before_any_legacy_handler_side_effect() {
        for command in [
            "start_managed_agent",
            "create_managed_agent",
            "list_managed_agents",
            "stop_managed_agent",
            "restart_managed_agent_runtime",
            "connect_acp_runtime",
            "mesh_start_node",
            "probe_backend_provider",
            "start_huddle",
            "join_huddle",
            "start_stt_pipeline",
            "download_voice_models",
            "preview_pocket_voice",
            "reconcile_inbound_persona_event",
            "mint_agent_card",
            "trigger_workflow",
            "relay_reconnect_hook",
            "terminal_attach",
            "open_project_terminal",
            "future_gateway_command",
        ] {
            let handled = Cell::new(false);
            let rejected = Cell::new(false);
            assert!(dispatch(
                command,
                (),
                |_| {
                    handled.set(true);
                    true
                },
                |_, reason| {
                    assert_eq!(reason, REFUSAL);
                    rejected.set(true);
                    true
                }
            ));
            assert_eq!(handled.get(), !PRIVATE, "{command}");
            assert_eq!(rejected.get(), PRIVATE, "{command}");
        }
    }

    #[test]
    fn office_signing_and_owned_cancellation_stay_available() {
        for command in [
            "get_identity",
            "sign_event",
            "create_auth_event",
            "apply_workspace",
            "send_channel_message",
            "get_channel_window",
            "cancel_media_fetch",
            "terminal_close",
            "cancel_pairing",
            "get_huddle_state",
            "leave_huddle",
        ] {
            assert!(dispatch(command, (), |_| true, |_, _| panic!("{command}")));
        }
        assert_eq!(require_legacy().is_err(), PRIVATE);
    }
}
