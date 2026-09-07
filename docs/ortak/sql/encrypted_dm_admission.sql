-- Unnumbered, unactivated protected admission. Assemble AFTER encrypted_dm_jobs.
-- No ordinary 1059 normalizer or dispatch consumer is enabled by this fragment.
-- Shared Office fence precedes selection, verified job, rumor, inbox and run.
-- The conservative v1 epoch is the carried Office generation: any relevant
-- Office mutation retires old authority permanently, including remove/restore.
-- No renewal or remote cleanup policy is introduced here.

ALTER TABLE runs ADD COLUMN payload_mode TEXT NOT NULL DEFAULT 'ordinary'
 CHECK(payload_mode IN('ordinary','confidential_dm_v1'));

CREATE FUNCTION ortak_confidential_runtime_binding(company UUID,revision UUID)
RETURNS JSONB LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 FROM public.employee_runtime_bindings b JOIN public.employee_revisions r
 ON r.company_id=b.company_id AND r.id=b.revision_id AND r.employee_id=b.employee_id
 WHERE b.company_id=company AND b.revision_id=revision AND b.validated_at IS NOT NULL
 AND r.manifest->'runtime'=jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 AND r.manifest->'permissions'='{"allowed_tools":[],"allowed_workspaces":[],"allowed_networks":[],"approval_required":[]}'::jsonb
 AND r.manifest#>'{routing,enabled}'='true'::jsonb
$$;

CREATE FUNCTION ortak_confidential_dm_run_id(company UUID,source BYTEA)
RETURNS UUID LANGUAGE SQL IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT substr(encode(public.digest(convert_to('ortak-confidential-run-id/1:'||company::text||':'||encode(source,'hex'),'UTF8'),'sha256'),'hex'),1,32)::uuid
 WHERE octet_length(source)=32
$$;

CREATE FUNCTION ortak_confidential_dm_source(company UUID,source BYTEA)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'format','ortak-confidential-dm-source/1','company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,
  'employee_public_key',encode(s.employee_public_key,'hex'),'human_public_key',encode(s.human_public_key,'hex'),
  'office_binding_id',s.office_binding_id,'key_version',s.key_version::text,
  'outer_event_id',encode(j.source_id,'hex'),'outer_event_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'outer_json_sha256',encode(j.source_hash,'hex'),'seal_event_id',encode(j.seal_id,'hex'),
  'seal_event_created_at',to_char(j.seal_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_event_id',encode(j.rumor_id,'hex'),'rumor_event_created_at',to_char(j.rumor_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_json_sha256',encode(j.rumor_hash,'hex'),'reply_rumor_id',encode(j.reply_to,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
$$;

CREATE FUNCTION ortak_confidential_dm_identity(company UUID,source BYTEA,run UUID,key UUID)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'authority_epoch',j.office_generation::text,'company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,'employee_lifecycle_epoch',j.employee_lifecycle_epoch::text,
  'employee_public_key',encode(s.employee_public_key,'hex'),'employee_revision_id',j.employee_revision_id,
  'human_public_key',encode(s.human_public_key,'hex'),'key_id',key,'key_version',s.key_version::text,
  'office_binding_id',s.office_binding_id,'rumor_id',encode(j.rumor_id,'hex'),'run_id',run,
  'source_evidence_hash',encode(public.digest(public.ortak_confidential_dm_source(company,source),'sha256'),'hex'),
  'source_outer_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'source_outer_id',encode(j.source_id,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
 AND run=public.ortak_confidential_dm_run_id(company,source)
 AND key<>'00000000-0000-0000-0000-000000000000'
$$;

CREATE TABLE confidential_runs (
 company_id UUID NOT NULL REFERENCES companies(id),community_id UUID NOT NULL,run_id UUID NOT NULL,
 source_id BYTEA NOT NULL CHECK(octet_length(source_id)=32),selection_id UUID NOT NULL,
 employee_id TEXT NOT NULL,human_public_key BYTEA NOT NULL CHECK(octet_length(human_public_key)=32),
 rumor_id BYTEA NOT NULL CHECK(octet_length(rumor_id)=32),key_id UUID NOT NULL,
 identity_bytes BYTEA NOT NULL CHECK(octet_length(identity_bytes) BETWEEN 1 AND 2048),
 source_bytes BYTEA NOT NULL CHECK(octet_length(source_bytes) BETWEEN 1 AND 4096),
 wrapped_key BYTEA NOT NULL CHECK(octet_length(wrapped_key) BETWEEN 1 AND 12288),
 start_key TEXT NOT NULL CHECK(start_key='ortak-run:'||company_id::text||':'||run_id::text),
 admitted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 admission_deadline TIMESTAMPTZ NOT NULL,execution_deadline TIMESTAMPTZ NOT NULL,
 claim_generation BIGINT NOT NULL CHECK(claim_generation BETWEEN 1 AND 3),claim_token UUID NOT NULL,claim_worker UUID NOT NULL,
 PRIMARY KEY(company_id,run_id), UNIQUE(company_id,source_id), UNIQUE(company_id,key_id),
 -- Independent of wrapper, Office key version, pair re-enable and model revision.
 UNIQUE(company_id,employee_id,human_public_key,rumor_id),
 FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
 FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
 FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
 CHECK(isfinite(admitted_at) AND isfinite(admission_deadline) AND isfinite(execution_deadline)
  AND admission_deadline>admitted_at AND execution_deadline>admitted_at AND execution_deadline<=admitted_at+interval '10 minutes')
);
SELECT attach_community_write_fence('confidential_runs');

CREATE TABLE confidential_run_payloads (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 purpose TEXT NOT NULL CHECK(purpose IN('snapshot','runtime_event','reply_draft')),
 ordinal INTEGER NOT NULL,
 envelope_bytes BYTEA NOT NULL CHECK(octet_length(envelope_bytes) BETWEEN 1 AND 98304),
 nonce BYTEA NOT NULL CHECK(octet_length(nonce)=12),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,run_id,purpose,ordinal),UNIQUE(company_id,run_id,purpose,nonce),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((purpose IN('snapshot','reply_draft') AND ordinal=0) OR (purpose='runtime_event' AND ordinal BETWEEN 1 AND 512))
);
SELECT attach_community_write_fence('confidential_run_payloads');

-- Each verified outer is consumed once, including alternative wrappers. This
-- immutable row is the terminal job receipt; the job's original lease remains
-- historical evidence and can never be reclaimed after consumption.
CREATE TABLE confidential_dm_receipts (
 company_id UUID NOT NULL,community_id UUID NOT NULL,source_id BYTEA NOT NULL,
 run_id UUID NOT NULL,duplicate_rumor BOOLEAN NOT NULL,
 claim_generation BIGINT NOT NULL,claim_token UUID NOT NULL,claim_worker UUID NOT NULL,
 committed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,source_id),
 FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id)
);
SELECT attach_community_write_fence('confidential_dm_receipts');

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=company AND source_id=source)
$$;

