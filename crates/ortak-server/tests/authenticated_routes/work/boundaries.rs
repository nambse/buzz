use super::*;

async fn source_message(f: &Fixture, channel: Uuid) -> String {
    let event = EventBuilder::new(Kind::Custom(9), "Canonical source fixture")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(&f.operator)
        .unwrap();
    let created = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
    sqlx::query(
        "INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
        VALUES($1,$2,$3,$4,9,$5,'Canonical source fixture',$6,$7)",
    )
    .bind(f.community)
    .bind(event.id.to_bytes().as_slice())
    .bind(f.operator.public_key().to_bytes().as_slice())
    .bind(created)
    .bind(serde_json::to_value(&event.tags).unwrap())
    .bind(event.sig.serialize().as_slice())
    .bind(channel)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at)
        VALUES($1,$2,$3,9,$4,$5,'decided',now())")
        .bind(f.company).bind(event.id.to_bytes().as_slice()).bind(created)
        .bind(f.operator.public_key().to_bytes().as_slice()).bind(channel)
        .execute(&f.pool).await.unwrap();
    event.id.to_hex()
}

async fn trusted_item(f: &Fixture, project: Uuid, title: &str, source: Option<String>) -> Uuid {
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    WorkService::new(f.control.clone())
        .create_work_item(
            &scope,
            NewWorkItem {
                project_id: project,
                title: title.to_owned(),
                description: String::new(),
                priority: WorkPriority::Normal,
                criteria: vec![],
                approvals: vec![],
                source_message_id: source,
            },
            WorkActor::Human(f.operator.public_key().to_hex()),
        )
        .await
        .unwrap()
        .item
        .item
        .id
}

