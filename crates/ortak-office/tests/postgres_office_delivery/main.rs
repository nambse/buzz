//! Production-seam Postgres tests for the Office delivery outbox slice.
//!
//! Run with a local database that can receive the embedded migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-office -- --ignored`

use std::time::Duration;

use chrono::Utc;
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::ports::{CompanyDirectory, OutboxRepository};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::RoutingPolicy;
use ortak_office::fakes::{FakeOfficePublisher, FakeOfficeSigner};
use ortak_office::{
    AuthorizedOfficePublish, BindingRejection, DeliveryConfig, DeliveryOutcome, EnqueueOutcome,
    FreezeOutcome, OfficeDeliveryError, OfficeDeliveryRepository, OfficeDeliveryService,
    OfficeEventError, OfficePublishDraft, OfficeSigner, PublishReceipt, KIND_STREAM_MESSAGE,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

mod fixture;
use fixture::*;
mod canonical;
use canonical::*;
mod cohort;
mod delivery;
mod fencing;
mod identity;
mod transport;