-- Separate outbox: ordinary runtime startup cannot claim this row by accident.
-- The later confidential worker owns these finite leases. No consumer is wired.
CREATE TABLE confidential_run_dispatches (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','delivered','failed','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 generation BIGINT NOT NULL DEFAULT 0 CHECK(generation=attempts),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','deadline_exceeded','cancelled')),
 finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id),FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK((state<>'pending')=(finished_at IS NOT NULL)),CHECK(state='pending' OR lease_token IS NULL),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_run_dispatches');

-- Canonical stored outer after finalization. The real ephemeral author, NULL
-- channel, exact partition and signed ciphertext remain unchanged in both rows.
CREATE FUNCTION ortak_confidential_dm_current(company UUID,run UUID)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_runs c
 JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
 JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
 JOIN public.encrypted_dm_selections s ON s.company_id=j.company_id AND s.selection_id=j.selection_id
 JOIN public.employees e ON e.company_id=j.company_id AND e.id=j.employee_id
 JOIN public.office_routing_cohorts co ON co.company_id=c.company_id AND co.community_id=c.community_id AND co.state='enabled'
 JOIN public.office_routing_channels ch ON ch.company_id=co.company_id AND ch.community_id=co.community_id AND ch.channel_id=s.channel_id
 JOIN public.office_routing_employees ce ON ce.company_id=co.company_id AND ce.employee_id=e.id
 JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=j.source_id
 JOIN public.events ev ON ev.community_id=c.community_id AND ev.id=j.source_id AND ev.created_at=j.source_created_at
 WHERE c.company_id=company AND c.run_id=run AND r.payload_mode='confidential_dm_v1'
 AND r.status IN('queued','running','waiting','completed') AND r.work_item_id IS NULL
 AND r.employee_id=j.employee_id AND r.employee_revision_id=j.employee_revision_id AND r.employee_lifecycle_epoch=j.employee_lifecycle_epoch
 AND r.message_id=j.source_id AND r.root_message_id=j.source_id
 AND s.enabled AND s.generation=j.selection_generation AND s.community_id=c.community_id
 AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
 AND public.ortak_encrypted_dm_pair_current(s) AND public.ortak_confidential_runtime_binding(company,j.employee_revision_id) IS NOT NULL
 AND coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=company),0)=j.office_generation
 AND clock_timestamp()<c.execution_deadline
 AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
 AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author AND i.received_at=j.source_received_at
 AND ev.kind=1059 AND ev.channel_id IS NULL AND ev.pubkey=j.source_author AND ev.deleted_at IS NULL
 AND ev.tags=jsonb_build_array(jsonb_build_array('p',encode(s.employee_public_key,'hex')))
 AND octet_length(ev.content) BETWEEN 132 AND 60000 AND octet_length(ev.tags::text)<=256
 AND public.digest(convert_to(public.ortak_conversation_json75(jsonb_build_object(
 'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
 'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex'))),'UTF8'),'sha256')=j.source_hash
 AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations stop WHERE stop.company_id=company AND stop.run_id=run))
