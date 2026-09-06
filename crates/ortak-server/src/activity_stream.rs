//! Signed, short-lived Activity SSE. PostgreSQL NOTIFY is only a wake hint;
//! every payload comes from dense persisted events under fresh Office authority.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event, Sse},
    Extension,
};
use futures_util::{stream, Stream};
use ortak_observability::{ActivityQueries, RunEventsQuery};
use serde::Deserialize;
use sqlx::postgres::PgListener;
use tokio::{sync::OwnedSemaphorePermit, time::Instant};
use uuid::Uuid;

use super::{detail_response, ApiState};
use crate::{
    auth::{human_allowed_on, lock_authority, Principal, RequestAuthority},
    error::{ApiError, Result},
};

const LIFETIME: Duration = Duration::from_secs(45);
const HEARTBEAT: Duration = Duration::from_secs(5);
const READ_DEADLINE: Duration = Duration::from_secs(8);

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubscriptionQuery {
    after_sequence: Option<i64>,
}

pub(super) async fn subscribe(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Query(query): Query<SubscriptionQuery>,
    Extension(authority): Extension<RequestAuthority>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    if query
        .after_sequence
        .is_some_and(|cursor| cursor < 0 || cursor > 9_007_199_254_740_991)
    {
        return Err(ApiError::invalid());
    }
    state
        .require_run(&principal, run_id, "read_events", &authority)
        .await?;
    let permit = Arc::clone(&state.stream_slots)
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "activity_stream_limit"))?;
    // LISTEN completes before the first durable read. The middleware's initial
    // authority transaction commits before this response body is polled.
    let mut listener = tokio::time::timeout(Duration::from_secs(3), async {
        let mut listener = PgListener::connect_with(&state.listener_pool).await?;
        listener.listen("ortak_activity_v1").await?;
        Ok::<_, sqlx::Error>(listener)
    })
    .await
    .map_err(|_| ApiError::unavailable())?
    .map_err(|_| ApiError::unavailable())?;
    // Do not let the driver silently recover and hide a notification gap.
    listener.eager_reconnect(false);
    let session = Session {
        state,
        principal,
        run_id,
        listener,
        cursor: query.after_sequence,
        deadline: Instant::now() + LIFETIME,
        heartbeat: Instant::now() + HEARTBEAT,
        needs_batch: true,
        closed: false,
        _permit: permit,
    };
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let (start, started) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut session = session;
        // Release resources after the absolute deadline even if the HTTP peer
        // never polls its body. The first snapshot still follows the first poll.
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
    run_id: Uuid,
    listener: PgListener,
    cursor: Option<i64>,
    deadline: Instant,
    heartbeat: Instant,
    needs_batch: bool,
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

    async fn next(&mut self) -> Event {
        loop {
            if Instant::now() >= self.deadline {
                return self.close("renew");
            }
            if self.needs_batch {
                let result = tokio::time::timeout_at(
                    self.deadline.min(Instant::now() + READ_DEADLINE),
                    self.batch(),
                )
                .await;
                return match result {
                    Ok(Ok(event)) => event,
                    Ok(Err(error)) => self.close(if matches!(error.0.as_u16(), 401 | 403 | 404) {
                        "revoked"
                    } else if error.0 == StatusCode::CONFLICT {
                        "resync"
                    } else {
                        "retry"
                    }),
                    Err(_) => self.close("retry"),
                };
            }
            tokio::select! {
                biased;
                _=tokio::time::sleep_until(self.deadline)=>return self.close("renew"),
                _=tokio::time::sleep_until(self.heartbeat)=>{
                    self.heartbeat=Instant::now()+HEARTBEAT;
                    let result=tokio::time::timeout_at(self.deadline.min(Instant::now()+READ_DEADLINE),self.authorize()).await;
                    return match result {
                        Ok(Ok(()))=>Event::default().event("heartbeat").data("{}"),
                        Ok(Err(error))=>self.close(if matches!(error.0.as_u16(),401|403|404) {"revoked"} else if error.0==StatusCode::CONFLICT {"resync"} else {"retry"}),
                        Err(_)=>self.close("retry"),
                    };
                }
                notification=self.listener.try_recv()=>match notification {
                    Ok(Some(notification))=>{
                        // A public UUID hint can only cause a fresh authorized
                        // read; it cannot inject content or advance the cursor.
                        if notification.payload().len()>256 { continue; }
                        if let Ok(hint)=serde_json::from_str::<Hint>(notification.payload()) {
                            if hint.company_id==self.principal.scope.company_id()
                                && hint.run_id.is_none_or(|id|id==self.run_id) {
                                self.needs_batch=true;
                            }
                        }
                    }
                    _=>return self.close("retry"),
                }
            }
        }
    }

    fn current_principal(&self) -> Result<Principal> {
        // Re-select the server-owned role/audience for every payload. API
        // configuration is immutable for a process; changes require restart.
        let grant = self
            .state
            .config
            .humans
            .iter()
            .find(|grant| grant.public_key == self.principal.public_key.to_hex())
            .cloned()
            .ok_or(ApiError(StatusCode::FORBIDDEN, "forbidden"))?;
        let mut principal = self.principal.clone();
        principal.grant = grant;
        Ok(principal)
    }

    async fn authority(
        &self,
        principal: &Principal,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        let mut tx = self.state.control.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','8s',true)")
            .execute(&mut *tx).await?;
        // Classify an already revoked company/community as authorization loss,
        // even when its lifecycle intentionally makes the authority lock fail.
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
            || !self
                .state
                .visible_run_on(&mut tx, principal, self.run_id)
                .await?
        {
            return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
        }
        Ok(tx)
    }

    async fn authorize(&mut self) -> Result<()> {
        let _slot = self
            .state
            .query_slots
            .acquire()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let principal = self.current_principal()?;
        self.authority(&principal).await?.commit().await?;
        Ok(())
    }

    async fn batch(&mut self) -> Result<Event> {
        // A cloned state/principal keeps the short authority fence alive across
        // projection reads on the existing read API's second pool connection.
        let state = self.state.clone();
        let principal = self.current_principal()?;
        let _slot = state
            .query_slots
            .acquire()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let run_id = self.run_id;
        let cursor = self.cursor;
        let authority = self.authority(&principal).await?;
        let detail = detail_response(&state, &principal, run_id).await?;
        let page = state
            .control
            .run_events(
                &principal.scope,
                run_id,
                &RunEventsQuery {
                    after_sequence: cursor,
                    limit: Some(25),
                    include_raw: false,
                },
            )
            .await?;
        let highwater = detail
            .pointer("/detail/run/last_event/sequence")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);
        if cursor.is_some_and(|cursor| cursor > highwater) || page.gap.is_some() {
            return Err(ApiError(StatusCode::CONFLICT, "activity_cursor_gap"));
        }
        let next = page.next_after_sequence;
        let more = page.has_more || highwater > next.unwrap_or(-1);
        let json = serde_json::to_string(&serde_json::json!({"detail":detail,"page":page}))
            .map_err(|_| ApiError::unavailable())?;
        if json.len() > 4 * 1024 * 1024 {
            return Err(ApiError::unavailable());
        }
        authority.commit().await?;
        self.cursor = next;
        self.needs_batch = more;
        self.heartbeat = Instant::now() + HEARTBEAT;
        let mut event = Event::default().event("activity").data(json);
        if let Some(sequence) = next {
            event = event.id(sequence.to_string());
        }
        Ok(event)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hint {
    company_id: Uuid,
    run_id: Option<Uuid>,
}
