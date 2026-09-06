//! Message-scoped routing snapshots. Notifications carry hints, never authority.
use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::Event, Sse},
    Extension,
};
use futures_util::{stream, Stream};
use nostr::EventId;
use serde::Deserialize;
use sqlx::postgres::PgListener;
use tokio::{sync::OwnedSemaphorePermit, time::Instant};
use uuid::Uuid;

use super::{routing_read, ApiState};
use crate::{
    auth::{human_allowed_on, lock_authority, Principal, RequestAuthority},
    error::{ApiError, Result},
};

const LIFETIME: Duration = Duration::from_secs(45);
const HEARTBEAT: Duration = Duration::from_secs(5);
const READ_DEADLINE: Duration = Duration::from_secs(8);
const FRAME_BYTES: usize = 65_536;

pub(super) async fn subscribe(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Extension(authority): Extension<RequestAuthority>,
    Path((channel, message)): Path<(Uuid, String)>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let message = EventId::from_hex(&message).map_err(|_| ApiError::invalid())?;
    let deadline = Instant::now() + LIFETIME;
    let permit = Arc::clone(&state.stream_slots)
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "routing_stream_limit"))?;
    let mut listener = tokio::time::timeout(Duration::from_secs(3), async {
        let mut listener = PgListener::connect_with(&state.listener_pool).await?;
        listener.listen("ortak_routing_v1").await?;
        Ok::<_, sqlx::Error>(listener)
    })
    .await
    .map_err(|_| ApiError::unavailable())?
    .map_err(|_| ApiError::unavailable())?;
    listener.eager_reconnect(false);
    // LISTEN is already active. Reject inaccessible/broken sources before SSE
    // admission; do not cache this projection for a possibly delayed first poll.
    tokio::time::timeout(READ_DEADLINE, async {
        let mut held = authority.0.lock().await;
        let connection = held.as_mut().ok_or_else(ApiError::unavailable)?;
        routing_read::current_on(&state, &principal, channel, message, connection).await?;
        Ok::<_, ApiError>(())
    })
    .await
    .map_err(|_| ApiError::unavailable())??;
    let session = Session {
        state,
        principal,
        channel,
        message,
        listener,
        deadline,
        heartbeat: Instant::now() + HEARTBEAT,
        needs_snapshot: true,
        closed: false,
        _permit: permit,
    };
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let (start, started) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut session = session;
        // This owner retires at the absolute deadline even for an unpolled
        // body; receiver drop also cancels any in-flight read/listener wait.
        if !matches!(
            tokio::time::timeout_at(session.deadline, started).await,
            Ok(Ok(()))
        ) {
            return;
        }
        loop {
            let permit = tokio::select! {
                _=tokio::time::sleep_until(session.deadline)=>break,
                permit=sender.reserve()=>match permit { Ok(permit)=>permit, Err(_)=>break },
            };
            let event = tokio::select! {
                _=sender.closed()=>break,
                event=session.next()=>event,
            };
            permit.send(event);
            if session.closed {
                break;
            }
        }
    });
    Ok(Sse::new(stream::unfold(
        (receiver, Some(start)),
        |(mut receiver, start)| async move {
            if let Some(start) = start {
                let _ = start.send(());
            }
            receiver
                .recv()
                .await
                .map(|event| (Ok(event), (receiver, None)))
        },
    )))
}

struct Session {
    state: ApiState,
    principal: Principal,
    channel: Uuid,
    message: EventId,
    listener: PgListener,
    deadline: Instant,
    heartbeat: Instant,
    needs_snapshot: bool,
    closed: bool,
    _permit: OwnedSemaphorePermit,
}

impl Session {
    fn close(&mut self, code: &'static str) -> Event {
        self.closed = true;
        Event::default()
            .event("control")
            .data(format!("{{\"code\":\"{code}\"}}"))
    }

    fn failed(&mut self, error: Option<ApiError>) -> Event {
        self.close(
            if error.is_some_and(|e| matches!(e.0.as_u16(), 401 | 403 | 404)) {
                "revoked"
            } else {
                "retry"
            },
        )
    }

    async fn next(&mut self) -> Event {
        loop {
            if Instant::now() >= self.deadline {
                return self.close("renew");
            }
            if self.needs_snapshot {
                let result = tokio::time::timeout_at(
                    self.deadline.min(Instant::now() + READ_DEADLINE),
                    self.read(true),
                )
                .await;
                return match result {
                    Ok(Ok(event)) => {
                        self.needs_snapshot = false;
                        self.heartbeat = Instant::now() + HEARTBEAT;
                        event
                    }
                    Ok(Err(error)) => self.failed(Some(error)),
                    Err(_) => self.failed(None),
                };
            }
            tokio::select! {
                biased;
                _=tokio::time::sleep_until(self.deadline)=>return self.close("renew"),
                _=tokio::time::sleep_until(self.heartbeat)=>{
                    self.heartbeat=Instant::now()+HEARTBEAT;
                    let result=tokio::time::timeout_at(self.deadline.min(Instant::now()+READ_DEADLINE),self.read(false)).await;
                    return match result {
                        Ok(Ok(event))=>event,
                        Ok(Err(error))=>self.failed(Some(error)),
                        Err(_)=>self.failed(None),
                    };
                },
                notification=self.listener.try_recv()=>match notification {
                    Ok(Some(notification))=>{
                        if notification.payload().len()<=256 {
                            if let Ok(hint)=serde_json::from_str::<Hint>(notification.payload()) {
                                if hint.company_id==self.principal.scope.company_id()
                                    && hint.message_id.as_deref().is_none_or(|id|id==self.message.to_hex()) {
                                    self.needs_snapshot=true;
                                }
                            }
                        }
                    },
                    _=>return self.close("retry"),
                },
            }
        }
    }

    async fn read(&self, snapshot: bool) -> Result<Event> {
        let _slot = self
            .state
            .query_slots
            .acquire()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let mut principal = self.principal.clone();
        principal.grant = self
            .state
            .config
            .humans
            .iter()
            .find(|grant| grant.public_key == principal.public_key.to_hex())
            .cloned()
            .ok_or(ApiError(StatusCode::FORBIDDEN, "forbidden"))?;
        let mut tx = self.state.control.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','8s',true)")
            .execute(&mut *tx).await?;
        if !human_allowed_on(
            &mut tx,
            &principal.scope,
            self.state.config.community_id,
            &principal.public_key,
        )
        .await?
        {
            return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
        }
        lock_authority(&mut tx, &principal.scope).await?;
        if !human_allowed_on(
            &mut tx,
            &principal.scope,
            self.state.config.community_id,
            &principal.public_key,
        )
        .await?
        {
            return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
        }
        let event = if snapshot {
            let page = routing_read::current_on(
                &self.state,
                &principal,
                self.channel,
                self.message,
                &mut tx,
            )
            .await?;
            let json = serde_json::to_string(&page).map_err(|_| ApiError::unavailable())?;
            if json.len() > FRAME_BYTES {
                return Err(ApiError::unavailable());
            }
            Event::default().event("routing").data(json)
        } else {
            // Heartbeats revalidate current canonical audience without polling
            // decision metadata. A missed hint is repaired on signed reconnect.
            routing_read::visible_on(&self.state, &principal, self.channel, self.message, &mut tx)
                .await?;
            Event::default().event("heartbeat").data("{}")
        };
        tx.commit().await?;
        Ok(event)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hint {
    company_id: Uuid,
    message_id: Option<String>,
}