$$;

CREATE FUNCTION ortak_lock_confidential_dm(company UUID,run UUID) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE target RECORD;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id,source_id INTO target FROM public.confidential_runs WHERE company_id=company AND run_id=run;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=target.selection_id FOR SHARE;
 PERFORM 1 FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=target.source_id FOR SHARE;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=target.source_id FOR SHARE;
 RETURN public.ortak_confidential_dm_current(company,run);
END
$$;

CREATE FUNCTION ortak_confidential_payload_valid(bytes BYTEA,identity BYTEA,purpose TEXT,ordinal INTEGER)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE wire JSONB; header JSONB; size INTEGER; nonce BYTEA; cipher BYTEA; maximum INTEGER;
BEGIN
 IF octet_length(bytes)>98304 OR octet_length(identity)>2048 THEN RETURN false; END IF;
 wire:=convert_from(bytes,'UTF8')::jsonb;
 IF jsonb_typeof(wire) IS DISTINCT FROM 'object' OR NOT wire ?& ARRAY['ciphertext','header','nonce'] OR wire-ARRAY['ciphertext','header','nonce']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wire),'UTF8')<>bytes THEN RETURN false; END IF;
 header:=wire->'header';
 IF jsonb_typeof(header) IS DISTINCT FROM 'object' OR NOT header ?& ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose'] OR header-ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose']<>'{}'::jsonb
  OR header->>'algorithm' IS DISTINCT FROM 'A256GCM' OR header->>'format' IS DISTINCT FROM 'ortak-confidential-payload/1'
  OR header->>'purpose' IS DISTINCT FROM purpose OR header->'ordinal' IS DISTINCT FROM to_jsonb(ordinal)
  OR convert_to(public.ortak_conversation_json75(header->'identity'),'UTF8') IS DISTINCT FROM identity
  OR jsonb_typeof(header->'plaintext_bytes')<>'number' THEN RETURN false; END IF;
 maximum:=CASE purpose WHEN 'snapshot' THEN 49152 WHEN 'runtime_event' THEN 32768 WHEN 'reply_draft' THEN 16384 END;
 IF maximum IS NULL OR (purpose='runtime_event' AND ordinal NOT BETWEEN 1 AND 512)
  OR (purpose<>'runtime_event' AND ordinal<>0) THEN RETURN false; END IF;
 IF (header->>'plaintext_bytes')!~'^(0|[1-9][0-9]{0,5})$' THEN RETURN false; END IF;
 size:=(header->>'plaintext_bytes')::integer;
 IF size>maximum OR jsonb_typeof(wire->'nonce')<>'string' OR length(wire->>'nonce')<>16
  OR jsonb_typeof(wire->'ciphertext')<>'string' OR length(wire->>'ciphertext')>65560 THEN RETURN false; END IF;
 nonce:=decode(wire->>'nonce','base64'); cipher:=decode(wire->>'ciphertext','base64');
 RETURN octet_length(nonce)=12 AND octet_length(cipher)=size+16
  AND replace(encode(nonce,'base64'),E'\n','')=wire->>'nonce'
  AND replace(encode(cipher,'base64'),E'\n','')=wire->>'ciphertext';
END
$$;

