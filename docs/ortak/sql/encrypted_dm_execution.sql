-- Unnumbered and unactivated. Requires encrypted_dm_jobs + admission fragments.
-- All payload bytes remain in the protected store. No ordinary run event,
-- snapshot, output, memory write or Office HTTP publication is introduced.

-- Preserve the immutable63 ordinary completion behavior and its existing OID.
-- Confidential completion is materialized only in the protected reply tables.
CREATE OR REPLACE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.payload_mode='confidential_dm_v1' THEN RETURN NEW; END IF;
    IF NEW.work_item_id IS NULL AND NEW.routing_decision_id IS NOT NULL
       AND NEW.status='completed' AND NEW.delivery_intent IN('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES(NEW.company_id,NEW.id)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TABLE confidential_execution_leases (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 state TEXT NOT NULL DEFAULT 'observing' CHECK(state IN('observing','sealing','cancelling','complete','stopped','unconfirmed')),
 generation BIGINT NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 128),
 failures INTEGER NOT NULL DEFAULT 0 CHECK(failures BETWEEN 0 AND 3),
 cancel_attempts INTEGER NOT NULL DEFAULT 0 CHECK(cancel_attempts BETWEEN 0 AND 3),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','protocol','deadline_exceeded','cancelled')),
 finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id),FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK(state IN('observing','sealing','cancelling') OR lease_token IS NULL),
 CHECK((state IN('complete','stopped','unconfirmed'))=(finished_at IS NOT NULL)),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_execution_leases');
CREATE INDEX confidential_execution_due ON confidential_execution_leases(company_id,next_attempt_at,run_id)
 WHERE state IN('observing','sealing','cancelling');

-- The authenticated time is copied beside the exact original envelope, then
-- checked against its inner time while opening under current authority.
CREATE TABLE confidential_event_receipts (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 512),
 purpose TEXT NOT NULL DEFAULT 'runtime_event' CHECK(purpose='runtime_event'),
 occurred_at TIMESTAMPTZ NOT NULL CHECK(isfinite(occurred_at)),
 PRIMARY KEY(company_id,run_id,ordinal),
 FOREIGN KEY(company_id,run_id,purpose,ordinal) REFERENCES confidential_run_payloads(company_id,run_id,purpose,ordinal)
);
SELECT attach_community_write_fence('confidential_event_receipts');

CREATE TABLE confidential_reply_bundles (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 rumor_id BYTEA NOT NULL CHECK(octet_length(rumor_id)=32),rumor_hash BYTEA NOT NULL CHECK(octet_length(rumor_hash)=32),
 recipient_id BYTEA NOT NULL CHECK(octet_length(recipient_id)=32),history_id BYTEA NOT NULL CHECK(octet_length(history_id)=32),
 recipient_bytes BYTEA NOT NULL CHECK(octet_length(recipient_bytes) BETWEEN 1 AND 65536),
 history_bytes BYTEA NOT NULL CHECK(octet_length(history_bytes) BETWEEN 1 AND 65536),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,run_id),UNIQUE(company_id,recipient_id),UNIQUE(company_id,history_id),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK(recipient_id<>history_id)
);
SELECT attach_community_write_fence('confidential_reply_bundles');
CREATE TABLE confidential_reply_outbox (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 copy INTEGER NOT NULL CHECK(copy IN(0,1)),
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','acked','failed','retired')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),generation BIGINT NOT NULL DEFAULT 0 CHECK(generation=attempts),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','deadline_exceeded')),
 acknowledged_at TIMESTAMPTZ,finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id,copy),FOREIGN KEY(company_id,run_id) REFERENCES confidential_reply_bundles(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),CHECK(state='pending' OR lease_token IS NULL),
 CHECK((state<>'pending')=(finished_at IS NOT NULL)),CHECK((state='acked')=(acknowledged_at IS NOT NULL)),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_reply_outbox');
CREATE INDEX confidential_reply_due ON confidential_reply_outbox(company_id,next_attempt_at,run_id,copy) WHERE state='pending';

