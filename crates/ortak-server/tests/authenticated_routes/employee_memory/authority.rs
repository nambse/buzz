use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_capability_own_source_and_canonical_approval() {
    let x = MemoryFixture::new(true).await;
    let f = &x.f;
    let preview_request = json!({"source_event_id":x.source,"destination_channel_id":f.hidden,"kind":"experience","human_public_key":null});
    let operator_only = app(
        f,
        &f.reader,
        false,
        Role::Operator,
        vec![f.channel, f.hidden],
        vec!["cem"],
    );
    assert_eq!(
        post(
            &operator_only,
            &f.reader,
            &format!("{PATH}/preview"),
            &preview_request
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let preview = x.preview(&f.reader, "experience").await;
    let mut command = x.command(&preview);
    // Escaped JSON is >4KiB, while the actual edited UTF8 content is exactly4KiB.
    command["fact"]["content"] = json!("\"".repeat(4096));
    let (status, result) = post(&operator_only, &f.reader, PATH, &command).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{result}");
    let (status, result) = post(&x.app, &f.reader, PATH, &command).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["effect"]["result_version"], 1);
    assert_eq!(result["fact"]["content"], command["fact"]["content"]);
    assert_eq!(x.counts().await, (1, 1, 0));
    let id = Uuid::parse_str(result["fact"]["id"].as_str().unwrap()).unwrap();
    let (canonical,sql_submission,rust_submission):(Vec<u8>,Vec<u8>,Vec<u8>)=sqlx::query_as(
        "SELECT f.provenance_bytes,ortak_employee_memory_submission(f,o.operation_id,o.action),o.submitted_bytes
        FROM employee_reviewed_memory_facts f JOIN employee_reviewed_memory_operations o ON o.company_id=f.company_id AND o.fact_id=f.id
        WHERE f.company_id=$1 AND f.id=$2")
        .bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
    let provenance = EmployeeMemoryProvenanceV1::from_canonical_bytes(&canonical).unwrap();
    assert_eq!(
        provenance.approval().approved_by().to_hex(),
        f.reader.public_key().to_hex()
    );
    assert_eq!(
        provenance.source().author_public_key(),
        provenance.approval().approved_by()
    );
    assert_eq!(sql_submission, rust_submission);
    assert_eq!(
        provenance.source_hash().unwrap().to_hex(),
        preview["source_hash"]
    );
    let auth_event:Vec<u8>=sqlx::query_scalar("SELECT auth_event_id FROM employee_reviewed_memory_operations WHERE company_id=$1 AND fact_id=$2")
        .bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(auth_event.len(), 32);
    let relationship = x.preview(&f.reader, "relationship").await;
    let (status, result) = post(&x.app, &f.reader, PATH, &x.command(&relationship)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        result["fact"]["audience"]["human_public_key"],
        f.reader.public_key().to_hex()
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_refuses_forged_source_audience_and_scope() {
    let x = MemoryFixture::new(false).await;
    let f = &x.f;
    let preview = x.preview(&f.operator, "experience").await;
    let own = x.command(&preview);
    let foreign_source = source(f, &f.reader, f.channel).await;
    let cases = vec![
        (
            "source_event_id",
            json!(foreign_source),
            StatusCode::FORBIDDEN,
        ),
        (
            "source_event_created_at",
            json!("2000-01-01T00:00:00.000000Z"),
            StatusCode::FORBIDDEN,
        ),
        (
            "expected_audience_hash",
            json!("aa".repeat(32)),
            StatusCode::CONFLICT,
        ),
        ("reviewed", json!(false), StatusCode::BAD_REQUEST),
        (
            "content",
            json!("hidden\u{0000}text"),
            StatusCode::BAD_REQUEST,
        ),
        (
            "expires_at",
            json!("2000-01-01T00:00:00.000000Z"),
            StatusCode::BAD_REQUEST,
        ),
        (
            "source_hash",
            json!("aa".repeat(32)),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (field, value, expected) in cases {
        let mut command = own.clone();
        command["fact"][field] = value;
        let (status, body) = post(&x.app, &f.operator, PATH, &command).await;
        assert_eq!(status, expected, "{field}: {body}");
    }
    let narrow = app(
        f,
        &f.operator,
        true,
        Role::Reader,
        vec![f.channel],
        vec!["cem"],
    );
    assert_eq!(
        post(&narrow, &f.operator, PATH, &own).await.0,
        StatusCode::FORBIDDEN
    );
    let mut relationship = own.clone();
    relationship["fact"]["kind"] = json!("relationship");
    relationship["fact"]["human_public_key"] = json!(f.reader.public_key().to_hex());
    assert_eq!(
        post(&x.app, &f.operator, PATH, &relationship).await.0,
        StatusCode::FORBIDDEN
    );
    let mut missing = own.clone();
    missing["fact"]
        .as_object_mut()
        .unwrap()
        .remove("human_public_key");
    assert_eq!(
        post(&x.app, &f.operator, PATH, &missing).await.0,
        StatusCode::BAD_REQUEST
    );
    // Parent capability and employee access are independent from a fake body claim.
    let inaccessible = app(
        f,
        &f.operator,
        true,
        Role::Operator,
        vec![f.channel, f.hidden],
        vec!["other"],
    );
    assert_eq!(
        post(&inaccessible, &f.operator, PATH, &own).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(x.counts().await, (0, 0, 0));
}
