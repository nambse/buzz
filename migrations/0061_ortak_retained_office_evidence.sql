-- Company-owned Office evidence survives a fenced community purge.
-- Never rewrite migrations 0057/0059 or exempt evidence from universal fences.
-- Community deletion severs mutable Office authority, while company-owned
-- snapshots and reconciliation receipts retain their permanent provenance.

ALTER TABLE office_inbox_reconciliations
    DROP CONSTRAINT office_inbox_reconciliations_company_id_community_id_fkey,
    DROP CONSTRAINT office_inbox_reconciliations_community_id_channel_id_fkey,
    ADD CONSTRAINT office_inbox_reconciliations_community_id_fkey
        FOREIGN KEY (community_id) REFERENCES communities(id);
ALTER TABLE office_identity_profiles
    ADD CONSTRAINT office_identity_profiles_community_id_fkey
        FOREIGN KEY (community_id) REFERENCES communities(id);

-- Neither FK cascades: the community tombstone itself is permanent.
-- Existing company/employee/provisioning FKs and immutable byte guards remain.
SELECT attach_community_write_fence('office_identity_profiles');

CREATE FUNCTION ortak_guard_retained_office_authority() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    -- Lock the current binding, then use a fresh statement snapshot after any
    -- lock wait. The universal community fence has already locked lifecycle.
    PERFORM 1 FROM office_company_bindings b
    WHERE b.company_id=NEW.company_id AND b.community_id=NEW.community_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'retained Office evidence requires current binding'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM office_company_bindings b JOIN communities c ON c.id=b.community_id
        WHERE b.company_id=NEW.company_id AND b.community_id=NEW.community_id
          AND c.deletion_state='active' AND c.deleted_at IS NULL
    ) THEN
        RAISE EXCEPTION 'retained Office evidence requires active community authority'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END
$$;

-- Alphabetically after community_write_fence_*; no bypass via executor GUCs.
CREATE TRIGGER ortak_retained_office_authority BEFORE INSERT OR UPDATE
ON office_identity_profiles FOR EACH ROW EXECUTE FUNCTION ortak_guard_retained_office_authority();
CREATE TRIGGER ortak_retained_office_authority BEFORE INSERT OR UPDATE
ON office_inbox_reconciliations FOR EACH ROW EXECUTE FUNCTION ortak_guard_retained_office_authority();

-- Existing reconciliation current-capture/channel validation remains mandatory.
-- Mutable office_routing_channels/cohorts are purged before Office bindings;
-- cohort FK cascades remove only routing employee selections, never employees.