CREATE FUNCTION ortak_confidential_run_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; r public.runs; wrapped JSONB; cipher BYTEA;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential run bytes are immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT r FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR (j.community_id,j.selection_id,j.employee_id,j.rumor_id,s.human_public_key) IS DISTINCT FROM
     (NEW.community_id,NEW.selection_id,NEW.employee_id,NEW.rumor_id,NEW.human_public_key)
  OR r.payload_mode<>'confidential_dm_v1' OR r.status<>'queued' OR r.runtime_run_ref IS NOT NULL
  OR (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.message_id,r.root_message_id) IS DISTINCT FROM
     (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.source_id,j.source_id)
  OR r.work_item_id IS NOT NULL OR public.ortak_confidential_runtime_binding(NEW.company_id,j.employee_revision_id) IS NULL
  OR NEW.run_id<>public.ortak_confidential_dm_run_id(NEW.company_id,NEW.source_id)
  OR NEW.identity_bytes IS DISTINCT FROM public.ortak_confidential_dm_identity(NEW.company_id,NEW.source_id,NEW.run_id,NEW.key_id)
  OR NEW.source_bytes IS DISTINCT FROM public.ortak_confidential_dm_source(NEW.company_id,NEW.source_id)
  OR NEW.admission_deadline IS DISTINCT FROM j.claim_expires_at THEN
  RAISE EXCEPTION 'Confidential run requires exact current verified claim' USING ERRCODE='check_violation';
 END IF;
 NEW.admitted_at:=clock_timestamp();
 IF NEW.execution_deadline>NEW.admitted_at+interval '10 minutes' THEN
  RAISE EXCEPTION 'Confidential execution deadline exceeds bound' USING ERRCODE='check_violation';
 END IF;
 wrapped:=convert_from(NEW.wrapped_key,'UTF8')::jsonb;
 IF jsonb_typeof(wrapped) IS DISTINCT FROM 'object' OR NOT wrapped ?& ARRAY['ciphertext','format','identity','purpose','signer_ref'] OR wrapped-ARRAY['ciphertext','format','identity','purpose','signer_ref']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wrapped),'UTF8')<>NEW.wrapped_key
  OR wrapped->>'format' IS DISTINCT FROM 'ortak-confidential-key-envelope/1'
  OR wrapped->>'purpose' IS DISTINCT FROM 'confidential_master'
  OR convert_to(wrapped->>'identity','UTF8') IS DISTINCT FROM NEW.identity_bytes
  OR wrapped->>'signer_ref' IS DISTINCT FROM s.decrypt_ref
  OR jsonb_typeof(wrapped->'ciphertext') IS DISTINCT FROM 'string'
  OR length(wrapped->>'ciphertext') NOT BETWEEN 132 AND 8192 THEN
  RAISE EXCEPTION 'Confidential wrapped key identity differs' USING ERRCODE='check_violation';
 END IF;
 cipher:=decode(wrapped->>'ciphertext','base64');
 IF octet_length(cipher)<99 OR get_byte(cipher,0)<>2 OR replace(encode(cipher,'base64'),E'\n','')<>wrapped->>'ciphertext' THEN
  RAISE EXCEPTION 'Confidential wrapped key encoding differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_run_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_guard();

CREATE FUNCTION ortak_confidential_payload_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; initial BOOLEAN; prior INTEGER;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential ciphertext is immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 initial:=NEW.purpose='snapshot' AND NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id);
 IF initial THEN
  IF NOT EXISTS(SELECT 1 FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=c.company_id AND j.source_id=c.source_id
   AND j.state='verified' AND j.claim_token=c.claim_token AND j.claim_expires_at>clock_timestamp() AND public.ortak_encrypted_dm_job_current(j)) THEN
   RAISE EXCEPTION 'Confidential initial snapshot claim expired' USING ERRCODE='check_violation';
  END IF;
 ELSIF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential payload authority retired' USING ERRCODE='check_violation';
 END IF;
 -- Serialize event ordinals without any plaintext parser or per-run unbounded scan.
 PERFORM 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
 IF NEW.community_id<>c.community_id OR NOT public.ortak_confidential_payload_valid(NEW.envelope_bytes,c.identity_bytes,NEW.purpose,NEW.ordinal)
  OR NEW.nonce IS DISTINCT FROM decode(convert_from(NEW.envelope_bytes,'UTF8')::jsonb->>'nonce','base64') THEN
  RAISE EXCEPTION 'Confidential payload wire differs' USING ERRCODE='check_violation';
 END IF;
 IF NEW.purpose='runtime_event' THEN
  IF NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('queued','running','waiting')) THEN
   RAISE EXCEPTION 'Confidential runtime event follows terminal run' USING ERRCODE='check_violation';
  END IF;
  SELECT coalesce(max(ordinal),0) INTO prior FROM public.confidential_run_payloads
   WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='runtime_event';
  IF NEW.ordinal<>prior+1 THEN RAISE EXCEPTION 'Confidential event sequence gap' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_payload_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_payloads
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_payload_guard();

