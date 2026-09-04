//! In-memory signer and publisher for tests and dry runs.
//!
//! [`FakeOfficeSigner`] holds ephemeral keys generated in memory for the
//! lifetime of the fake; nothing is read from or written to disk, and the
//! `Debug` output never includes key material.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

use nostr::JsonUtil;
use ortak_control::adapter::Detail;
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::{CompanyScope, MessageId};
use uuid::Uuid;

use crate::event::FrozenSignedEvent;
use crate::publisher::{OfficePublishError, OfficePublisher, PublishReceipt};
use crate::signer::{OfficeSigner, OfficeSignerError, SigningRequest};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct SignerState {
    signers: HashMap<String, nostr::Keys>,
    calls: u32,
    tamper_content: bool,
    unavailable: bool,
}

/// In-memory signer keyed by opaque signer reference.
#[derive(Default)]
pub struct FakeOfficeSigner {
    state: Mutex<SignerState>,
}

impl fmt::Debug for FakeOfficeSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakeOfficeSigner")
            .field("signer_refs", &state.signers.keys().collect::<Vec<_>>())
            .field("calls", &state.calls)
            .finish_non_exhaustive()
    }
}

impl FakeOfficeSigner {
    /// A signer with no references registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a signer reference backed by a freshly generated in-memory key.
    pub fn with_generated_signer(self, signer_ref: &str) -> Self {
        lock(&self.state)
            .signers
            .insert(signer_ref.to_owned(), nostr::Keys::generate());
        self
    }

    /// Public key a registered reference produces.
    pub fn public_key(&self, signer_ref: &str) -> Option<OfficePublicKey> {
        let state = lock(&self.state);
        let keys = state.signers.get(signer_ref)?;
        OfficePublicKey::parse_hex(&keys.public_key().to_hex()).ok()
    }

    /// Number of `sign` calls so far.
    pub fn sign_calls(&self) -> u32 {
        lock(&self.state).calls
    }

    /// Makes the fake alter the content before signing, simulating a signer
    /// that does not sign what it was given.
    pub fn set_tamper_content(&self, tamper: bool) {
        lock(&self.state).tamper_content = tamper;
    }

    /// Makes every call fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }
}

impl OfficeSigner for FakeOfficeSigner {
    async fn sign(&self, request: &SigningRequest) -> Result<FrozenSignedEvent, OfficeSignerError> {
        let (keys, tamper) = {
            let mut state = lock(&self.state);
            state.calls += 1;
            if state.unavailable {
                return Err(OfficeSignerError::Unavailable {
                    detail: Detail::new("fake signer is offline"),
                });
            }
            let keys = state
                .signers
                .get(request.signer_ref().as_str())
                .cloned()
                .ok_or_else(|| OfficeSignerError::unresolvable(request.signer_ref()))?;
            (keys, state.tamper_content)
        };
        let mut unsigned = request.unsigned().to_nostr()?;
        // A real signer authors under whatever key it holds; the port must
        // notice when that is not the expected key.
        unsigned.pubkey = keys.public_key();
        if tamper {
            unsigned.content.push_str(" (altered by signer)");
        }
        let event =
            unsigned
                .sign_with_keys(&keys)
                .map_err(|error| OfficeSignerError::Rejected {
                    detail: Detail::new(error.to_string()),
                })?;
        Ok(FrozenSignedEvent::seal(
            request.unsigned(),
            event.as_json().as_bytes(),
        )?)
    }
}

/// One event the fake publisher accepted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedEvent {
    /// Company scope of the publish.
    pub company_id: Uuid,
    /// Signed event id.
    pub event_id: MessageId,
    /// Bytes exactly as received.
    pub signed_bytes: Vec<u8>,
}

#[derive(Debug, Default)]
struct PublisherState {
    published: Vec<PublishedEvent>,
    calls: u32,
    fail_next: u32,
    unavailable: bool,
}

/// In-memory Office that verifies incoming bytes and deduplicates by event id.
#[derive(Debug, Default)]
pub struct FakeOfficePublisher {
    state: Mutex<PublisherState>,
}

impl FakeOfficePublisher {
    /// An available, empty Office.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next `count` publishes fail with `Unavailable`.
    pub fn fail_next(&self, count: u32) {
        lock(&self.state).fail_next = count;
    }

    /// Makes every publish fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    /// Events accepted so far, in order.
    pub fn published(&self) -> Vec<PublishedEvent> {
        lock(&self.state).published.clone()
    }

    /// Number of `publish` calls, including failed ones.
    pub fn publish_calls(&self) -> u32 {
        lock(&self.state).calls
    }
}

impl OfficePublisher for FakeOfficePublisher {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        let mut state = lock(&self.state);
        state.calls += 1;
        if state.unavailable {
            return Err(OfficePublishError::Unavailable {
                detail: Detail::new("fake office is offline"),
            });
        }
        if state.fail_next > 0 {
            state.fail_next -= 1;
            return Err(OfficePublishError::Unavailable {
                detail: Detail::new("fake office dropped the connection"),
            });
        }
        // Behave like the relay: accept only bytes that verify on their own.
        let parsed = nostr::Event::from_json(event.signed_bytes()).map_err(|error| {
            OfficePublishError::Rejected {
                detail: Detail::new(error.to_string()),
            }
        })?;
        if parsed.verify().is_err() || parsed.id.to_bytes() != *event.event_id().as_bytes() {
            return Err(OfficePublishError::Rejected {
                detail: Detail::new("event does not verify"),
            });
        }
        if state
            .published
            .iter()
            .any(|published| published.event_id == event.event_id())
        {
            return Ok(PublishReceipt::AlreadyPresent);
        }
        state.published.push(PublishedEvent {
            company_id: scope.company_id(),
            event_id: event.event_id(),
            signed_bytes: event.signed_bytes().to_vec(),
        });
        Ok(PublishReceipt::Accepted)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use ortak_domain::{CredentialRef, EmployeeId};

