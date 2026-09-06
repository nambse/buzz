//! Valid JSON scratch survives the actual decoder, deferred DB guard and start.
use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with migration72"]
async fn snapshot_json_comparison_is_injective_for_nul_soh_and_literal_escapes() {
    let f = Fixture::new().await;
    let cases = [
        "\0",
        "\u{1}",
        "\u{1}\u{2}",
        "\0\u{1}",
        "\u{1}\0",
        "\\u0000",
        "\\u0001",
        "\\\0",
        "\\\\\0",
        "\\\\u0000",
        "a🦀é\0z",
        "a🦀é\u{1}z",
    ];
    let mut encoded = Vec::new();
    for content in cases {
        let body = json!({"content":content,"nested":{"same":content}}).to_string();
        let canonical: Value = sqlx::query_scalar("SELECT ortak_snapshot_scratch_jsonb($1::json)")
            .bind(body)
            .fetch_one(&f.pool)
            .await
            .expect("valid JSON including escaped NUL is representable for comparison");
        assert!(
            !encoded.contains(&canonical),
            "distinct JSON strings must not alias"
        );
        encoded.push(canonical);
    }
    let equivalent: bool = sqlx::query_scalar(
        r#"SELECT ortak_snapshot_scratch_jsonb('{"a":"\u0061\u0000","b":1}'::json)
            = ortak_snapshot_scratch_jsonb('{"b":1.0,"a":"a\u0000"}'::json)"#,
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(
        equivalent,
        "JSON key order and Unicode spelling stay semantic"
    );
}

fn render_scratch(wire: &mut Value) {
    let mut context: Vec<Value> = wire["recall"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            json!(
                json!({"type":"run_scratch_memory","trust":"untrusted_data","record":record})
                    .to_string()
            )
        })
        .collect();
    context.extend(wire["reviewed"]["records"].as_array().unwrap().iter().map(|record|
        json!(json!({"type":"reviewed_project_memory","trust":"untrusted_data","record":record}).to_string())));
    wire["spec"]["context"]["memory_context"] = json!(context);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with migration72"]
async fn reviewed_runtime_json_scratch_keeps_exact_bytes_budget_and_reviewed_guards() {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (run, _) = queue(&x.f, &x.app, &item).await;
    let lease =
        x.f.control
            .claim_runtime_dispatches(
                &x.scope,
                "fake-runtime",
                "json-compat",
                Duration::from_secs(60),
                1,
            )
            .await
            .unwrap()
            .remove(0);
    let DispatchAuthorization::Authorized(authority) =
        x.f.control
            .authorize_dispatch(&x.scope, &lease)
            .await
            .unwrap()
    else {
        panic!("current Work authority")
    };
    let memory = NamedMemory(
        FakeMemoryAdapter::new().with_existing_binding(x.employee.memory.as_ref().unwrap()),
    );
    let initial = ReviewedRunMemory::new(&memory, x.f.control.clone(), x.scope.clone())
        .snapshot(&authority, run, &RedactionPolicy::new())
        .await
        .unwrap();
    let mut wire: Value = serde_json::from_slice(&initial.encode().unwrap()).unwrap();
    let reviewed_bytes = wire["reviewed"]["records"][0]["content"]
        .as_str()
        .unwrap()
        .len();
    // The public snapshot decoder accepts bounded valid scratch JSON. Normal
    // remote recall strips controls, but restored snapshots retain exact bytes.
    let prefix = "NUL\0 SOH\u{1} adjacent\u{1}\0\u{2} escaped\\u0000\\u0001 backslash\\\0 🦀é ";
    let records: Vec<Value> = (0..4).map(|i| {
        let limit = 4096 - if i == 3 { reviewed_bytes } else { 0 };
        let content = format!("{prefix}{}", "a".repeat(limit-prefix.len()));
        json!({"record_ref":format!("scratch-{i}"),"content":content,
            "scope":{"scope":"run_scratch","run_id":run},
            "provenance":{"employee_id":"cem","run_id":run,"source":"run_scratch","recorded_at":Utc::now()}})
    }).collect();
    wire["recall"] = json!({"records":records,"truncated":false});
    render_scratch(&mut wire);
    let bytes = serde_json::to_vec(&wire).unwrap();
    let candidate =
        FrozenRunSnapshot::decode(&bytes, &authority, run).expect("valid v3 scratch JSON");
    assert_eq!(candidate.encode().unwrap(), bytes);

    for mutation in [
        "scratch_nul_alias",
        "scratch_literal_alias",
        "reviewed_content",
        "reviewed_pin",
        "byte_budget",
        "missing_use",
        "legacy_reviewed",
    ] {
        let mut forged = wire.clone();
        match mutation {
            "scratch_nul_alias" | "scratch_literal_alias" => {
                let mut rendered: Value = serde_json::from_str(
                    forged["spec"]["context"]["memory_context"][0]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
                let replacement = if mutation == "scratch_nul_alias" {
                    "\u{1}"
                } else {
                    "\\u0000"
                };
                rendered["record"]["content"] = json!(rendered["record"]["content"]
                    .as_str()
                    .unwrap()
                    .replacen('\0', replacement, 1));
                forged["spec"]["context"]["memory_context"][0] = json!(rendered.to_string());
            }
            "reviewed_content" => {
                forged["reviewed"]["records"][0]["content"] = json!("Forged deployment fact");
                render_scratch(&mut forged);
            }
            "reviewed_pin" => {
                forged["reviewed"]["records"][0]["pin"]["approved_by"] = json!("ff".repeat(32));
                render_scratch(&mut forged);
            }
            "byte_budget" => {
                forged["recall"]["records"][3]["content"] = json!(format!(
                    "{}a",
                    forged["recall"]["records"][3]["content"].as_str().unwrap()
                ));
                render_scratch(&mut forged);
            }
            "legacy_reviewed" => forged["version"] = json!(2),
            _ => {}
        }
        let bytes = serde_json::to_vec(&forged).unwrap();
        let mut tx = x.f.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO run_context_snapshots(company_id,run_id,spec_bytes,spec_hash) VALUES($1,$2,$3,$4)")
            .bind(x.f.company).bind(run).bind(&bytes).bind(Sha256::digest(&bytes).to_vec()).execute(&mut *tx).await.unwrap();
        if mutation != "missing_use" {
            sqlx::query("INSERT INTO run_reviewed_memory_uses(company_id,community_id,run_id,ordinal,fact_id,target_id,fact_version,
                consumption_epoch,content_hash,source_hash,binding_hash,approval_id,approved_by,expires_at)
                SELECT f.company_id,f.community_id,$2,0,f.id,t.id,f.version,t.consumption_epoch,x.content_hash,x.source_hash,t.binding_hash,
                f.promotion_operation_id,f.approved_by,f.expires_at FROM reviewed_memory_facts f
                JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
                JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
                WHERE f.company_id=$1 AND f.id=$3")
                .bind(x.f.company).bind(run).bind(x.fact).execute(&mut *tx).await.unwrap();
        }
        let err = tx
            .commit()
            .await
            .expect_err("forged snapshot must fail its deferred guard");
        assert_eq!(
            err.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514"),
            "{mutation}: {err}"
        );
    }
    let result =
        x.f.control
            .freeze_run_snapshot(&x.scope, &lease, &authority, run, &candidate)
            .await
            .unwrap();
    assert!(
        matches!(result, FreezeSnapshotOutcome::Ready(_)),
        "{result:?}"
    );
    let stored: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(
        stored, bytes,
        "comparison encoding never changes frozen bytes"
    );
    let adapter = FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
    let result = RunSupervisor::new(x.f.control.clone(), &adapter, SupervisorConfig::default())
        .with_run_memory(NoSecondRecall)
        .dispatch(&x.scope, &lease)
        .await
        .unwrap();
    assert!(
        matches!(result, DispatchOutcome::Started { .. }),
        "{result:?}"
    );
    assert_eq!(adapter.start_specs(), vec![candidate.spec().clone()]);
}