CREATE FUNCTION ortak_confidential_receipt_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; c public.confidential_runs; s public.encrypted_dm_selections;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential receipt is retained' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR j.community_id<>NEW.community_id OR c.employee_id<>j.employee_id OR c.human_public_key<>s.human_public_key OR c.rumor_id<>j.rumor_id
  OR NEW.duplicate_rumor IS DISTINCT FROM (NEW.source_id<>c.source_id) THEN
  RAISE EXCEPTION 'Confidential receipt needs exact verified rumor' USING ERRCODE='check_violation';
 END IF;
 NEW.committed_at:=clock_timestamp();
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_receipt_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_dm_receipts
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_receipt_guard();

CREATE FUNCTION ortak_confidential_consumed_job() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=OLD.company_id AND source_id=OLD.source_id)
  AND NEW IS DISTINCT FROM OLD THEN
  RAISE EXCEPTION 'Consumed decrypt job cannot be reclaimed' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_consumed_job BEFORE UPDATE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_consumed_job();

CREATE FUNCTION ortak_confidential_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; receipt public.confidential_dm_receipts;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF TG_TABLE_NAME='confidential_dm_receipts' THEN
  SELECT * INTO STRICT receipt FROM public.confidential_dm_receipts WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=j.company_id AND selection_id=j.selection_id;
  IF j.claim_expires_at<=clock_timestamp() OR j.valid_before<=clock_timestamp()
   OR coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=j.company_id),0)<>j.office_generation
   OR NOT s.enabled OR s.generation<>j.selection_generation OR NOT public.ortak_encrypted_dm_pair_current(s)
   OR NOT EXISTS(SELECT 1 FROM public.office_inbox i WHERE i.company_id=j.company_id AND i.event_id=j.source_id
       AND i.state=(CASE WHEN receipt.duplicate_rumor THEN 'dropped' ELSE 'decided' END) AND i.finalized_at IS NOT NULL) THEN
   RAISE EXCEPTION 'Confidential receipt authority expired before commit' USING ERRCODE='serialization_failure';
  END IF;
  IF receipt.duplicate_rumor THEN RETURN NEW; END IF;
 END IF;
 IF NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential current authority expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 IF TG_TABLE_NAME='confidential_runs' OR (TG_TABLE_NAME='confidential_run_payloads' AND to_jsonb(NEW)->>'purpose'='snapshot') THEN
  IF c.admission_deadline<=clock_timestamp()
   OR NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id AND run_id=c.run_id AND NOT duplicate_rumor)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=c.company_id AND run_id=c.run_id AND purpose='snapshot' AND ordinal=0)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_dispatches WHERE company_id=c.company_id AND run_id=c.run_id AND state='pending' AND attempts=0) THEN
   RAISE EXCEPTION 'Confidential admission is incomplete or expired' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_at_commit AFTER INSERT ON confidential_runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();
CREATE CONSTRAINT TRIGGER confidential_payload_at_commit AFTER INSERT ON confidential_run_payloads
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();
CREATE CONSTRAINT TRIGGER confidential_receipt_at_commit AFTER INSERT ON confidential_dm_receipts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();

CREATE FUNCTION ortak_confidential_run_mode_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.payload_mode IS DISTINCT FROM OLD.payload_mode THEN
  RAISE EXCEPTION 'Run payload mode is immutable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 IF NEW.work_item_id IS NOT NULL OR NEW.routing_decision_id IS NULL OR NEW.message_id IS NULL OR NEW.root_message_id<>NEW.message_id
  OR NEW.error_message IS NOT NULL OR (NEW.error_code IS NOT NULL AND NEW.error_code NOT IN('confidential_failed','confidential_cancelled'))
  OR (NEW.cancel_reason IS NOT NULL AND NEW.cancel_reason NOT IN('office_revoked','human_requested'))
  OR (NEW.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref!~'^[A-Za-z0-9][A-Za-z0-9:._/-]{0,255}$') THEN
  RAISE EXCEPTION 'Confidential run permits bounded metadata only' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND
  (to_jsonb(NEW)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) IS DISTINCT FROM
  (to_jsonb(OLD)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) THEN
  RAISE EXCEPTION 'Confidential run authority is immutable' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.status IN('completed','failed','cancelled') AND NEW.status<>OLD.status THEN
  RAISE EXCEPTION 'Confidential terminal status cannot revive' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref IS DISTINCT FROM OLD.runtime_run_ref THEN
  RAISE EXCEPTION 'Confidential start correlation cannot change' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential fresh execution authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_run_mode_guard BEFORE INSERT OR UPDATE ON runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_mode_guard();

