-- Unnumbered encrypted-DM prerequisite. No activation/normalizer/run changes.
-- Root must assemble with confidential persistence, expiry/deletion inventory
-- and final admission before enabling any consumer. Depends on immutable 1..76.

CREATE TABLE encrypted_dm_selections (
    company_id UUID NOT NULL REFERENCES companies(id),
    selection_id UUID NOT NULL CHECK(selection_id<>'00000000-0000-0000-0000-000000000000'),
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    human_public_key BYTEA NOT NULL CHECK(octet_length(human_public_key)=32),
    employee_public_key BYTEA NOT NULL CHECK(octet_length(employee_public_key)=32),
    office_binding_id UUID NOT NULL,
    key_version BIGINT NOT NULL CHECK(key_version>=0),
    decrypt_ref TEXT NOT NULL CHECK(ortak_is_credential_ref(decrypt_ref)),
    purpose TEXT NOT NULL DEFAULT 'dm_decrypt' CHECK(purpose='dm_decrypt'),
    enabled BOOLEAN NOT NULL DEFAULT false,
    generation BIGINT NOT NULL DEFAULT 1 CHECK(generation>0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    enabled_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,selection_id),
    CHECK(human_public_key<>employee_public_key),
    CHECK(NOT enabled OR enabled_at IS NOT NULL)
);
-- Outer1059 identifies a recipient, not its human or conversation. This initial
-- explicit one-human mode refuses ambiguity instead of guessing. Retained rows
-- are not overwritten, and this partial index does not preclude future multipair.
CREATE UNIQUE INDEX encrypted_dm_one_enabled_pair
 ON encrypted_dm_selections(company_id,employee_id) WHERE enabled;
SELECT attach_community_write_fence('encrypted_dm_selections');

CREATE FUNCTION ortak_encrypted_dm_pair_current(s encrypted_dm_selections)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(
  SELECT 1 FROM public.office_company_bindings cb
  JOIN public.companies co ON co.id=cb.company_id AND co.status='active'
  JOIN public.communities cm ON cm.id=cb.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
  JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=s.channel_id
  JOIN public.employees e ON e.company_id=co.id AND e.id=s.employee_id AND e.status='active'
  JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
  JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id AND b.id=s.office_binding_id
  WHERE cb.company_id=s.company_id AND cb.community_id=s.community_id
    AND ch.channel_type='dm' AND ch.visibility='private'
    AND ch.archived_at IS NULL AND ch.deleted_at IS NULL AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
    AND b.public_key=s.employee_public_key AND b.signer_ref=s.decrypt_ref
    AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
    AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
    AND r.manifest#>>'{office,public_key}'=encode(s.employee_public_key,'hex')
    AND r.manifest#>>'{office,signer_ref}'=s.decrypt_ref
    AND ch.participant_hash=public.digest(
        least(s.human_public_key,s.employee_public_key)||greatest(s.human_public_key,s.employee_public_key),'sha256')
    AND (SELECT count(*) FROM (SELECT 1 FROM public.channel_members m
        WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id LIMIT 3) members)=2
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.human_public_key AND m.removed_at IS NULL)
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.employee_public_key AND m.removed_at IS NULL)
    AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings other WHERE other.company_id=s.company_id AND other.public_key=s.human_public_key)
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.human_public_key
        AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
    AND NOT EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.pubkey=s.human_public_key AND m.role='bot')
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.employee_public_key AND u.deactivated_at IS NOT NULL)
 )
$$;

