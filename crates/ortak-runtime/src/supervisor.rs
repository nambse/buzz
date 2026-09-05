//! Run supervisor: dispatch one leased `run_dispatch` row, pump runtime
//! events from the durable cursor, and cancel a correlated run.
//!
//! Order of operations for one leased row ([`RunSupervisor::dispatch`]):
//!
//! 1. In-memory preconditions (kind) fail closed and touch nothing.
//! 2. The sealed [`DispatchAuthority`](crate::DispatchAuthority) is derived from durable rows; a stale
//!    lease writes nothing, a refusal records a bounded outbox failure.
//! 3. The durable run row is created (or read back) under the lease fence.
//!    A run that is already correlated or terminal completes the lease
//!    without a runtime call.
//! 4. Required memory health and bounded RunScratch recall happen outside
//!    transactions. The full RunSpec is frozen once under fresh canonical
//!    authority. Retries reuse its exact stored context without recalling again.
//! 5. [`RuntimeAdapter::start_run`] receives the frozen specification outside
//!    any transaction with the stable [`run_idempotency_key`](crate::run_idempotency_key).
//! 6. Correlation is compare-and-set on the run row and completes the lease
//!    in the same commit; a different runtime reference is refused and the
//!    orphaned runtime run must be cancelled; a failed cleanup propagates.
//!
//! Event ingestion ([`RunSupervisor::pump`]) reads the last durable cursor,
//! calls [`RuntimeAdapter::next_events`] outside any transaction, normalizes
//! and redacts through the [`RunEvent`] contract, and appends under the run
//! row lock so the terminal event and the terminal status commit together.
//! Only typed events move the status; a malformed or out-of-order event is
//! recorded as a non-terminal `error.raised` under its cursor so the stream
//! still advances.
//!
//! Completed Office output is scheduled separately by [`crate::office_output`].

use std::time::Duration;

use chrono::Utc;
use ortak_control::adapter::Detail;
use ortak_control::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use ortak_control::ports::OutboxRepository;
use ortak_control::run_event::{BoundedText, RedactionPolicy, RunEvent, RunEventPayload};
use ortak_control::runtime::{
    CancelOutcome, RuntimeAdapter, RuntimeError, RuntimeEventBatch, RuntimeRunRef,
};
use ortak_control::CompanyScope;
use uuid::Uuid;

use crate::authority::DispatchRefusal;
use crate::error::{Result, RunSupervisionError};
use crate::memory_context::{
    AdapterRunMemory, FreezeSnapshotOutcome, NoRunMemory, RunContextRepository, RunMemory,
};
use crate::repository::{
    AppendOutcome, CorrelationOutcome, DispatchAuthorization, PrepareOutcome, RunDispatchRepository,
};
use crate::state::{status_after, RunStatus};

/// Stable code recorded when the runtime reports the run terminal without
/// emitting a terminal event.
pub const CODE_RUNTIME_STREAM_ENDED: &str = "runtime_stream_ended";
/// Stable code recorded when the runtime no longer knows the correlated run.
pub const CODE_RUNTIME_RUN_UNKNOWN: &str = "runtime_run_unknown";
/// Stable code recorded for an adapter event that could not be normalized or
/// was not a valid lifecycle transition.
pub const CODE_EVENT_REJECTED: &str = "event_rejected";

/// Supervisor tuning.
#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    /// Delay before a failed dispatch row becomes due again.
    pub retry_backoff: Duration,
    /// Events requested from the runtime per pump.
    pub event_batch_limit: usize,
    /// Batches drained per [`RunSupervisor::drain`] call.
    pub max_drain_batches: usize,
    /// Redaction applied to every persisted event and cancellation reason.
    pub redaction: RedactionPolicy,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            retry_backoff: Duration::from_secs(30),
            event_batch_limit: 64,
            max_drain_batches: 16,
            redaction: RedactionPolicy::new(),
        }
    }
}

