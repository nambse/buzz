-- SOURCE FRAGMENT ONLY: root owns allocation, numbered migration and convergence.
-- Hints contain public scope IDs only. LISTEN precedes durable current reads;
-- a lost hint is repaired by the next bounded signed subscription, not a cursor.
CREATE FUNCTION ortak_routing_notify() RETURNS TRIGGER AS $$
DECLARE
    message TEXT;
BEGIN
    IF TG_TABLE_NAME = 'routing_decisions' THEN
        message := encode(NEW.message_id, 'hex');
    ELSIF TG_TABLE_NAME <> 'office_authority_generations' THEN
        RAISE EXCEPTION 'invalid routing notification source' USING ERRCODE='55000';
    END IF;
    PERFORM pg_notify('ortak_routing_v1', json_build_object(
        'company_id', NEW.company_id, 'message_id', message)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_routing_decisions_notify AFTER INSERT ON routing_decisions
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();
-- Existing canonical Office fences advance this row in the authority mutation
-- transaction, including membership, identity, community and source removal.
CREATE TRIGGER trg_routing_authority_notify AFTER INSERT OR UPDATE ON office_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();
