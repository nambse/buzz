//! Capture a candidate from the actual saga, with its real prepared authority.
#![allow(dead_code)]
use ortak_control::ports::ProvisioningRepository;
use ortak_control::provisioning::{
    ActivationTarget, IdentityReservation, OperationUpdate, ProvisioningOperation,
    ProvisioningRequest, RevisionActivation, StepRecord,
};
use ortak_control::{CompanyScope, ControlError, Result};
use ortak_domain::EmployeeId;
use std::{sync::Mutex, time::Duration};
use uuid::Uuid;

pub struct Capture<'a, R> {
    pub inner: &'a R,
    candidate: Mutex<Option<RevisionActivation>>,
}
impl<'a, R> Capture<'a, R> {
    pub fn new(inner: &'a R) -> Self {
        Self {
            inner,
            candidate: Mutex::new(None),
        }
    }
    pub fn has_candidate(&self) -> bool {
        self.candidate.lock().unwrap().is_some()
    }
    pub fn take(&self) -> RevisionActivation {
        self.candidate
            .lock()
            .unwrap()
            .take()
            .expect("actual saga produced activation")
    }
}
impl<R: ProvisioningRepository> ProvisioningRepository for Capture<'_, R> {
    async fn begin_operation(
        &self,
        s: &CompanyScope,
        r: &ProvisioningRequest,
    ) -> Result<ProvisioningOperation> {
        self.inner.begin_operation(s, r).await
    }
    async fn load_operation(
        &self,
        s: &CompanyScope,
        id: Uuid,
    ) -> Result<Option<ProvisioningOperation>> {
        self.inner.load_operation(s, id).await
    }
    async fn update_operation(
        &self,
        s: &CompanyScope,
        id: Uuid,
        r: &OperationUpdate,
    ) -> Result<()> {
        self.inner.update_operation(s, id, r).await
    }
    async fn record_step(&self, s: &CompanyScope, id: Uuid, r: &StepRecord) -> Result<()> {
        self.inner.record_step(s, id, r).await
    }
    async fn reserve_employee_identity(
        &self,
        s: &CompanyScope,
        id: &EmployeeId,
    ) -> Result<IdentityReservation> {
        self.inner.reserve_employee_identity(s, id).await
    }
    async fn prepare_activation(
        &self,
        s: &CompanyScope,
        id: Uuid,
        r: &StepRecord,
        t: Duration,
    ) -> Result<ActivationTarget> {
        self.inner.prepare_activation(s, id, r, t).await
    }
    async fn activate_revision(
        &self,
        _s: &CompanyScope,
        _id: Uuid,
        r: &RevisionActivation,
    ) -> Result<Uuid> {
        *self.candidate.lock().unwrap() = Some(r.clone());
        Err(ControlError::InvalidData(
            "fixture captured activation before commit".into(),
        ))
    }
}