CREATE FUNCTION ortak_encrypted_dm_selection_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION 'Encrypted DM selection is retained' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF (to_jsonb(NEW)-ARRAY['enabled','generation','changed_at','enabled_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['enabled','generation','changed_at','enabled_at'])
     OR NEW.generation<>OLD.generation OR NEW.changed_at<>OLD.changed_at
     OR NEW.enabled_at IS DISTINCT FROM OLD.enabled_at THEN
   RAISE EXCEPTION 'Encrypted DM selection identity is immutable' USING ERRCODE='check_violation';
  END IF;
  IF NEW.enabled=OLD.enabled THEN RETURN OLD; END IF;
 END IF;
 -- Config changes are Office mutations. Try-lock fails rather than upgrading
 -- across another signed reader; no caller holds this fence through crypto.
 PERFORM public.ortak_advance_office_authority(NEW.company_id,'encrypted_dm_selections');
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 IF TG_OP='INSERT' THEN
  IF NEW.generation<>1 OR (SELECT count(*) FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id)>=128 THEN
   RAISE EXCEPTION 'Encrypted DM retained selection bound' USING ERRCODE='check_violation';
  END IF;
  NEW.created_at:=clock_timestamp();
 ELSE NEW.generation:=OLD.generation+1;
 END IF;
 IF (TG_OP='INSERT' OR NEW.enabled) AND NOT public.ortak_encrypted_dm_pair_current(NEW) THEN
  RAISE EXCEPTION 'Encrypted DM selected pair unavailable' USING ERRCODE='check_violation';
 END IF;
 NEW.changed_at:=clock_timestamp();
 IF NEW.enabled THEN NEW.enabled_at:=NEW.changed_at; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER encrypted_dm_selection_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_selections
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_guard();

CREATE FUNCTION ortak_encrypted_dm_selection_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_selections;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_selections
  WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id;
 IF current_row.enabled AND NOT public.ortak_encrypted_dm_pair_current(current_row) THEN
  RAISE EXCEPTION 'Encrypted DM selection expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER encrypted_dm_selection_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_selections
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_commit_guard();

-- Reconstruct signed outer bytes from server columns only. Explicit partition,
-- pending untouched inbox, one p, strict cipher/tag bounds; no plaintext input.
CREATE FUNCTION ortak_encrypted_dm_outer(target UUID, community UUID, source BYTEA, at_time TIMESTAMPTZ, recipient BYTEA)
RETURNS BYTEA LANGUAGE plpgsql VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE ev RECORD; canonical TEXT;
BEGIN
 SELECT e.id,e.pubkey,e.created_at,e.kind,e.tags,e.content,e.sig INTO ev
 FROM public.office_inbox i JOIN public.events e
  ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
 WHERE i.company_id=target AND i.event_id=source AND i.event_created_at=at_time
  AND i.event_kind=1059 AND e.kind=1059 AND e.channel_id IS NULL AND i.channel_id IS NULL
  AND i.author_pubkey=e.pubkey AND e.deleted_at IS NULL
  AND i.state='pending' AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
  AND e.created_at>=timestamptz '1970-01-01 00:00:00+00' AND e.created_at<timestamptz '10000-01-01 00:00:00+00'
  AND date_trunc('second',e.created_at)=e.created_at
  AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
  AND octet_length(e.content) BETWEEN 132 AND 60000 AND e.content~'^[A-Za-z0-9+/]*={0,2}$'
  AND octet_length(e.tags::text)<=256 AND e.tags=jsonb_build_array(jsonb_build_array('p',encode(recipient,'hex')));
 IF NOT FOUND THEN RETURN NULL; END IF;
 canonical:=public.ortak_conversation_json75(jsonb_build_object(
  'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
  'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex')));
 IF canonical IS NULL OR octet_length(canonical)>65536 THEN RETURN NULL; END IF;
 RETURN convert_to(canonical,'UTF8');
END
$$;

CREATE TABLE encrypted_dm_decrypt_jobs (
 company_id UUID NOT NULL REFERENCES companies(id), community_id UUID NOT NULL,
 source_id BYTEA NOT NULL CHECK(octet_length(source_id)=32), source_created_at TIMESTAMPTZ NOT NULL,
 source_author BYTEA NOT NULL CHECK(octet_length(source_author)=32), source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
 source_received_at TIMESTAMPTZ NOT NULL, selection_id UUID NOT NULL, selection_generation BIGINT NOT NULL CHECK(selection_generation>0),
 employee_id TEXT NOT NULL, employee_revision_id UUID NOT NULL, employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
 office_generation BIGINT NOT NULL CHECK(office_generation>=0), valid_before TIMESTAMPTZ NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), deadline TIMESTAMPTZ NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','claimed','verified','failed','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 claim_generation BIGINT NOT NULL DEFAULT 0 CHECK(claim_generation BETWEEN 0 AND 3),
 claim_token UUID, worker_id UUID, claimed_at TIMESTAMPTZ, claim_expires_at TIMESTAMPTZ, crypto_deadline TIMESTAMPTZ,
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), terminal_at TIMESTAMPTZ,
 error_code TEXT CHECK(error_code IN('material_unavailable','crypto_invalid','authority_changed','source_unavailable','deadline_exceeded','attempts_exhausted','cancelled')),
 seal_id BYTEA CHECK(octet_length(seal_id)=32), seal_created_at TIMESTAMPTZ,
 rumor_id BYTEA CHECK(octet_length(rumor_id)=32), rumor_created_at TIMESTAMPTZ,
 rumor_hash BYTEA CHECK(octet_length(rumor_hash)=32), reply_to BYTEA CHECK(octet_length(reply_to)=32), verified_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,source_id),
 FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
 CHECK(isfinite(deadline) AND isfinite(valid_before) AND deadline>source_received_at AND deadline<=source_received_at+interval '120 seconds' AND valid_before<=deadline),
 CHECK(isfinite(next_attempt_at) AND next_attempt_at<=deadline+interval '5 seconds'),
 CHECK(claim_generation=attempts),
 CHECK((state IN('claimed','verified'))=(claim_token IS NOT NULL)),
 CHECK((claim_token IS NULL)=(worker_id IS NULL) AND (claim_token IS NULL)=(claimed_at IS NULL)
  AND (claim_token IS NULL)=(claim_expires_at IS NULL) AND (claim_token IS NULL)=(crypto_deadline IS NULL)),
 CHECK(claim_token IS NULL OR (claim_token<>'00000000-0000-0000-0000-000000000000' AND worker_id<>'00000000-0000-0000-0000-000000000000')),
 CHECK(claim_token IS NULL OR (claimed_at<crypto_deadline AND crypto_deadline<=claimed_at+interval '5 seconds'
  AND crypto_deadline<=claim_expires_at AND claim_expires_at<=claimed_at+interval '30 seconds' AND claim_expires_at<=valid_before)),
 CHECK((state IN('failed','cancelled'))=(terminal_at IS NOT NULL)),
 CHECK((verified_at IS NULL)=(rumor_id IS NULL) AND (verified_at IS NULL)=(seal_id IS NULL)
  AND (verified_at IS NULL)=(seal_created_at IS NULL) AND (verified_at IS NULL)=(rumor_created_at IS NULL)
  AND (verified_at IS NULL)=(rumor_hash IS NULL)),
 CHECK(verified_at IS NOT NULL OR reply_to IS NULL),
 CHECK(seal_created_at IS NULL OR (seal_created_at>=timestamptz '1970-01-01 00:00:00+00'
  AND seal_created_at<timestamptz '10000-01-01 00:00:00+00' AND date_trunc('second',seal_created_at)=seal_created_at)),
 CHECK(rumor_created_at IS NULL OR (rumor_created_at>=timestamptz '1970-01-01 00:00:00+00'
  AND rumor_created_at<timestamptz '10000-01-01 00:00:00+00' AND date_trunc('second',rumor_created_at)=rumor_created_at)),
 CHECK(state<>'verified' OR verified_at IS NOT NULL)
);
CREATE INDEX encrypted_dm_jobs_due ON encrypted_dm_decrypt_jobs(company_id,next_attempt_at,source_received_at,source_id)
 WHERE state IN('pending','claimed','verified');
