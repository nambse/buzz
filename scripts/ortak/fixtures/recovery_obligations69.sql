-- Synthetic G admission fixture, schema69 only. All production guards remain
-- enabled. No signature/provider/remote-memory acceptance is claimed here.
BEGIN;
DO $$
DECLARE company UUID; community UUID; channel UUID; project UUID; revision UUID;
    fact UUID; target UUID; deployment UUID; promotion UUID; publication UUID;
    human TEXT; employee_key TEXT; message BYTEA; memory JSONB; receipt JSONB;
    binding_hash BYTEA; approved TIMESTAMPTZ; expires TIMESTAMPTZ; scope_number INTEGER;
BEGIN
    FOR scope_number IN 1..2 LOOP
        company=gen_random_uuid(); community=gen_random_uuid(); channel=gen_random_uuid();
        project=gen_random_uuid(); revision=gen_random_uuid(); fact=gen_random_uuid();
        target=gen_random_uuid(); deployment=gen_random_uuid(); promotion=gen_random_uuid(); publication=gen_random_uuid();
        human=replace(gen_random_uuid()::text,'-','')||replace(gen_random_uuid()::text,'-','');
        employee_key=replace(gen_random_uuid()::text,'-','')||replace(gen_random_uuid()::text,'-','');
        message=sha256(convert_to('synthetic-source:'||company::text,'UTF8'));
        memory=jsonb_build_object('adapter','honcho','endpoint_ref','service://fixture/honcho',
            'workspace','synthetic-'||company::text,'user_peer','synthetic-human',
            'employee_peer','synthetic-employee','options','{}'::jsonb);
        INSERT INTO companies(id,slug,display_name) VALUES(company,'g-'||company::text,'G synthetic fixture');
        INSERT INTO communities(id,host) VALUES(community,'g-'||community::text||'.invalid');
        INSERT INTO office_company_bindings(company_id,community_id) VALUES(company,community);
        INSERT INTO channels(community_id,id,name,created_by,visibility)
            VALUES(community,channel,'G fixture',decode(human,'hex'),'private');
        INSERT INTO users(community_id,pubkey) VALUES(community,decode(human,'hex'));
        INSERT INTO channel_members(community_id,channel_id,pubkey,role)
            VALUES(community,channel,decode(human,'hex'),'member'),(community,channel,decode(employee_key,'hex'),'bot');
        INSERT INTO employees(company_id,id) VALUES(company,'cem');
        INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
            VALUES(company,revision,'cem',1,jsonb_build_object('memory',memory,'office',
                jsonb_build_object('public_key',employee_key,'signer_ref','credential://fixture/synthetic')),
                sha256(convert_to(company::text,'UTF8')),'adopt');
        INSERT INTO employee_memory_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,
            endpoint_ref,workspace,user_peer,employee_peer,options,validated_at)
            VALUES(company,revision,'cem','honcho','adopt',memory->>'endpoint_ref',memory->>'workspace',
                memory->>'user_peer',memory->>'employee_peer','{}',clock_timestamp());
        INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
            VALUES(company,'cem',revision,'adopt',decode(employee_key,'hex'),'credential://fixture/synthetic',clock_timestamp());
        UPDATE employees SET active_revision_id=revision,status='active' WHERE company_id=company AND id='cem';
        INSERT INTO projects(company_id,id,slug,name,created_by_type,created_by_id)
            VALUES(company,project,'fixture','G fixture','human',human);
        INSERT INTO project_api_bindings(company_id,project_id,community_id,channel_id,created_by)
            VALUES(company,project,community,channel,human);
        INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by)
            VALUES(company,project,human,'owner',human);
        INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
            VALUES(community,message,decode(human,'hex'),transaction_timestamp(),9,
                jsonb_build_array(jsonb_build_array('h',channel::text)),'Synthetic G source',decode(repeat('00',64),'hex'),channel);
        INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at)
            VALUES(company,message,transaction_timestamp(),9,decode(human,'hex'),channel,'decided',clock_timestamp());
        approved=clock_timestamp(); expires=approved+INTERVAL '1 day';
        INSERT INTO reviewed_memory_facts(company_id,community_id,id,project_id,employee_id,source_message_id,content,
            approved_by,approved_at,expires_at,promotion_operation_id)
            VALUES(company,community,fact,project,'cem',message,'Synthetic retained G fact',human,approved,expires,promotion);
        INSERT INTO reviewed_memory_operations(company_id,community_id,actor_pubkey,operation_id,action,request_hash,
            fact_id,project_id,result_version,auth_event_id)
            VALUES(company,community,human,promotion,'promote',sha256(convert_to('synthetic-promotion:'||fact::text,'UTF8')),
                fact,project,1,message);
        receipt=jsonb_build_object('company_id',company,'employee_id','cem','deployment_id',deployment,
            'binding',memory,'request_hash',encode(sha256(convert_to(deployment::text,'UTF8')),'hex'),
            'native_ids',jsonb_build_object('workspace','synthetic-'||company::text));
        binding_hash=sha256(convert_to(jsonb_build_object('request_hash',receipt->'request_hash','native_ids',receipt->'native_ids')::text,'UTF8'));
        INSERT INTO reviewed_memory_targets(company_id,community_id,id,project_id,employee_id,deployment_id,binding,
            creation_receipt,binding_hash,employee_revision_id,employee_lifecycle_epoch,enabled,valid_until)
            VALUES(company,community,target,project,'cem',deployment,memory,receipt,binding_hash,revision,0,true,clock_timestamp()+INTERVAL '55 seconds');
        INSERT INTO reviewed_memory_exports(company_id,community_id,fact_id,project_id,employee_id,target_id,
            employee_revision_id,employee_lifecycle_epoch,content_hash,source_hash,requested_by,operation_id)
            SELECT company,community,fact,project,'cem',target,revision,0,sha256(convert_to(f.content,'UTF8')),
                ortak_reviewed_export_source_hash(f),human,publication FROM reviewed_memory_facts f WHERE f.company_id=company AND f.id=fact;
        INSERT INTO reviewed_memory_export_commands(company_id,community_id,actor_pubkey,operation_id,fact_id,action,
            request_hash,result_version,auth_event_id)
            VALUES(company,community,human,publication,fact,'publish',sha256(convert_to('synthetic-command:'||fact::text,'UTF8')),0,message);
        INSERT INTO reviewed_memory_export_jobs(company_id,community_id,fact_id,action,idempotency_key,request_hash,next_attempt_at)
            VALUES(company,community,fact,'publish','reviewed:publish:'||fact::text,
                sha256(convert_to('synthetic-publish:'||fact::text,'UTF8')),clock_timestamp()),
                (company,community,fact,'withdraw','reviewed:withdraw:'||fact::text,
                sha256(convert_to('synthetic-withdraw:'||fact::text,'UTF8')),expires);
    END LOOP;
END $$;
COMMIT;

-- A real due claim transaction, preserving its canonical key/request identity.
BEGIN;
UPDATE reviewed_memory_export_jobs SET attempt_count=attempt_count+1,total_attempts=total_attempts+1,
    lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+INTERVAL '55 seconds',updated_at=clock_timestamp()
    WHERE action='publish' AND state='pending';
COMMIT;

-- Synthetic remote ACK, committed atomically with the exact claimed job. Both
-- reciprocal deferred receipt guards and ordinary community fences run.
BEGIN;
UPDATE reviewed_memory_export_jobs SET state='acknowledged',updated_at=clock_timestamp()
    WHERE action='publish' AND state='pending';
INSERT INTO reviewed_memory_export_receipts(company_id,community_id,fact_id,action,request_hash,binding_hash,
    content_hash,remote_status,erased_from_reviewed_store,tombstone_at,lease_token,total_attempts)
    SELECT j.company_id,j.community_id,j.fact_id,j.action,j.request_hash,t.binding_hash,x.content_hash,
        'active',false,NULL,j.lease_token,j.total_attempts
    FROM reviewed_memory_export_jobs j JOIN reviewed_memory_exports x USING(company_id,fact_id)
    JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id WHERE j.action='publish';
COMMIT;