/// Outcome of dispatching one leased row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    /// The runtime started the run and the correlation plus lease completion
    /// are durable.
    Started {
        /// Durable run.
        run_id: Uuid,
        /// Runtime correlation.
        runtime_run_ref: RuntimeRunRef,
    },
    /// The correlation is durable but the lease was stale when it committed;
    /// the current lease holder will observe the correlated run and settle.
    CorrelatedUnderStaleLease {
        /// Durable run.
        run_id: Uuid,
        /// Runtime correlation.
        runtime_run_ref: RuntimeRunRef,
    },
    /// A previous attempt already correlated the run; the lease is settled.
    AlreadyCorrelated {
        /// Durable run.
        run_id: Uuid,
        /// Runtime correlation.
        runtime_run_ref: RuntimeRunRef,
        /// Status as read.
        status: RunStatus,
    },
    /// The run is already terminal; the lease is settled without a runtime call.
    AlreadyFinished {
        /// Durable run.
        run_id: Uuid,
        /// Terminal status.
        status: RunStatus,
    },
    /// A durable fact refused the dispatch before any runtime call.
    Refused {
        /// Why.
        refusal: DispatchRefusal,
        /// Outbox retry state after recording the refusal.
        retry: OutboxFailOutcome,
    },
    /// The runtime refused or failed to start the run; nothing is correlated.
    RuntimeFailed {
        /// Durable run left `queued`.
        run_id: Uuid,
        /// Bounded error text recorded on the row.
        error: String,
        /// Outbox retry state.
        retry: OutboxFailOutcome,
    },
    /// The lease was stale at some step; nothing was written under it.
    StaleLease,
}

/// Outcome of one pump.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PumpOutcome {
    /// Events were normalized and appended.
    Appended {
        /// Events stored by this pump.
        appended: usize,
        /// Events skipped because their cursor was already stored.
        duplicates: usize,
        /// Status after the append.
        status: RunStatus,
        /// True when the runtime returned fewer events than requested.
        exhausted: bool,
    },
    /// The runtime had nothing new.
    Idle {
        /// Status as read.
        status: RunStatus,
    },
    /// The run is terminal; the runtime was not called.
    Terminal {
        /// Terminal status.
        status: RunStatus,
    },
    /// The run has no runtime correlation yet; the runtime was not called.
    NotCorrelated {
        /// Status as read.
        status: RunStatus,
    },
}

/// Outcome of a supervised cancellation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CancellationOutcome {
    /// The runtime cancelled the run and the normalized cancellation is durable.
    Cancelled {
        /// Run.
        run_id: Uuid,
    },
    /// The run was already terminal; the runtime was not called.
    AlreadyTerminal {
        /// Run.
        run_id: Uuid,
        /// Terminal status.
        status: RunStatus,
    },
    /// The runtime reported the run already terminal; its remaining events
    /// were drained and `status` is the durable result.
    RuntimeAlreadyTerminal {
        /// Run.
        run_id: Uuid,
        /// Status after draining.
        status: RunStatus,
    },
}

/// Dispatches, pumps, and cancels runs over one runtime adapter.
#[derive(Clone, Debug)]
pub struct RunSupervisor<R, A, M = NoRunMemory> {
    repository: R,
    adapter: A,
    config: SupervisorConfig,
    memory: M,
}

impl<R, A> RunSupervisor<R, A, NoRunMemory>
where
    R: RunDispatchRepository + OutboxRepository,
    A: RuntimeAdapter,
{
    /// Builds the supervisor.
    pub fn new(repository: R, adapter: A, config: SupervisorConfig) -> Self {
        Self {
            repository,
            adapter,
            config,
            memory: NoRunMemory,
        }
    }
}