CREATE INDEX encrypted_dm_jobs_live ON encrypted_dm_decrypt_jobs(company_id,claim_expires_at)
 WHERE state IN('claimed','verified');
CREATE INDEX encrypted_dm_verified_rumor ON encrypted_dm_decrypt_jobs(company_id,employee_id,rumor_id)
 WHERE verified_at IS NOT NULL;
SELECT attach_community_write_fence('encrypted_dm_decrypt_jobs');

-- Replaced only by the later protected-admission fragment. This prerequisite
-- remains usable without that table; no job is consumed by verification alone.
CREATE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT false
$$;

CREATE FUNCTION ortak_encrypted_dm_job_current(j encrypted_dm_decrypt_jobs)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.encrypted_dm_selections s
  JOIN public.employees e ON e.company_id=s.company_id AND e.id=s.employee_id
  JOIN public.office_routing_cohorts c ON c.company_id=s.company_id AND c.community_id=s.community_id AND c.state='enabled'
  JOIN public.office_routing_channels ch ON ch.company_id=c.company_id AND ch.community_id=c.community_id AND ch.channel_id=s.channel_id
  JOIN public.office_routing_employees ce ON ce.company_id=c.company_id AND ce.employee_id=e.id
  JOIN public.office_inbox i ON i.company_id=j.company_id AND i.event_id=j.source_id
  WHERE s.company_id=j.company_id AND s.selection_id=j.selection_id AND s.community_id=j.community_id
   AND s.enabled AND s.generation=j.selection_generation AND s.employee_id=j.employee_id
   AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
   AND i.received_at=j.source_received_at AND i.received_at>=s.enabled_at AND i.author_pubkey=j.source_author
   AND clock_timestamp()<j.valid_before
   AND coalesce((SELECT generation FROM public.office_authority_generations g WHERE g.company_id=j.company_id),0)=j.office_generation
   AND public.ortak_encrypted_dm_pair_current(s)
   AND public.digest(public.ortak_encrypted_dm_outer(j.company_id,j.community_id,j.source_id,j.source_created_at,s.employee_public_key),'sha256')=j.source_hash)