-- SQL backstop for every retained ordinary content/effect sink. Existing guards
-- still run for ordinary rows and their legacy snapshot/NUL contracts are intact.
CREATE FUNCTION ortak_confidential_reject_ordinary() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.payload_mode='confidential_dm_v1') THEN
  RAISE EXCEPTION 'Confidential run cannot use an ordinary content path' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_no_ordinary_snapshot BEFORE INSERT OR UPDATE ON run_context_snapshots FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_events BEFORE INSERT OR UPDATE ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_office BEFORE INSERT OR UPDATE ON runtime_office_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_work BEFORE INSERT OR UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_memory BEFORE INSERT OR UPDATE ON runtime_memory_writes FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_reviewed_use BEFORE INSERT OR UPDATE ON run_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_use BEFORE INSERT OR UPDATE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_action BEFORE INSERT OR UPDATE ON workspace_tool_actions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_receipt BEFORE INSERT OR UPDATE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_reader BEFORE INSERT OR UPDATE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_work_execution BEFORE INSERT OR UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_artifact BEFORE INSERT OR UPDATE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_work_attachment BEFORE INSERT OR UPDATE ON work_attachments FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_outbox BEFORE INSERT OR UPDATE ON outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE FUNCTION ortak_confidential_dispatch_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Confidential dispatch is retained' USING ERRCODE='check_violation'; END IF;
 SELECT execution_deadline INTO STRICT deadline FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Confidential dispatch initial state' USING ERRCODE='check_violation';
  END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
   OR OLD.state<>'pending' THEN RAISE EXCEPTION 'Confidential dispatch identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.state='pending'
   AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
   AND OLD.next_attempt_at<=clock_timestamp() AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp())
   AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=least(deadline,clock_timestamp()+interval '30 seconds') THEN
   fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- Exact lease accounting remains possible after source/Office revocation.
   -- A delivered result requires a retained start reference; it grants no start.
   IF NEW.state='delivered' AND (OLD.lease_expires_at<=clock_timestamp() OR OLD.lease_token IS NULL
      OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND runtime_run_ref IS NOT NULL)) THEN
    RAISE EXCEPTION 'Confidential delivery needs retained start receipt' USING ERRCODE='check_violation';
   ELSIF NEW.state='pending' AND (NEW.error_code<>'unavailable' OR OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
     OR NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+(CASE WHEN NEW.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)) THEN
    RAISE EXCEPTION 'Confidential retry is not bounded lease accounting' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Confidential dispatch lease transition refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF NEW.next_attempt_at>deadline+interval '5 seconds' THEN RAISE EXCEPTION 'Confidential retry deadline exceeded' USING ERRCODE='check_violation'; END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential dispatch authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_dispatch_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_dispatches
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_guard();

CREATE FUNCTION ortak_confidential_dispatch_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
  IF NEW.lease_expires_at<=clock_timestamp() OR NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
   RAISE EXCEPTION 'Confidential dispatch expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_dispatch_at_commit AFTER UPDATE ON confidential_run_dispatches
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_commit_guard();