impl<R, A, M> RunSupervisor<R, A, M>
where
    R: RunDispatchRepository + OutboxRepository,
    A: RuntimeAdapter,
{
    /// Uses a borrowed configured memory deployment for bounded pre-run recall.
    /// Pumping and cancellation remain independent of memory availability.
    pub fn with_memory<'a, N: ortak_control::memory::MemoryAdapter>(
        self,
        memory: &'a N,
    ) -> RunSupervisor<R, A, AdapterRunMemory<'a, N>> {
        RunSupervisor {
            repository: self.repository,
            adapter: self.adapter,
            config: self.config,
            memory: AdapterRunMemory::new(memory),
        }
    }

    /// Returns the underlying repository.
    pub fn repository(&self) -> &R {
        &self.repository
    }

    /// Dispatches one leased `run_dispatch` row (see module docs).
    pub async fn dispatch(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
    ) -> Result<DispatchOutcome>
    where
        R: RunContextRepository,
        M: RunMemory,
    {
        if lease.kind != OutboxKind::RunDispatch {
            return Err(RunSupervisionError::WrongKind { found: lease.kind });
        }

        let authority = match self.repository.authorize_dispatch(scope, lease).await? {
            DispatchAuthorization::Authorized(authority) => *authority,
            DispatchAuthorization::Refused(refusal) => {
                return self.refuse(scope, lease, refusal).await
            }
            DispatchAuthorization::StaleLease => return Ok(DispatchOutcome::StaleLease),
        };
        if authority.binding().adapter != self.adapter.adapter_name() {
            let refusal = DispatchRefusal::AdapterMismatch {
                expected: authority.binding().adapter.clone(),
                found: self.adapter.adapter_name().to_owned(),
            };
            return self.refuse(scope, lease, refusal).await;
        }

        let prepared = match self.repository.prepare_run(scope, &authority).await? {
            PrepareOutcome::Refused(refusal) => return self.refuse(scope, lease, refusal).await,
            PrepareOutcome::Prepared(prepared) => prepared,
            PrepareOutcome::StaleLease => return Ok(DispatchOutcome::StaleLease),
        };
        if prepared.status.is_terminal() {
            return Ok(if self.repository.complete(scope, lease).await? {
                DispatchOutcome::AlreadyFinished {
                    run_id: prepared.run_id,
                    status: prepared.status,
                }
            } else {
                DispatchOutcome::StaleLease
            });
        }
        if let Some(runtime_run_ref) = prepared.runtime_run_ref {
            return Ok(if self.repository.complete(scope, lease).await? {
                DispatchOutcome::AlreadyCorrelated {
                    run_id: prepared.run_id,
                    runtime_run_ref,
                    status: prepared.status,
                }
            } else {
                DispatchOutcome::StaleLease
            });
        }

        if let Err(refusal) = self.memory.check(&authority).await {
            return self.refuse(scope, lease, refusal).await;
        }
        let candidate = match self
            .repository
            .load_run_snapshot(scope, &authority, prepared.run_id)
            .await
        {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => match self
                .memory
                .snapshot(&authority, prepared.run_id, &self.config.redaction)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(refusal) => return self.refuse(scope, lease, refusal).await,
            },
            Err(_) => {
                let retry = self
                    .record_failure(scope, lease, "run snapshot load failed")
                    .await?;
                return Ok(match retry {
                    OutboxFailOutcome::Stale => DispatchOutcome::StaleLease,
                    retry => DispatchOutcome::RuntimeFailed {
                        run_id: prepared.run_id,
                        error: "run snapshot load failed".to_owned(),
                        retry,
                    },
                });
            }
        };
        // Recall happened outside a transaction. This final step rederives
        // canonical authority and renews admission while storing/reusing the
        // immutable durable winner. It is mandatory on lost-ack retries too.
        let frozen = match self
            .repository
            .freeze_run_snapshot(scope, lease, &authority, prepared.run_id, &candidate)
            .await
        {
            Ok(FreezeSnapshotOutcome::Ready(snapshot)) => snapshot,
            Ok(FreezeSnapshotOutcome::Refused(refusal)) => {
                return self.refuse(scope, lease, refusal).await
            }
            Ok(FreezeSnapshotOutcome::StaleLease) => return Ok(DispatchOutcome::StaleLease),
            Err(_) => {
                let retry = self
                    .record_failure(scope, lease, "run snapshot admission failed")
                    .await?;
                return Ok(match retry {
                    OutboxFailOutcome::Stale => DispatchOutcome::StaleLease,
                    retry => DispatchOutcome::RuntimeFailed {
                        run_id: prepared.run_id,
                        error: "run snapshot admission failed".to_owned(),
                        retry,
                    },
                });
            }
        };
        let spec = frozen.spec();
        let receipt = match self.adapter.start_run(spec).await {
            Ok(receipt) => receipt,
            Err(error) => {
                let retry = self
                    .record_failure(scope, lease, &error.to_string())
                    .await?;
                return Ok(match retry {
                    OutboxFailOutcome::Stale => DispatchOutcome::StaleLease,
                    retry => DispatchOutcome::RuntimeFailed {
                        run_id: prepared.run_id,
                        error: bounded(&error.to_string()),
                        retry,
                    },
                });
            }
        };

        match self
            .repository
            .correlate_run(scope, &authority, prepared.run_id, &receipt)
            .await?
        {
            CorrelationOutcome::Correlated { lease_completed } => Ok(if lease_completed {
                DispatchOutcome::Started {
                    run_id: prepared.run_id,
                    runtime_run_ref: receipt.runtime_run_ref,
                }
            } else {
                tracing::warn!(
                    run_id = %prepared.run_id,
                    outbox_id = %lease.id,
                    "run correlated but the dispatch lease was stale at completion"
                );
                DispatchOutcome::CorrelatedUnderStaleLease {
                    run_id: prepared.run_id,
                    runtime_run_ref: receipt.runtime_run_ref,
                }
            }),
            CorrelationOutcome::AlreadyCorrelated {
                status,
                lease_completed,
            } => Ok(if lease_completed {
                DispatchOutcome::AlreadyCorrelated {
                    run_id: prepared.run_id,
                    runtime_run_ref: receipt.runtime_run_ref,
                    status,
                }
            } else {
                DispatchOutcome::StaleLease
            }),
            CorrelationOutcome::Terminal {
                status,
                lease_completed,
            } => Ok(if lease_completed {
                DispatchOutcome::AlreadyFinished {
                    run_id: prepared.run_id,
                    status,
                }
            } else {
                DispatchOutcome::StaleLease
            }),
            CorrelationOutcome::RefConflict { durable } => {
                // The durable correlation wins. The runtime run this attempt
                // received is an orphan. A failed cleanup must propagate
                // rather than report a successfully handled dispatch.
                tracing::error!(
                    run_id = %prepared.run_id,
                    durable = %durable,
                    presented = %receipt.runtime_run_ref,
                    "runtime returned a different run reference for the same idempotency key"
                );
                self.adapter
                    .cancel_run(
                        &receipt.runtime_run_ref,
                        "superseded by durable correlation",
                    )
                    .await?;
                let error = RunSupervisionError::RuntimeRefConflict {
                    run_id: prepared.run_id,
                    durable,
                    presented: receipt.runtime_run_ref,
                };
                let retry = self
                    .record_failure(scope, lease, &error.to_string())
                    .await?;
                Ok(match retry {
                    OutboxFailOutcome::Stale => DispatchOutcome::StaleLease,
                    retry => DispatchOutcome::RuntimeFailed {
                        run_id: prepared.run_id,
                        error: bounded(&error.to_string()),
                        retry,
                    },
                })
            }
        }
    }

    /// Pumps one batch of runtime events into the run (see module docs).
    pub async fn pump(&self, scope: &CompanyScope, run_id: Uuid) -> Result<PumpOutcome> {
        let state = self
            .repository
            .run_cursor_state(scope, run_id)
            .await?
            .ok_or(RunSupervisionError::UnknownRun { run_id })?;
        if state.status.is_terminal() {
            return Ok(PumpOutcome::Terminal {
                status: state.status,
            });
        }
        let Some(runtime_run_ref) = state.runtime_run_ref.clone() else {
            return Ok(PumpOutcome::NotCorrelated {
                status: state.status,
            });
        };

        let limit = self.config.event_batch_limit.max(1);
        let batch = match self
            .adapter
            .next_events(&runtime_run_ref, state.last_cursor.as_ref(), limit)
            .await
        {
            Ok(batch) => batch,
            Err(RuntimeError::UnknownRun { .. }) => {
                let failure = self.synthesized_failure(
                    run_id,
                    CODE_RUNTIME_RUN_UNKNOWN,
                    "runtime no longer knows the correlated run",
                )?;
                return self
                    .append(scope, run_id, &runtime_run_ref, vec![failure], true)
                    .await;
            }
            Err(error) => return Err(error.into()),
        };

        let exhausted = batch.terminal || batch.events.len() < limit;
        let events = self.normalize_batch(run_id, &runtime_run_ref, state.status, &batch)?;
        if events.is_empty() {
            return Ok(PumpOutcome::Idle {
                status: state.status,
            });
        }
        self.append(scope, run_id, &runtime_run_ref, events, exhausted)
            .await
    }

    /// Pumps until the run is terminal, the runtime is idle, or
    /// `max_drain_batches` batches were read.
    pub async fn drain(&self, scope: &CompanyScope, run_id: Uuid) -> Result<PumpOutcome> {
        let mut last = self.pump(scope, run_id).await?;
        for _ in 1..self.config.max_drain_batches.max(1) {
            match &last {
                PumpOutcome::Appended {
                    exhausted: false, ..
                } => last = self.pump(scope, run_id).await?,
                _ => break,
            }
        }
        Ok(last)
    }

    /// Cancels a correlated run: the adapter is called outside any
    /// transaction, then the normalized cancellation is recorded. The run is
    /// selected by its durable id; a caller cannot present a runtime
    /// reference. Replays on a terminal run return without a runtime call.
    pub async fn cancel(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        reason: &str,
    ) -> Result<CancellationOutcome> {
        let state = self
            .repository
            .run_cursor_state(scope, run_id)
            .await?
            .ok_or(RunSupervisionError::UnknownRun { run_id })?;
        if state.status.is_terminal() {
            return Ok(CancellationOutcome::AlreadyTerminal {
                run_id,
                status: state.status,
            });
        }
        let Some(runtime_run_ref) = state.runtime_run_ref.clone() else {
            return Err(RunSupervisionError::NotCorrelated {
                run_id,
                status: state.status,
            });
        };
        let reason = bounded(reason);

        match self.adapter.cancel_run(&runtime_run_ref, &reason).await? {
            CancelOutcome::Cancelled => {
                let event = RunEvent::normalize(
                    run_id,
                    Utc::now(),
                    None,
                    &RunEventPayload::RunCancelled {
                        reason: BoundedText::raw(reason),
                    },
                    &self.config.redaction,
                )?;
                match self
                    .repository
                    .append_supervised_events(scope, run_id, &runtime_run_ref, &[event])
                    .await?
                {
                    AppendOutcome::Appended { .. } => Ok(CancellationOutcome::Cancelled { run_id }),
                    AppendOutcome::RunTerminal { status } => {
                        Ok(CancellationOutcome::AlreadyTerminal { run_id, status })
                    }
                    AppendOutcome::RefMismatch { durable } => {
                        Err(RunSupervisionError::RuntimeRefConflict {
                            run_id,
                            durable: durable.unwrap_or_else(|| RuntimeRunRef(String::new())),
                            presented: runtime_run_ref,
                        })
                    }
                }
            }
            CancelOutcome::AlreadyTerminal => {
                let status = match self.drain(scope, run_id).await? {
                    PumpOutcome::Appended { status, .. }
                    | PumpOutcome::Idle { status }
                    | PumpOutcome::Terminal { status }
                    | PumpOutcome::NotCorrelated { status } => status,
                };
                Ok(CancellationOutcome::RuntimeAlreadyTerminal { run_id, status })
            }
        }
    }

    async fn append(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        runtime_run_ref: &RuntimeRunRef,
        events: Vec<RunEvent>,
        exhausted: bool,
    ) -> Result<PumpOutcome> {
        match self
            .repository
            .append_supervised_events(scope, run_id, runtime_run_ref, &events)
            .await?
        {
            AppendOutcome::Appended {
                sequences,
                duplicate_cursors,
                status,
            } => Ok(PumpOutcome::Appended {
                appended: sequences.len(),
                duplicates: duplicate_cursors.len(),
                status,
                exhausted,
            }),
            AppendOutcome::RunTerminal { status } => Ok(PumpOutcome::Terminal { status }),
            AppendOutcome::RefMismatch { durable } => {
                Err(RunSupervisionError::RuntimeRefConflict {
                    run_id,
                    durable: durable.unwrap_or_else(|| RuntimeRunRef(String::new())),
                    presented: runtime_run_ref.clone(),
                })
            }
        }
    }

    /// Normalizes one adapter batch into persistable events.
    ///
    /// - A `run.started` naming another runtime run, an event that is not a
    ///   valid transition from the projected status, or an event that fails
    ///   normalization becomes a non-terminal `error.raised` under the same
    ///   cursor so the durable cursor still advances.
    /// - Events after the first terminal event are dropped.
    /// - A batch the runtime marks terminal without a terminal event ends the
    ///   run with a synthesized `run.failed`.
    fn normalize_batch(
        &self,
        run_id: Uuid,
        runtime_run_ref: &RuntimeRunRef,
        current: RunStatus,
        batch: &RuntimeEventBatch,
    ) -> Result<Vec<RunEvent>> {
        let mut events = Vec::with_capacity(batch.events.len() + 1);
        let mut projected = current;
        let mut closed = false;
        for raw in &batch.events {
            let cursor = Some(raw.cursor.0.clone());
            let payload = match &raw.payload {
                RunEventPayload::RunStarted {
                    runtime_run_ref: reported,
                } if reported != &runtime_run_ref.0 => {
                    rejected_event("runtime reported a run.started for a different runtime run")
                }
                payload => match status_after(projected, payload.event_type()) {
                    Ok(_) => payload.clone(),
                    Err(invalid) => rejected_event(&invalid.to_string()),
                },
            };
            let event = match RunEvent::normalize(
                run_id,
                raw.occurred_at,
                cursor.clone(),
                &payload,
                &self.config.redaction,
            ) {
                Ok(event) => event,
                Err(error) => RunEvent::normalize(
                    run_id,
                    raw.occurred_at,
                    cursor,
                    &rejected_event(&error.to_string()),
                    &self.config.redaction,
                )?,
            };
            projected = status_after(projected, event.event_type())?;
            let terminal = event.event_type().is_terminal();
            events.push(event);
            if terminal {
                closed = true;
                let dropped = batch.events.len() - events.len();
                if dropped > 0 {
                    tracing::warn!(
                        run_id = %run_id,
                        dropped,
                        "runtime emitted events after a terminal event; dropping them"
                    );
                }
                break;
            }
        }
        if batch.terminal && !closed {
            events.push(self.synthesized_failure(
                run_id,
                CODE_RUNTIME_STREAM_ENDED,
                "runtime reported the run terminal without a terminal event",
            )?);
        }
        Ok(events)
    }

    fn synthesized_failure(&self, run_id: Uuid, code: &str, message: &str) -> Result<RunEvent> {
        Ok(RunEvent::normalize(
            run_id,
            Utc::now(),
            None,
            &RunEventPayload::RunFailed {
                code: code.to_owned(),
                message: BoundedText::raw(message),
            },
            &self.config.redaction,
        )?)
    }

    async fn refuse(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        refusal: DispatchRefusal,
    ) -> Result<DispatchOutcome> {
        let retry = self
            .record_failure(scope, lease, &format!("dispatch refused: {refusal}"))
            .await?;
        Ok(match retry {
            OutboxFailOutcome::Stale => DispatchOutcome::StaleLease,
            retry => DispatchOutcome::Refused { refusal, retry },
        })
    }

    async fn record_failure(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        error: &str,
    ) -> Result<OutboxFailOutcome> {
        let error = bounded(error);
        let retry_after = Utc::now()
            + chrono::Duration::from_std(self.config.retry_backoff)
                .unwrap_or_else(|_| chrono::Duration::seconds(30));
        tracing::warn!(
            outbox_id = %lease.id,
            attempt = lease.attempt_count,
            error = %error,
            "run dispatch attempt failed"
        );
        Ok(self
            .repository
            .fail(scope, lease, &error, retry_after)
            .await?)
    }
}

fn rejected_event(detail: &str) -> RunEventPayload {
    RunEventPayload::ErrorRaised {
        code: CODE_EVENT_REJECTED.to_owned(),
        message: BoundedText::raw(detail),
        retryable: false,
    }
}

/// Bounds and sanitizes free text before it is stored on an outbox or run row.
fn bounded(text: &str) -> String {
    Detail::new(text).as_str().to_owned()
}