    use super::*;
    use crate::event::{
        OfficeEventError, OfficePublishIntent, OfficePublishPayload, StoredSignedEvent,
        KIND_STREAM_MESSAGE,
    };
    use crate::repository::AuthorizedOfficePublish;

    const SIGNER_REF: &str = "credential://office/cem";

    /// An authorized publish as the repository seam would derive it, built
    /// through the crate-private constructor (tests live inside the crate).
    fn request(
        signer: &FakeOfficeSigner,
        signer_ref: &str,
        key_of: &str,
    ) -> AuthorizedOfficePublish {
        AuthorizedOfficePublish::new(
            Uuid::new_v4(),
            OfficePublishIntent {
                company_id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
                employee_id: EmployeeId::parse("cem").expect("id"),
                employee_revision_id: Uuid::new_v4(),
                kind: KIND_STREAM_MESSAGE,
                tags: vec![vec!["h".to_owned(), "general".to_owned()]],
                content: "Merhaba".to_owned(),
            },
            Uuid::new_v4(),
            CredentialRef::parse(signer_ref).expect("ref"),
            signer.public_key(key_of).expect("registered"),
        )
    }

    #[tokio::test]
    async fn signer_output_is_verified_and_round_trips_through_storage() {
        let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER_REF);
        let request = request(&signer, SIGNER_REF, SIGNER_REF);
        let signing = request
            .signing_request(Utc::now())
            .expect("signing request");
        let frozen = signer.sign(&signing).await.expect("sign");

        assert_eq!(frozen.public_key(), request.public_key());
        assert_eq!(frozen.fingerprint(), request.fingerprint());
        let event = nostr::Event::from_json(frozen.signed_bytes()).expect("parse");
        event.verify().expect("frozen bytes verify on their own");
        assert_eq!(event.id.to_bytes(), *frozen.event_id().as_bytes());
        assert_eq!(event.content, "Merhaba");

        let stored = FrozenSignedEvent::from_stored(
            &StoredSignedEvent {
                company_id: request.company_id(),
                run_id: request.run_id(),
                event_id: frozen.event_id().as_bytes(),
                signed_bytes: frozen.signed_bytes(),
            },
            &request.payload(),
        )
        .expect("stored bytes re-verify");
        assert_eq!(stored, frozen);

        // Swapped id, foreign fingerprint, and a damaged signature all fail closed.
        let other_id = MessageId::from_bytes([7u8; 32]);
        assert_eq!(
            FrozenSignedEvent::from_stored(
                &StoredSignedEvent {
                    company_id: request.company_id(),
                    run_id: request.run_id(),
                    event_id: other_id.as_bytes(),
                    signed_bytes: frozen.signed_bytes(),
                },
                &request.payload(),
            )
            .unwrap_err(),
            OfficeEventError::StoredIdMismatch
        );
        let mut other_intent = request.intent().clone();
        other_intent.content.push('!');
        assert_eq!(
            FrozenSignedEvent::from_stored(
                &StoredSignedEvent {
                    company_id: request.company_id(),
                    run_id: request.run_id(),
                    event_id: frozen.event_id().as_bytes(),
                    signed_bytes: frozen.signed_bytes(),
                },
                &OfficePublishPayload::new(&other_intent, *request.public_key()),
            )
            .unwrap_err(),
            OfficeEventError::FingerprintMismatch
        );
        let mut damaged: serde_json::Value =
            serde_json::from_slice(frozen.signed_bytes()).expect("json");
        damaged["sig"] = serde_json::Value::String("0".repeat(128));
        assert_eq!(
            FrozenSignedEvent::seal(signing.unsigned(), damaged.to_string().as_bytes())
                .unwrap_err(),
            OfficeEventError::InvalidSignature
        );
    }

    #[tokio::test]
    async fn signer_producing_another_key_fails_closed() {
        let signer = FakeOfficeSigner::new()
            .with_generated_signer(SIGNER_REF)
            .with_generated_signer("credential://office/other");
        // Expect the other key while signing through Cem's reference.
        let request = request(&signer, SIGNER_REF, "credential://office/other");
        let signing = request
            .signing_request(Utc::now())
            .expect("signing request");
        let error = signer.sign(&signing).await.unwrap_err();
        assert!(
            matches!(
                error,
                OfficeSignerError::Verification(OfficeEventError::PublicKeyMismatch { .. })
            ),
            "{error:?}"
        );
        assert!(!error.is_retryable());
    }

    #[tokio::test]
    async fn signer_altering_the_event_fails_closed() {
        let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER_REF);
        signer.set_tamper_content(true);
        let request = request(&signer, SIGNER_REF, SIGNER_REF);
        let signing = request
            .signing_request(Utc::now())
            .expect("signing request");
        assert_eq!(
            signer.sign(&signing).await.unwrap_err(),
            OfficeSignerError::Verification(OfficeEventError::EventMismatch)
        );
    }

    #[tokio::test]
    async fn unknown_reference_reports_only_the_reference() {
        let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER_REF);
        let request = request(&signer, "credential://office/missing", SIGNER_REF);
        let signing = request
            .signing_request(Utc::now())
            .expect("signing request");
        assert_eq!(
            signer.sign(&signing).await.unwrap_err(),
            OfficeSignerError::SignerUnresolvable {
                signer_ref: "credential://office/missing".to_owned()
            }
        );
        assert!(!format!("{signer:?}").contains("secret"));
    }
}
