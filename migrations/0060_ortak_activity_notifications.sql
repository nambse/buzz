-- Transactional hints for authorized durable Activity streams.
-- Transactional NOTIFY carries public scope hints only. Durable data and the
-- company authority fence stay authoritative; a lost hint is repaired on
-- reconnect after LISTEN, never treated as event delivery or a cursor.
CREATE FUNCTION ortak_activity_notify() RETURNS TRIGGER AS $$
DECLARE
    facts JSONB := to_jsonb(NEW);
    run UUID;
BEGIN
    IF TG_ARGV[0] <> '' THEN
        run := (facts ->> TG_ARGV[0])::UUID;
        IF run IS NULL THEN RETURN NEW; END IF;
    END IF;
    PERFORM pg_notify('ortak_activity_v1', json_build_object(
        'company_id', (facts ->> 'company_id')::UUID, 'run_id', run)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_activity_events AFTER INSERT ON run_events
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_runs AFTER INSERT OR UPDATE OF status, delivery_intent, cancel_reason, error_code, error_message, started_at, finished_at ON runs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('id');
CREATE TRIGGER trg_activity_cancel_requests AFTER INSERT OR UPDATE OF status, last_error_code, acknowledged_at ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_cancellations AFTER INSERT OR UPDATE OF state, last_error_code, acknowledged_at ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_office_outputs AFTER INSERT OR UPDATE OF state, outbox_id, last_error_code ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_outbox AFTER INSERT OR UPDATE OF state, last_error, delivered_at ON outbox
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_memory_writes AFTER INSERT OR UPDATE OF state, attempt_count, next_attempt_at, last_error_code, receipt, acknowledged_at ON runtime_memory_writes
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_context AFTER INSERT ON run_context_snapshots
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
-- Existing Office triggers bump this generation in the SAME transaction as
-- channel/relay membership, audience, user, company or source authority changes.
CREATE TRIGGER trg_activity_authority AFTER INSERT OR UPDATE ON office_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('');
