-- A successful score is not permission to dispatch after its inbox lease ends.
-- Production routing already holds inbox before root/recipient/outbox locks.
-- Keep same-generation zero-wake finalization available after scorer timeout.
CREATE FUNCTION ortak_check_routing_claim_expiry() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_claim RECORD;
BEGIN
    -- Legacy rows without Office authority remain inert at runtime admission,
    -- matching0048. Every production routing commit supplies its Office witness.
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
    -- Evaluate the clock after lock acquisition, including any finalization wait.
    IF NOT FOUND OR current_claim.state NOT IN ('claimed', 'decided')
       OR current_claim.claim_generation IS DISTINCT FROM NEW.inbox_claim_generation
       OR current_claim.claim_expires_at IS NULL
       OR clock_timestamp() >= current_claim.claim_expires_at THEN
        RAISE EXCEPTION 'ortak: waking routing claim changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER ortak_routing_claim_expiry_at_commit
AFTER INSERT ON routing_decisions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ortak_check_routing_claim_expiry();
