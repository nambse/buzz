//! Purpose-specific encrypted pair commands. No generic decrypt/key API, ordinary
//! event insertion, notification, search, draft-cache or optimistic publication.

mod authority;
mod codec;
mod store;
mod transport;

use crate::app_state::AppState;
use authority::{Pair, Session};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};
use tauri::{Manager, State};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, Error>;
/// Closed, content-free IPC refusal codes.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Unavailable,
    Revoked,
    Bounds,
    Encoding,
    Storage,
    Conflict,
    Busy,
}

static ACTIVE_VIEW: Mutex<Option<String>> = Mutex::new(None);
static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

/// The active local participant view; contains no URL or authority assertion.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    view_id: String,
    channel_id: String,
    expected_human: String,
    expected_relay: String,
}
impl Context {
    fn validate(&self) -> Result<()> {
        if !crate::private_native::PRIVATE {
            return Err(Error::Unavailable);
        }
        authority::uuid(&self.view_id)?;
        authority::uuid(&self.channel_id)?;
        authority::hex(&self.expected_human)?;
        if self.expected_relay.len() > 1024 {
            return Err(Error::Bounds);
        }
        Ok(())
    }
    fn live(&self) -> Result<()> {
        self.validate()?;
        if ACTIVE_VIEW
            .lock()
            .map_err(|_| Error::Unavailable)?
            .as_deref()
            != Some(self.view_id.as_str())
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }
    fn session(&self, state: &AppState) -> Result<Session> {
        self.live()?;
        Session::current(state, &self.expected_human, &self.expected_relay)
    }
    fn check(&self, session: &Session, state: &AppState) -> Result<()> {
        self.live()?;
        session.check(state)
    }
    fn base(&self, pair: &Pair) -> String {
        hex::encode(Sha256::digest(format!(
            "ortak-native-dm-store/1\n{}\n{}\n{}\n{}",
            self.expected_relay, pair.company_id, self.expected_human, self.channel_id
        )))
    }
}

/// Public metadata from a fresh native-authenticated central observation.
#[derive(Serialize)]
pub struct AuthorityView {
    pub pair: Pair,
    pub scope: String,
}
/// Volatile IPC display data, never inserted into ordinary persistent caches.
#[derive(Serialize)]
pub struct MessageView {
    pub rumor_id: String,
    pub sender: String,
    pub created_at: u64,
    pub reply_to: Option<String>,
    pub text: String,
}
/// One scoped plaintext display; callers must clear it when the view expires.
#[derive(Serialize)]
pub struct OpenView {
    pub pair: Pair,
    pub scope: String,
    pub draft: store::Draft,
    pub pending: Option<store::Pending>,
    pub retired: Vec<store::Pending>,
    pub messages: Vec<MessageView>,
    pub limited: bool,
    pub withheld_count: usize,
}

fn database(app: &tauri::AppHandle) -> Result<store::Store> {
    store::Store::at(&app.path().app_data_dir().map_err(|_| Error::Storage)?)
}

/// Begins one native participant view, retiring the previous volatile view.
#[tauri::command]
pub async fn encrypted_dm_begin(
    context: Context,
    state: State<'_, AppState>,
) -> Result<AuthorityView> {
    context.validate()?;
    *ACTIVE_VIEW.lock().map_err(|_| Error::Unavailable)? = Some(context.view_id.clone());
    encrypted_dm_authority(context, state).await
}

/// Invalidates a view before frontend buffers are released. No ciphertext deletion.
#[tauri::command]
pub fn encrypted_dm_close(view_id: String) -> Result<()> {
    authority::uuid(&view_id)?;
    let mut active = ACTIVE_VIEW.lock().map_err(|_| Error::Unavailable)?;
    if active.as_deref() == Some(view_id.as_str()) {
        *active = None;
    }
    Ok(())
}