CREATE FUNCTION ortak_confidential_execution_immutable() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN RAISE EXCEPTION 'Confidential execution history is retained' USING ERRCODE='check_violation'; END
$$;
CREATE TRIGGER confidential_event_immutable BEFORE UPDATE OR DELETE ON confidential_event_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_reply_immutable BEFORE UPDATE OR DELETE ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_execution_retain BEFORE DELETE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_outbox_retain BEFORE DELETE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_event_no_truncate BEFORE TRUNCATE ON confidential_event_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_reply_no_truncate BEFORE TRUNCATE ON confidential_reply_bundles FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_execution_no_truncate BEFORE TRUNCATE ON confidential_execution_leases FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_outbox_no_truncate BEFORE TRUNCATE ON confidential_reply_outbox FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE FUNCTION ortak_confidential_execution_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; fresh BOOLEAN:=false;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state NOT IN('observing','cancelling') OR NEW.generation<>0 OR NEW.failures<>0 OR NEW.cancel_attempts<>0 OR NEW.lease_token IS NOT NULL THEN
   RAISE EXCEPTION 'Invalid confidential supervision admission' USING ERRCODE='check_violation'; END IF;
  fresh:=NEW.state='observing';
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
    OR OLD.state IN('stopped','unconfirmed') OR (OLD.state='complete' AND NOT (NEW.state='cancelling'
        AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL
        AND EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='pending'))) THEN
   RAISE EXCEPTION 'Confidential supervision cannot revive' USING ERRCODE='check_violation'; END IF;
  IF NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>OLD.state OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+interval '30 seconds'
    OR NEW.failures<>OLD.failures OR NEW.cancel_attempts<>OLD.cancel_attempts+(CASE WHEN NEW.state='cancelling' THEN 1 ELSE 0 END) THEN
    RAISE EXCEPTION 'Confidential supervision lease refused' USING ERRCODE='check_violation'; END IF;
   fresh:=NEW.state IN('observing','sealing');
  ELSIF NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   IF NEW.cancel_attempts<>OLD.cancel_attempts OR (NEW.state=OLD.state AND NEW.state IN('observing','sealing')
      AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
       OR NEW.next_attempt_at<statement_timestamp()+interval '1 second'
       OR NEW.failures NOT IN(0,OLD.failures+1)))
    OR (OLD.state='cancelling' AND NEW.state NOT IN('cancelling','stopped','unconfirmed')) THEN
    RAISE EXCEPTION 'Confidential supervision settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential supervision generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND (NEW.generation>=124 OR clock_timestamp()>=c.execution_deadline OR NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id)) THEN
  RAISE EXCEPTION 'Confidential supervision authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_execution_guard BEFORE INSERT OR UPDATE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_guard();

CREATE FUNCTION ortak_confidential_execution_commit() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=TG_OP='INSERT';
BEGIN
 IF NOT EXISTS(SELECT 1 FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id) THEN
  RAISE EXCEPTION 'Confidential execution community mismatch' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='confidential_execution_leases' THEN
  fresh:=NEW.state IN('observing','sealing') AND (TG_OP='INSERT' OR NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL);
  IF NEW.state='stopped' AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='acknowledged') THEN
   RAISE EXCEPTION 'Confidential stopped state lacks containment acknowledgement' USING ERRCODE='check_violation'; END IF;
  IF NEW.state IN('complete','sealing') AND NOT EXISTS(SELECT 1 FROM public.runs r
     WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status='completed'
      AND ((r.delivery_intent='silent' AND NEW.state='complete'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=3)
       OR (r.delivery_intent='reply'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=4
          AND EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0)
          AND (NEW.state='sealing' OR EXISTS(SELECT 1 FROM public.confidential_reply_bundles WHERE company_id=NEW.company_id AND run_id=NEW.run_id))))) THEN
   RAISE EXCEPTION 'Confidential terminal projection is incomplete' USING ERRCODE='check_violation'; END IF;
 ELSIF TG_TABLE_NAME='confidential_reply_outbox' AND TG_OP='UPDATE' THEN
  fresh:=NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL;
 END IF;
 IF fresh AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential execution authority expired at commit' USING ERRCODE='serialization_failure'; END IF;
 IF TG_TABLE_NAME='confidential_reply_bundles' THEN
  IF (SELECT count(*) FROM public.confidential_reply_outbox WHERE company_id=NEW.company_id AND run_id=NEW.run_id)<>2
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0) THEN
   RAISE EXCEPTION 'Confidential reply freeze is incomplete' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF TG_TABLE_NAME='confidential_run_payloads' THEN
  IF NEW.purpose='runtime_event' AND NOT EXISTS(SELECT 1 FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND ordinal=NEW.ordinal) THEN
   RAISE EXCEPTION 'Confidential event time receipt absent' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_execution_at_commit AFTER INSERT OR UPDATE ON confidential_execution_leases DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_event_at_commit AFTER INSERT ON confidential_event_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_reply_at_commit AFTER INSERT ON confidential_reply_bundles DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_outbox_at_commit AFTER INSERT OR UPDATE ON confidential_reply_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_event_payload_at_commit AFTER INSERT ON confidential_run_payloads DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE FUNCTION ortak_confidential_reply_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE identity JSONB; wire JSONB; bytes BYTEA; target TEXT; expected BYTEA; n INTEGER;
