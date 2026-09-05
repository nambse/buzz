use std::time::Instant;

use crate::{
    unavailable, unsupported, HonchoEmployeeBinding, HonchoMemoryAdapter, MemoryCapability,
    MemoryError,
};

#[derive(Default)]
pub(crate) struct Witness {
    pub generation: u64,
    pub expires: Option<Instant>,
}

#[derive(Clone, Copy)]
pub(crate) enum IoGate {
    Witness(u64, MemoryCapability),
    Validation(u64),
}

impl HonchoMemoryAdapter {
    pub(crate) fn begin_validation(
        &self,
        allowed: &HonchoEmployeeBinding,
    ) -> Result<u64, MemoryError> {
        let mut states = self
            .witnesses
            .lock()
            .map_err(|_| unavailable("memory validation state unavailable"))?;
        let state = states.entry(allowed.employee_id.clone()).or_default();
        state.expires = None;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| unavailable("memory validation generation exhausted"))?;
        Ok(state.generation)
    }

    pub(crate) fn publish_validation(
        &self,
        allowed: &HonchoEmployeeBinding,
        generation: u64,
    ) -> Result<(), MemoryError> {
        let mut states = self
            .witnesses
            .lock()
            .map_err(|_| unavailable("memory validation state unavailable"))?;
        let state = states
            .get_mut(&allowed.employee_id)
            .filter(|state| state.generation == generation)
            .ok_or_else(|| unavailable("memory validation was superseded"))?;
        state.expires = Some(Instant::now() + self.config.witness_lifetime);
        Ok(())
    }

    pub(crate) fn check_gate(
        &self,
        allowed: &HonchoEmployeeBinding,
        gate: IoGate,
    ) -> Result<(), MemoryError> {
        let states = self
            .witnesses
            .lock()
            .map_err(|_| unavailable("memory validation state unavailable"))?;
        let state = states.get(&allowed.employee_id);
        match gate {
            IoGate::Witness(generation, capability) => {
                if state.is_some_and(|state| {
                    state.generation == generation
                        && state
                            .expires
                            .is_some_and(|expires| expires > Instant::now())
                }) {
                    Ok(())
                } else {
                    Err(unsupported(capability))
                }
            }
            IoGate::Validation(generation) => {
                if state.is_some_and(|state| state.generation == generation) {
                    Ok(())
                } else {
                    Err(unavailable("memory validation was superseded"))
                }
            }
        }
    }
}