fn assert_hidden(status: StatusCode, body: Value) {
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body, json!({"error":{"code":"not_found"}}));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn canonical_source_channel_and_project_grant_revocation_hide_work() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let wide = work_app(&f, true, Role::Reader, vec![f.channel, f.hidden]);
    let visible = project(&f, &app, f.channel).await;
    let hidden = project(&f, &wide, f.hidden).await;
    let hidden_item = item(&f, &wide, hidden).await;
    let (status, body) = get(&app, &f.operator, &format!("/api/v1/projects/{hidden}")).await;
    assert_hidden(status, body);
    let (status, body) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&hidden_item)),
    )
    .await;
    assert_hidden(status, body);
    let (_, projects) = get(&app, &f.operator, "/api/v1/projects").await;
    assert_eq!(projects["projects"].as_array().unwrap().len(), 1);
    assert_eq!(projects["projects"][0]["id"], visible.to_string());

    let source = source_message(&f, f.channel).await;
    let mut promotion = item_body("Promoted manual task");
    promotion["source_message_id"] = json!(source);
    let path = format!("/api/v1/projects/{visible}/promotions");
    let (status, promoted) = post(&app, &f.operator, &path, &promotion).await;
    assert_eq!(status, StatusCode::CREATED, "{promoted}");
    let promoted_id = id(&promoted["work_item"]);
    let (status, replay) = post(&app, &f.operator, &path, &promotion).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["work_item"], promoted["work_item"]);
    let hidden_source = source_message(&f, f.hidden).await;
    // A trusted preexisting item may reference a source outside the HTTP audience.
    // Source filtering must happen before LIMIT rather than hiding the first page.
    let unexposed = trusted_item(
        &f,
        visible,
        "Hidden canonical source",
        Some(hidden_source.clone()),
    )
    .await;
    let (status, body) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{unexposed}"),
    )
    .await;
    assert_hidden(status, body);
    let list = format!("/api/v1/projects/{visible}/work-items?limit=1");
    let (status, page) = get(&app, &f.operator, &list).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["work_items"].as_array().unwrap().len(), 1);
    assert_eq!(page["work_items"][0]["id"], promoted_id.to_string());
    assert!(!page.to_string().contains(&hidden_source));

    sqlx::query("UPDATE events SET deleted_at=now() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, body) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{promoted_id}"),
    )
    .await;
    assert_hidden(status, body);
    assert_eq!(
        get(&app, &f.operator, &list).await.1["work_items"],
        json!([])
    );
    // A saved operation receipt never grants a bypass around a deleted source.
    let (status, body) = post(&app, &f.operator, &path, &promotion).await;
    assert_hidden(status, body);

    let ordinary = item(&f, &app, visible).await;
    grant(&f, visible, &f.reader, "viewer").await;
    assert_eq!(
        get(&app, &f.reader, &format!("/api/v1/projects/{visible}"))
            .await
            .0,
        StatusCode::OK
    );
    sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company).bind(visible).bind(f.reader.public_key().to_hex()).execute(&f.pool).await.unwrap();
    let (status, body) = get(
        &app,
        &f.reader,
        &format!("/api/v1/work-items/{}", id(&ordinary)),
    )
    .await;
    assert_hidden(status, body);
    sqlx::query("UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice())
        .execute(&f.pool).await.unwrap();
    for path in [
        format!("/api/v1/projects/{visible}"),
        format!("/api/v1/work-items/{}", id(&ordinary)),
    ] {
        let (status, body) = get(&app, &f.operator, &path).await;
        assert_hidden(status, body);
    }
    assert_eq!(
        get(&app, &f.operator, "/api/v1/projects").await.1["projects"],
        json!([])
    );
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn manual_projection_hides_trusted_run_attachments_and_raw_history() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let current = item(&f, &app, project).await;
    let hidden_run = f.run(f.hidden).await;
    let before = runtime_counts(&f).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let attached = WorkService::new(f.control.clone())
        .attach_record(
            &scope,
            AttachRecord {
                work_item_id: id(&current),
                expected_version: version(&current),
                reference: AttachmentRef::Run { run_id: hidden_run },
                label: Some("private-attachment-label".to_owned()),
                actor: WorkActor::Human(f.operator.public_key().to_hex()),
            },
        )
        .await
        .unwrap();
    assert_eq!(attached.item.attachments.len(), 1);
    let (status, detail) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&current)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["work_item"]["history_omitted"], true);
    assert_eq!(detail["work_item"]["execution_available"], false);
    assert_eq!(detail["work_item"]["history"].as_array().unwrap().len(), 1);
    let encoded = detail.to_string();
    for hidden in [
        hidden_run.to_string(),
        "private-attachment-label".to_owned(),
    ] {
        assert!(
            !encoded.contains(&hidden),
            "manual projection leaked an attachment field"
        );
    }
    for field in [
        "attachments",
        "dependencies",
        "source_routing_decision_id",
        "run_id",
        "routing_decision_id",
    ] {
        assert!(detail["work_item"].get(field).is_none());
    }
    assert!(detail["work_item"]["history"][0].get("payload").is_none());
    assert_eq!(runtime_counts(&f).await, before);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn strict_bodies_and_project_scoped_keyset_pages_remain_bounded() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let other = super::project(&f, &app, f.channel).await;
    let path = format!("/api/v1/projects/{project}/work-items");
    let mut forged = item_body("Unknown actor field");
    forged["actor"] = json!({"type":"human","public_key":f.operator.public_key().to_hex()});
    assert_eq!(
        post(&app, &f.operator, &path, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    let body =
        json!({"operation_id":Uuid::new_v4(),"title":"Too large","description":"x".repeat(16384)})
            .to_string();
    assert!(body.len() > 16384);
    assert_eq!(
        response(&app, signed(&f.operator, "POST", &path, &body, true))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM work_items WHERE company_id=$1")
        .bind(f.company)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "rejected bodies must not create Work resources");
    let mut expected = HashSet::new();
    for index in 0..26 {
        expected.insert(
            trusted_item(&f, project, &format!("Page item {index}"), None)
                .await
                .to_string(),
        );
    }
    let (status, first) = get(&app, &f.operator, &format!("{path}?limit=999")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let entries = first["work_items"].as_array().unwrap();
    assert_eq!(entries.len(), 25);
    let cursor = first["next_cursor"].as_str().unwrap();
    assert!(cursor.starts_with(&format!("{project}/")));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", cursor)
        .finish();
    let (status, second) = get(&app, &f.operator, &format!("{path}?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["work_items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());
    let actual: HashSet<_> = entries
        .iter()
        .chain(second["work_items"].as_array().unwrap())
        .map(|entry| entry["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(
        get(
            &app,
            &f.operator,
            &format!("/api/v1/projects/{other}/work-items?{query}")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
}