BEGIN
 IF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status='completed' AND delivery_intent='reply') THEN
  RAISE EXCEPTION 'Confidential reply has no current completion' USING ERRCODE='check_violation'; END IF;
 SELECT convert_from(identity_bytes,'UTF8')::jsonb INTO STRICT identity FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 FOR n IN 0..1 LOOP
  bytes:=CASE n WHEN 0 THEN NEW.recipient_bytes ELSE NEW.history_bytes END;
  expected:=CASE n WHEN 0 THEN NEW.recipient_id ELSE NEW.history_id END;
  target:=identity->>(CASE n WHEN 0 THEN 'human_public_key' ELSE 'employee_public_key' END);
  wire:=convert_from(bytes,'UTF8')::jsonb;
  IF jsonb_typeof(wire)<>'object' OR NOT wire ?& ARRAY['id','pubkey','created_at','kind','tags','content','sig']
   OR wire-ARRAY['id','pubkey','created_at','kind','tags','content','sig']<>'{}'::jsonb
   OR wire->>'id' IS DISTINCT FROM encode(expected,'hex') OR wire->'kind' IS DISTINCT FROM '1059'::jsonb
   OR wire->'tags' IS DISTINCT FROM jsonb_build_array(jsonb_build_array('p',target))
   OR ((wire->>'pubkey')~'^[0-9a-f]{64}$') IS DISTINCT FROM true OR ((wire->>'sig')~'^[0-9a-f]{128}$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'created_at') IS DISTINCT FROM 'number'
   OR ((wire->>'created_at')~'^(0|[1-9][0-9]{0,11})$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'content') IS DISTINCT FROM 'string' OR octet_length(wire->>'content') NOT BETWEEN 132 AND 60000 THEN
   RAISE EXCEPTION 'Confidential reply copy mismatch' USING ERRCODE='check_violation'; END IF;
 END LOOP;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_reply_guard BEFORE INSERT ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_guard();

CREATE FUNCTION ortak_confidential_reply_lease_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 SELECT c.execution_deadline INTO STRICT deadline FROM public.confidential_runs c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id AND c.community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL THEN RAISE EXCEPTION 'Invalid confidential output admission' USING ERRCODE='check_violation'; END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id,NEW.copy) IS DISTINCT FROM(OLD.company_id,OLD.community_id,OLD.run_id,OLD.copy) OR OLD.state<>'pending' THEN
   RAISE EXCEPTION 'Confidential output identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>'pending' OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>least(deadline,clock_timestamp()+interval '30 seconds') THEN
    RAISE EXCEPTION 'Confidential output lease refused' USING ERRCODE='check_violation'; END IF;fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- A known ACK for the unchanged locked owner is receipt-only after expiry.
   -- Pending retry still needs a live lease and cannot gain new authority here.
   IF NEW.state='acked' AND OLD.lease_token IS NULL
    OR NEW.state='pending' AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp())
    OR NEW.state='pending' AND (NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+interval '5 seconds') THEN
    RAISE EXCEPTION 'Confidential output settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential output generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential output authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_reply_lease_guard BEFORE INSERT OR UPDATE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_lease_guard();