$$;

CREATE FUNCTION ortak_encrypted_dm_job_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Encrypted DM job is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.verified_at IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Encrypted DM job initial state' USING ERRCODE='check_violation';
  END IF;
  fresh:=true;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) THEN
   RAISE EXCEPTION 'Encrypted DM job source is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.state IN('failed','cancelled') THEN
   IF NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION 'Encrypted DM terminal job retained' USING ERRCODE='check_violation'; END IF;
   RETURN OLD;
  END IF;
  IF OLD.verified_at IS NOT NULL AND
   (NEW.seal_id,NEW.seal_created_at,NEW.rumor_id,NEW.rumor_created_at,NEW.rumor_hash,NEW.reply_to,NEW.verified_at) IS DISTINCT FROM
   (OLD.seal_id,OLD.seal_created_at,OLD.rumor_id,OLD.rumor_created_at,OLD.rumor_hash,OLD.reply_to,OLD.verified_at) THEN
   RAISE EXCEPTION 'Encrypted DM verified metadata is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.verified_at IS NULL AND NEW.verified_at IS NOT NULL
    AND NOT(OLD.state='claimed' AND NEW.state='verified') THEN
   RAISE EXCEPTION 'Encrypted DM metadata requires current verification' USING ERRCODE='check_violation';
  END IF;
  -- Identical in-budget receipt replay has no new effect and cannot renew a
  -- token or deadline. Deferred current checks still apply to the result row.
  IF OLD.state='verified' AND NEW IS NOT DISTINCT FROM OLD THEN RETURN OLD; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.claim_generation=OLD.claim_generation+1 AND NEW.state='claimed'
   AND (OLD.state='pending' OR OLD.claim_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp()) AND OLD.next_attempt_at<=clock_timestamp()
   AND NEW.claim_token IS NOT NULL AND NEW.claim_token IS DISTINCT FROM OLD.claim_token THEN fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.claim_generation=OLD.claim_generation THEN
   IF NEW.state='verified' AND OLD.state='claimed' AND OLD.crypto_deadline>clock_timestamp()
    AND (OLD.verified_at IS NOT NULL OR NEW.verified_at>=OLD.claimed_at) AND NEW.verified_at<=clock_timestamp()
    AND (NEW.claim_token,NEW.worker_id,NEW.claimed_at,NEW.claim_expires_at,NEW.crypto_deadline) IS NOT DISTINCT FROM
        (OLD.claim_token,OLD.worker_id,OLD.claimed_at,OLD.claim_expires_at,OLD.crypto_deadline) THEN fresh:=true;
   ELSIF NEW.state IN('failed','cancelled') AND NEW.error_code IS NOT NULL THEN NULL;
   ELSIF NEW.state='pending' AND OLD.state IN('claimed','verified') AND OLD.claim_expires_at>clock_timestamp()
    AND NEW.error_code='material_unavailable' AND OLD.attempts<3
    AND NEW.next_attempt_at>=statement_timestamp()+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END) THEN NULL;
   ELSE RAISE EXCEPTION 'Encrypted DM job transition refused' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Encrypted DM claim generation refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF fresh THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
  -- Inbox claim-state changes deliberately do not advance Office generation.
  -- Retain its row lock through commit as well as comparing canonical facts.
  PERFORM 1 FROM public.office_inbox WHERE company_id=NEW.company_id AND event_id=NEW.source_id FOR SHARE;
  IF NEW.state='claimed' THEN
   IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-claims:'||NEW.company_id::text,0))
     OR NEW.claimed_at>clock_timestamp() OR NEW.crypto_deadline<=clock_timestamp()
     OR (SELECT count(*) FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=NEW.company_id
          AND j.source_id<>NEW.source_id AND j.state IN('claimed','verified') AND NOT public.ortak_encrypted_dm_job_consumed(j.company_id,j.source_id) AND j.claim_expires_at>clock_timestamp())>=2 THEN
    RAISE EXCEPTION 'Encrypted DM finite claim slot unavailable' USING ERRCODE='serialization_failure';
   END IF;
  END IF;
  IF NOT public.ortak_encrypted_dm_job_current(NEW) THEN
   RAISE EXCEPTION 'Encrypted DM job authority changed' USING ERRCODE='serialization_failure';
  END IF;
  IF NEW.state='verified' AND NEW.reply_to IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM public.encrypted_dm_decrypt_jobs previous
    JOIN public.encrypted_dm_selections p ON p.company_id=previous.company_id AND p.selection_id=previous.selection_id
    JOIN public.encrypted_dm_selections s ON s.company_id=NEW.company_id AND s.selection_id=NEW.selection_id
    WHERE previous.company_id=NEW.company_id AND previous.employee_id=NEW.employee_id
      AND previous.rumor_id=NEW.reply_to AND previous.verified_at IS NOT NULL AND previous.source_id<>NEW.source_id
      AND (p.community_id,p.channel_id,p.human_public_key,p.employee_public_key)=(s.community_id,s.channel_id,s.human_public_key,s.employee_public_key)) THEN
   RAISE EXCEPTION 'Encrypted DM reply lacks same-pair verified provenance' USING ERRCODE='check_violation';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER encrypted_dm_job_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_guard();

CREATE FUNCTION ortak_encrypted_dm_job_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_decrypt_jobs;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 IF current_row.state IN('pending','claimed','verified') AND
  (TG_OP='INSERT' OR NEW.state='verified' OR NEW.attempts>OLD.attempts) THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  IF NOT public.ortak_encrypted_dm_job_current(current_row)
   OR (current_row.state IN('claimed','verified') AND clock_timestamp()>=current_row.crypto_deadline) THEN
   RAISE EXCEPTION 'Encrypted DM job expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER encrypted_dm_job_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_decrypt_jobs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_commit_guard();

-- Both retained ledgers reject bulk erasure as well as row deletion.
CREATE TRIGGER encrypted_dm_selections_no_truncate BEFORE TRUNCATE ON encrypted_dm_selections
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER encrypted_dm_decrypt_jobs_no_truncate BEFORE TRUNCATE ON encrypted_dm_decrypt_jobs
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
