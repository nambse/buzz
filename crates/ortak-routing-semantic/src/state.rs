use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::response::Parsed;

#[derive(Default)]
pub(crate) struct State {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    cache: BTreeMap<[u8; 32], Cached>,
    failures: u8,
    open_until: Option<Instant>,
    generation: u64,
    probing: bool,
}
struct Cached {
    inserted: Instant,
    result: Parsed,
}

/// A cancelled request is a failed attempt, including a cancelled recovery probe.
pub(crate) struct Attempt<'a> {
    state: &'a State,
    generation: u64,
    probe: bool,
    finished: bool,
}

impl State {
    pub fn cached(&self, key: &[u8; 32]) -> Option<Parsed> {
        let mut state = self.inner.lock().ok()?;
        let now = Instant::now();
        state
            .cache
            .retain(|_, v| now.duration_since(v.inserted) < Duration::from_secs(300));
        state.cache.get(key).map(|v| v.result.clone())
    }

    pub fn insert(&self, key: [u8; 32], result: Parsed) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.cache.len() >= 256 && !state.cache.contains_key(&key) {
            let oldest = state
                .cache
                .iter()
                .min_by_key(|(_, v)| v.inserted)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                state.cache.remove(&oldest);
            }
        }
        state.cache.insert(
            key,
            Cached {
                inserted: Instant::now(),
                result,
            },
        );
    }

    pub fn attempt(&self) -> Result<Attempt<'_>, &'static str> {
        let mut state = self.inner.lock().map_err(|_| "scorer_state_unavailable")?;
        let probe = if let Some(until) = state.open_until {
            if until > Instant::now() || state.probing {
                return Err("circuit_open");
            }
            state.probing = true;
            true
        } else {
            false
        };
        Ok(Attempt {
            state: self,
            generation: state.generation,
            probe,
            finished: false,
        })
    }
}

impl Attempt<'_> {
    pub fn finish(mut self, success: bool) {
        self.record(success);
        self.finished = true;
    }

    fn record(&self, success: bool) {
        let Ok(mut state) = self.state.inner.lock() else {
            return;
        };
        // A late success cannot close a circuit opened by a newer failure.
        if state.generation != self.generation {
            return;
        }
        if success {
            state.failures = 0;
            if self.probe {
                state.open_until = None;
                state.probing = false;
                state.generation = state.generation.wrapping_add(1);
            }
        } else {
            state.failures = state.failures.saturating_add(1);
            if self.probe || state.failures >= 3 {
                state.open_until = Some(Instant::now() + Duration::from_secs(30));
                state.probing = false;
                state.generation = state.generation.wrapping_add(1);
            }
        }
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.record(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_probe_and_stale_success_cannot_clear_the_circuit() {
        let state = State::default();
        let stale = state.attempt().unwrap();
        for _ in 0..3 {
            state.attempt().unwrap().finish(false);
        }
        stale.finish(true);
        assert!(state.attempt().is_err());
        state.inner.lock().unwrap().open_until = Some(Instant::now());
        let probe = state.attempt().unwrap();
        assert!(state.attempt().is_err());
        drop(probe);
        assert!(state.attempt().is_err());
        state.inner.lock().unwrap().open_until = Some(Instant::now());
        state.attempt().unwrap().finish(true);
        assert!(state.attempt().is_ok());
    }

    #[test]
    fn cache_expires_and_never_exceeds_its_bound() {
        let state = State::default();
        let result = Parsed {
            scores: vec![],
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            response_bytes: 0,
        };
        for n in 0u16..300 {
            let mut key = [0; 32];
            key[..2].copy_from_slice(&n.to_be_bytes());
            state.insert(key, result.clone());
        }
        assert_eq!(state.inner.lock().unwrap().cache.len(), 256);
        let key = *state.inner.lock().unwrap().cache.keys().next().unwrap();
        state
            .inner
            .lock()
            .unwrap()
            .cache
            .get_mut(&key)
            .unwrap()
            .inserted = Instant::now() - Duration::from_secs(301);
        assert!(state.cached(&key).is_none());
    }
}