/// Read-only heartbeat; returned data is current metadata, never a reusable grant.
#[tauri::command]
pub async fn encrypted_dm_authority(
    context: Context,
    state: State<'_, AppState>,
) -> Result<AuthorityView> {
    let _slot = READERS.try_acquire().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(6), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        context.check(&session, &state)?;
        Ok(AuthorityView {
            scope: pair.scope()?,
            pair,
        })
    })
    .await
    .map_err(|_| Error::Unavailable)?
}

/// Opens only the current pair's protected draft and a bounded recipient snapshot.
#[tauri::command]
pub async fn encrypted_dm_open(
    context: Context,
    expected_scope: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<OpenView> {
    let _slot = READERS.try_acquire().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(25), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        let scope = pair.scope()?;
        if scope != expected_scope {
            return Err(Error::Revoked);
        }
        context.check(&session, &state)?;
        let store = database(&app)?;
        let base = context.base(&pair);
        let mut socket = transport::Socket::authenticated(&session).await?;
        context.check(&session, &state)?;
        let events = socket.read(&context.expected_human).await?;
        drop(socket);
        session.unchanged(&state, &pair).await?;
        context.check(&session, &state)?;
        let mut selected = BTreeMap::new();
        let mut withheld_count = 0;
        for event in &events {
            // Other same-recipient pairs and invalid ciphertext are withheld;
            // they are never normalized to empty successful plaintext messages.
            if let Ok(opened) =
                codec::open(&session.keys, &pair.employee_public_key, event.as_bytes())
            {
                if let Some(old) = selected.get(&opened.rumor_id) {
                    let old: &codec::Opened = old;
                    if old.sender != opened.sender
                        || old.created_at != opened.created_at
                        || old.text.as_str() != opened.text.as_str()
                        || old.reply_to != opened.reply_to
                    {
                        return Err(Error::Encoding);
                    }
                } else {
                    selected.insert(opened.rumor_id.clone(), opened);
                }
            } else {
                withheld_count += 1;
            }
        }
        let known: std::collections::BTreeSet<String> = selected.keys().cloned().collect();
        let mut messages: Vec<_> = selected
            .into_values()
            .map(|m| MessageView {
                rumor_id: m.rumor_id,
                sender: m.sender,
                created_at: m.created_at,
                reply_to: m.reply_to.filter(|id| known.contains(id)),
                text: m.text.to_string(),
            })
            .collect();
        messages.sort_by(|a, b| (a.created_at, &a.rumor_id).cmp(&(b.created_at, &b.rumor_id)));
        context.check(&session, &state)?;
        let draft = store.draft(&base, &scope, &session.keys)?;
        let pending = store.pending(&base)?.map(|e| e.pending);
        let retired = store.retired(&base)?;
        session.unchanged(&state, &pair).await?;
        context.check(&session, &state)?;
        Ok(OpenView {
            pair,
            scope,
            draft,
            pending,
            retired,
            messages,
            limited: events.len() == 32,
            withheld_count,
        })
    })
    .await
    .map_err(|_| Error::Unavailable)?
}

/// Saves a native self-NIP44 draft with exact scope and monotonic CAS version.
#[tauri::command]
pub async fn encrypted_dm_save_draft(
    context: Context,
    expected_scope: String,
    version: u64,
    text: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<u64> {
    let text = Zeroizing::new(text);
    codec::text(&text, true)?;
    let _lock = MUTATION.try_lock().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(6), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        if pair.scope()? != expected_scope {
            return Err(Error::Revoked);
        }
        context.check(&session, &state)?;
        database(&app)?.save_draft(
            &context.base(&pair),
            &expected_scope,
            &session.keys,
            version,
            &text,
        )
    })
    .await
    .map_err(|_| Error::Unavailable)?
}