CREATE FUNCTION ortak_commit_confidential_dm(company UUID,source BYTEA,run UUID,key UUID,identity BYTEA,wrapped BYTEA,snapshot BYTEA,nonce BYTEA)
RETURNS TABLE(committed_run_id UUID,duplicate_rumor BOOLEAN)
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; old public.confidential_runs;
 decision UUID; input_hash BYTEA; policy_hash TEXT; binding JSONB;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source FOR UPDATE;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR identity IS DISTINCT FROM public.ortak_confidential_dm_identity(company,source,run,key) THEN
  RAISE EXCEPTION 'Confidential verified commit claim changed' USING ERRCODE='check_violation';
 END IF;
 -- Serialize absent rumor discovery before allocating any run/chain. The hash
 -- lock is collision-conservative; the full unique tuple is the authority.
 IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-confidential-rumor:'||company::text||':'||j.employee_id||':'||encode(s.human_public_key,'hex')||':'||encode(j.rumor_id,'hex'),0)) THEN
  RAISE EXCEPTION 'Confidential rumor commit busy' USING ERRCODE='serialization_failure';
 END IF;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=source FOR UPDATE;
 IF NOT public.ortak_encrypted_dm_job_current(j) THEN RAISE EXCEPTION 'Confidential source claimed elsewhere' USING ERRCODE='serialization_failure'; END IF;
 SELECT * INTO old FROM public.confidential_runs WHERE company_id=company AND employee_id=j.employee_id AND human_public_key=s.human_public_key AND rumor_id=j.rumor_id;
 IF FOUND THEN
  INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
   VALUES(company,j.community_id,source,old.run_id,true,j.claim_generation,j.claim_token,j.worker_id);
  UPDATE public.office_inbox SET state='dropped',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
  RETURN QUERY SELECT old.run_id,true; RETURN;
 END IF;
 binding:=public.ortak_confidential_runtime_binding(company,j.employee_revision_id);
 IF binding IS NULL THEN RAISE EXCEPTION 'Confidential selected policy is not empty' USING ERRCODE='check_violation'; END IF;
 decision:=run; input_hash:=public.digest(public.ortak_confidential_dm_source(company,source),'sha256');
 policy_hash:='sha256:'||encode(public.digest(convert_to('ortak-confidential-dm-direct/1','UTF8'),'sha256'),'hex');
 INSERT INTO public.delivery_chains(company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes,hop_count,wake_count)
  VALUES(company,source,'confidential_dm_v1',policy_hash,1,1,0,0);
 INSERT INTO public.routing_decisions(company_id,id,message_id,root_message_id,inbox_claim_generation,origin_type,origin_id,mode,summary_reason,
  policy_version,policy_fingerprint,input_hash,candidate_revision_ids,wake_count,hop_consumed,chain_hop_count,chain_wake_count,
  office_authority_generation,office_authority_valid_before,office_input_hash)
 VALUES(company,decision,source,source,0,'human',encode(s.human_public_key,'hex'),'deterministic','direct_message',
  'confidential_dm_v1',policy_hash,input_hash,jsonb_build_array(j.employee_revision_id),1,true,1,1,j.office_generation,j.claim_expires_at,j.source_hash);
 INSERT INTO public.routing_recipients(company_id,routing_decision_id,employee_id,position,action,reason,employee_revision_id,employee_lifecycle_epoch)
  VALUES(company,decision,j.employee_id,0,'wake','direct_message',j.employee_revision_id,j.employee_lifecycle_epoch);
 INSERT INTO public.delivery_chain_visits(company_id,root_message_id,employee_id,routing_decision_id,recipient_action,batch_hop)
  VALUES(company,source,j.employee_id,decision,'wake',1);
 UPDATE public.delivery_chains SET hop_count=1,wake_count=1,updated_at=clock_timestamp() WHERE company_id=company AND root_message_id=source;
 INSERT INTO public.runs(company_id,id,employee_id,employee_revision_id,routing_decision_id,message_id,root_message_id,runtime_adapter,
  payload_mode,employee_lifecycle_epoch,office_admission_generation,office_admission_valid_before,office_admission_token)
 VALUES(company,run,j.employee_id,j.employee_revision_id,decision,source,source,binding->>'adapter','confidential_dm_v1',j.employee_lifecycle_epoch,
  j.office_generation,j.claim_expires_at,j.claim_token);
 INSERT INTO public.confidential_runs(company_id,community_id,run_id,source_id,selection_id,employee_id,human_public_key,rumor_id,key_id,
  identity_bytes,source_bytes,wrapped_key,start_key,admission_deadline,execution_deadline,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,run,source,s.selection_id,j.employee_id,s.human_public_key,j.rumor_id,key,identity,
  public.ortak_confidential_dm_source(company,source),wrapped,'ortak-run:'||company::text||':'||run::text,j.claim_expires_at,clock_timestamp()+interval '10 minutes',j.claim_generation,j.claim_token,j.worker_id);
 INSERT INTO public.confidential_run_payloads(company_id,community_id,run_id,purpose,ordinal,envelope_bytes,nonce)
 VALUES(company,j.community_id,run,'snapshot',0,snapshot,nonce);
 INSERT INTO public.confidential_run_dispatches(company_id,community_id,run_id) VALUES(company,j.community_id,run);
 INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,source,run,false,j.claim_generation,j.claim_token,j.worker_id);
 UPDATE public.office_inbox SET state='decided',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
 RETURN QUERY SELECT run,false;
END
$$;

