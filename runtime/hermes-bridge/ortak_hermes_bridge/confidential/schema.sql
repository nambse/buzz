-- Version 1: ordinary run registry is shared for unique start/cancel ownership.
-- Only this explicit side table selects the protected mode; no legacy backfill.
CREATE TABLE IF NOT EXISTS confidential_runs (
    start_key TEXT PRIMARY KEY REFERENCES runs(start_key),
    format TEXT NOT NULL CHECK(format='ortak-confidential-journal/1'),
    identity BLOB NOT NULL CHECK(typeof(identity)='blob' AND length(identity)<=2048),
    snapshot BLOB NOT NULL CHECK(typeof(snapshot)='blob' AND length(snapshot)<=98304),
    key_id TEXT NOT NULL UNIQUE,
    snapshot_nonce BLOB NOT NULL CHECK(length(snapshot_nonce)=12),
    UNIQUE(key_id,snapshot_nonce)
);
CREATE TABLE IF NOT EXISTS confidential_events (
    start_key TEXT NOT NULL REFERENCES confidential_runs(start_key),
    sequence INTEGER NOT NULL CHECK(sequence BETWEEN 1 AND 512),
    occurred_at TEXT NOT NULL,
    envelope BLOB NOT NULL CHECK(typeof(envelope)='blob' AND length(envelope)<=98304),
    key_id TEXT NOT NULL REFERENCES confidential_runs(key_id),
    nonce BLOB NOT NULL CHECK(length(nonce)=12),
    PRIMARY KEY(start_key,sequence), UNIQUE(key_id,nonce)
);
CREATE TABLE IF NOT EXISTS confidential_status (
    start_key TEXT PRIMARY KEY REFERENCES confidential_runs(start_key),
    code TEXT NOT NULL CHECK(code IN ('executor_interrupted','executor_unavailable','policy_denied',
      'provider_failed','deadline_exceeded','provider_incomplete','provider_response_invalid','invalid_output',
      'credential_denied','runtime_selection_changed','unsupported_hermes_tool_selection','cancelled')),
    occurred_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS confidential_start_guard BEFORE INSERT ON confidential_runs BEGIN
    SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM runs WHERE start_key=NEW.start_key
      AND status='accepted' AND sequence=0 AND fingerprint IS NOT NULL)
      OR EXISTS(SELECT 1 FROM events WHERE start_key=NEW.start_key)
      OR EXISTS(SELECT 1 FROM workspace_runs WHERE start_key=NEW.start_key)
      OR EXISTS(SELECT 1 FROM profile_probes WHERE start_key=NEW.start_key)
      OR EXISTS(SELECT 1 FROM private_failure_diagnostics WHERE start_key=NEW.start_key)
      THEN RAISE(ABORT,'confidential mode conflict') END;
END;
CREATE TRIGGER IF NOT EXISTS confidential_registry_guard BEFORE UPDATE ON runs
WHEN EXISTS(SELECT 1 FROM confidential_runs WHERE start_key=OLD.start_key) BEGIN
    SELECT CASE WHEN NEW.start_key IS NOT OLD.start_key OR NEW.fingerprint IS NOT OLD.fingerprint
      OR NEW.started_at IS NOT OLD.started_at
      OR (NEW.status<>OLD.status AND NOT (
        (OLD.status='accepted' AND NEW.status IN ('running','cancelling','failed')) OR
        (OLD.status='running' AND NEW.status IN ('completed','cancelling','failed')) OR
        (OLD.status='cancelling' AND NEW.status='cancelled')))
      OR (NEW.sequence<>OLD.sequence AND NOT (NEW.sequence=OLD.sequence+1 AND EXISTS(
        SELECT 1 FROM confidential_events WHERE start_key=OLD.start_key AND sequence=NEW.sequence)))
      THEN RAISE(ABORT,'confidential registry conflict') END;
END;
CREATE TRIGGER IF NOT EXISTS confidential_run_immutable BEFORE UPDATE ON confidential_runs BEGIN
    SELECT RAISE(ABORT,'immutable confidential run');
END;
CREATE TRIGGER IF NOT EXISTS confidential_run_retained BEFORE DELETE ON confidential_runs BEGIN
    SELECT RAISE(ABORT,'retained confidential run');
END;
CREATE TRIGGER IF NOT EXISTS confidential_event_guard BEFORE INSERT ON confidential_events BEGIN
    SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM confidential_runs c JOIN runs r USING(start_key)
      WHERE c.start_key=NEW.start_key AND c.key_id=NEW.key_id AND r.sequence+1=NEW.sequence
      AND r.status IN ('accepted','running')) THEN RAISE(ABORT,'confidential event conflict') END;
END;
CREATE TRIGGER IF NOT EXISTS confidential_event_cursor AFTER INSERT ON confidential_events BEGIN
    UPDATE runs SET sequence=NEW.sequence WHERE start_key=NEW.start_key;
END;
CREATE TRIGGER IF NOT EXISTS confidential_event_immutable BEFORE UPDATE ON confidential_events BEGIN
    SELECT RAISE(ABORT,'immutable confidential event');
END;
CREATE TRIGGER IF NOT EXISTS confidential_event_retained BEFORE DELETE ON confidential_events BEGIN
    SELECT RAISE(ABORT,'retained confidential event');
END;
CREATE TRIGGER IF NOT EXISTS confidential_status_immutable BEFORE UPDATE ON confidential_status BEGIN
    SELECT RAISE(ABORT,'immutable confidential status');
END;
CREATE TRIGGER IF NOT EXISTS confidential_status_retained BEFORE DELETE ON confidential_status BEGIN
    SELECT RAISE(ABORT,'retained confidential status');
END;
CREATE TRIGGER IF NOT EXISTS ordinary_event_mode_guard BEFORE INSERT ON events BEGIN
    SELECT CASE WHEN EXISTS(SELECT 1 FROM confidential_runs WHERE start_key=NEW.start_key)
      THEN RAISE(ABORT,'confidential mode required') END;
END;
CREATE TRIGGER IF NOT EXISTS ordinary_workspace_mode_guard BEFORE INSERT ON workspace_runs BEGIN
    SELECT CASE WHEN EXISTS(SELECT 1 FROM confidential_runs WHERE start_key=NEW.start_key)
      THEN RAISE(ABORT,'confidential mode required') END;
END;
CREATE TRIGGER IF NOT EXISTS ordinary_probe_mode_guard BEFORE INSERT ON profile_probes BEGIN
    SELECT CASE WHEN EXISTS(SELECT 1 FROM confidential_runs WHERE start_key=NEW.start_key)
      THEN RAISE(ABORT,'confidential mode required') END;
END;
CREATE TRIGGER IF NOT EXISTS ordinary_diagnostic_mode_guard BEFORE INSERT ON private_failure_diagnostics BEGIN
    SELECT CASE WHEN EXISTS(SELECT 1 FROM confidential_runs WHERE start_key=NEW.start_key)
      THEN RAISE(ABORT,'confidential mode required') END;
END;