/// Freezes both exact outer bytes atomically, before opening a publish socket.
#[tauri::command]
pub async fn encrypted_dm_prepare(
    context: Context,
    expected_scope: String,
    operation_id: String,
    draft_version: u64,
    text: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<store::Pending> {
    let text = Zeroizing::new(text);
    codec::text(&text, false)?;
    authority::uuid(&operation_id)?;
    let _lock = MUTATION.try_lock().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(15), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        if pair.scope()? != expected_scope {
            return Err(Error::Revoked);
        }
        let store = database(&app)?;
        let base = context.base(&pair);
        if let Some(entry) = store.entry(&base, &operation_id)? {
            entry.require_sendable(&expected_scope)?;
            context.check(&session, &state)?;
            let old = codec::open(
                &session.keys,
                &pair.employee_public_key,
                entry.frozen.outer_json[1].as_bytes(),
            )?;
            if old.text.as_str() != text.as_str() || old.reply_to.is_some() {
                return Err(Error::Conflict);
            }
            context.check(&session, &state)?;
            return Ok(entry.pending);
        }
        if store.pending(&base)?.is_some() {
            return Err(Error::Conflict);
        }
        context.check(&session, &state)?;
        let draft = store.draft(&base, &expected_scope, &session.keys)?;
        let protected_text = Zeroizing::new(draft.text);
        if draft.version != draft_version || protected_text.as_str() != text.as_str() {
            return Err(Error::Conflict);
        }
        let frozen = codec::freeze(&session.keys, &pair.employee_public_key, &text).await?;
        session.unchanged(&state, &pair).await?;
        context.check(&session, &state)?;
        Ok(store
            .freeze(
                &base,
                &expected_scope,
                &operation_id,
                draft_version,
                &frozen,
            )?
            .pending)
    })
    .await
    .map_err(|_| Error::Unavailable)?
}

/// Publishes only retained copies. Unknown/rejected ACK leaves the same copy
/// pending. Each actual send rechecks pair generation and the current native view.
#[tauri::command]
pub async fn encrypted_dm_publish(
    context: Context,
    expected_scope: String,
    operation_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<store::Pending> {
    authority::uuid(&operation_id)?;
    let _lock = MUTATION.try_lock().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(25), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        if pair.scope()? != expected_scope {
            return Err(Error::Revoked);
        }
        let store = database(&app)?;
        let base = context.base(&pair);
        let entry = store.entry(&base, &operation_id)?.ok_or(Error::Conflict)?;
        entry.require_sendable(&expected_scope)?;
        for ordinal in 0..2 {
            if entry.pending.acknowledged[ordinal] {
                continue;
            }
            let mut socket = transport::Socket::authenticated(&session).await?;
            session.unchanged(&state, &pair).await?;
            context.check(&session, &state)?;
            socket
                .publish(
                    &entry.frozen.outer_json[ordinal],
                    &entry.frozen.outer_ids[ordinal],
                )
                .await?;
            drop(socket);
            // Even when authority was lost during the flight, preserve the actual
            // exact ACK. It does not authorize another copy or a plaintext read.
            store.ack(&base, &operation_id, ordinal)?;
        }
        context.check(&session, &state)?;
        Ok(store
            .entry(&base, &operation_id)?
            .ok_or(Error::Storage)?
            .pending)
    })
    .await
    .map_err(|_| Error::Unavailable)?
}

/// Explicitly stop retries of an owned send while retaining its frozen copies
/// and ACKs. This does not undo delivery or create/re-sign a replacement message.
#[tauri::command]
pub async fn encrypted_dm_retire(
    context: Context,
    expected_scope: String,
    original_scope: String,
    operation_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<store::Pending> {
    authority::uuid(&operation_id)?;
    authority::hex(&original_scope)?;
    let _lock = MUTATION.try_lock().map_err(|_| Error::Busy)?;
    tokio::time::timeout(Duration::from_secs(6), async {
        let session = context.session(&state)?;
        let pair = session.pair(&state, &context.channel_id).await?;
        if pair.scope()? != expected_scope {
            return Err(Error::Revoked);
        }
        let store = database(&app)?;
        session.unchanged(&state, &pair).await?;
        context.check(&session, &state)?;
        let receipt = store.retire(&context.base(&pair), &original_scope, &operation_id)?;
        context.check(&session, &state)?;
        Ok(receipt)
    })
    .await
    .map_err(|_| Error::Unavailable)?
}
