-- Frozen channel identity keeps retained run provenance tied to the same pair.
CREATE FUNCTION ortak_private_dm_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.channel_type = 'dm' AND OLD.participant_hash IS NOT NULL
       AND (NEW.channel_type IS DISTINCT FROM OLD.channel_type
            OR NEW.visibility IS DISTINCT FROM OLD.visibility
            OR NEW.participant_hash IS DISTINCT FROM OLD.participant_hash) THEN
        RAISE EXCEPTION 'A retained DM participant identity cannot be replaced'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER ortak_private_dm_identity BEFORE UPDATE ON channels
FOR EACH ROW EXECUTE FUNCTION ortak_private_dm_identity();

-- Keep every existing authority field and add pair/expiry changes. Time-only
-- expiry is additionally carried in the Rust OfficeAuthority.valid_before
-- witness and checked by the existing deferred admission guards at commit.
DROP TRIGGER ortak_office_authority_channels ON channels;
CREATE TRIGGER ortak_office_authority_channels BEFORE INSERT OR UPDATE OR DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation(
    'community', 'community_id', 'id', 'channel_type', 'visibility',
    'archived_at', 'deleted_at', 'participant_hash', 'ttl_seconds', 'ttl_deadline');