CREATE INDEX confidential_dispatch_due ON confidential_run_dispatches(company_id,next_attempt_at,run_id) WHERE state='pending';
CREATE INDEX confidential_runs_selection ON confidential_runs(company_id,selection_id,run_id);
CREATE TRIGGER confidential_runs_no_truncate BEFORE TRUNCATE ON confidential_runs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_payloads_no_truncate BEFORE TRUNCATE ON confidential_run_payloads FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_receipts_no_truncate BEFORE TRUNCATE ON confidential_dm_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_dispatches_no_truncate BEFORE TRUNCATE ON confidential_run_dispatches FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_confidential_run_complete_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs;
BEGIN
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 SELECT * INTO c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Confidential run has no protected admission' USING ERRCODE='check_violation'; END IF;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=c.company_id AND source_id=c.source_id;
 IF NOT EXISTS(SELECT 1 FROM public.routing_decisions d
  JOIN public.routing_recipients rr ON rr.company_id=d.company_id AND rr.routing_decision_id=d.id AND rr.employee_id=j.employee_id
  JOIN public.delivery_chain_visits v ON v.company_id=d.company_id AND v.root_message_id=d.root_message_id AND v.employee_id=rr.employee_id AND v.routing_decision_id=d.id
  JOIN public.delivery_chains ch ON ch.company_id=d.company_id AND ch.root_message_id=d.root_message_id
  WHERE d.company_id=c.company_id AND d.id=NEW.routing_decision_id AND d.message_id=j.source_id AND d.root_message_id=j.source_id
   AND d.id=NEW.id AND d.mode='deterministic' AND d.summary_reason='direct_message'
   AND d.policy_version='confidential_dm_v1' AND d.inbox_claim_generation=0 AND d.origin_type='human' AND d.origin_id=encode(c.human_public_key,'hex')
   AND d.input_hash=public.digest(c.source_bytes,'sha256') AND d.office_input_hash=j.source_hash
   AND d.wake_count=1 AND d.hop_consumed AND d.chain_hop_count=1 AND d.chain_wake_count=1
   AND rr.action='wake' AND rr.employee_revision_id=j.employee_revision_id AND rr.employee_lifecycle_epoch=j.employee_lifecycle_epoch
   AND v.batch_hop=1 AND ch.hop_count=1 AND ch.wake_count=1 AND ch.max_hops=1 AND ch.max_wakes=1)
  OR NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential admission routing provenance differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_complete_at_commit AFTER INSERT ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_complete_guard();

CREATE FUNCTION ortak_confidential_run_transition_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.payload_mode='confidential_dm_v1' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential execution expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_transition_at_commit AFTER UPDATE ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_transition_guard();

-- Preserve the migration53 trigger OID and ordinary claim semantics. A
-- confidential wake consumes its distinct verified decrypt lease, never a
-- fabricated ordinary inbox claim or a policy-name-only exception.
CREATE OR REPLACE FUNCTION ortak_check_routing_claim_expiry() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_claim RECORD;
BEGIN
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id
        AND r.id=NEW.id AND r.routing_decision_id=NEW.id AND r.payload_mode='confidential_dm_v1') THEN
        PERFORM public.ortak_lock_office_authority(NEW.company_id);
        IF NOT EXISTS(SELECT 1 FROM public.confidential_runs c
          JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
          JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
          JOIN public.confidential_dm_receipts receipt ON receipt.company_id=c.company_id AND receipt.source_id=c.source_id AND receipt.run_id=c.run_id
          JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=c.source_id
          WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id AND c.source_id=NEW.message_id
            AND NEW.root_message_id=c.source_id AND NEW.inbox_claim_generation=0
            AND NEW.policy_version='confidential_dm_v1' AND NEW.mode='deterministic'
            AND NEW.origin_type='human' AND NEW.origin_id=encode(c.human_public_key,'hex')
            AND NEW.wake_count=1 AND NEW.hop_consumed
            AND NEW.office_authority_generation=j.office_generation
            AND NEW.office_authority_valid_before=j.claim_expires_at AND NEW.office_input_hash=j.source_hash
            AND NEW.input_hash=public.digest(c.source_bytes,'sha256')
            AND j.state='verified' AND j.claim_expires_at>clock_timestamp() AND j.valid_before>clock_timestamp()
            AND c.admission_deadline=j.claim_expires_at AND c.admission_deadline>clock_timestamp()
            AND (c.claim_generation,c.claim_token,c.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND (receipt.claim_generation,receipt.claim_token,receipt.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND NOT receipt.duplicate_rumor
            AND (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.office_admission_token)=
                (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.claim_token)
            AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
            AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author
            AND public.ortak_confidential_dm_current(c.company_id,c.run_id)) THEN
            RAISE EXCEPTION 'ortak: confidential decrypt claim changed or expired before commit'
                USING ERRCODE='serialization_failure';
        END IF;
        RETURN NEW;
    END IF;
    -- Unchanged migration53 ordinary inbox-claim branch.
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
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
