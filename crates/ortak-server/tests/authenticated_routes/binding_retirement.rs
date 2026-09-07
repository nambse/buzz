use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn missing_current_company_binding_is_not_found_before_any_product_read() {
    let f = Fixture::new().await;
    let app = product_router(
        f.control.clone(),
        config(Uuid::new_v4(), &f.operator, f.channel),
        Arc::new(Replay::default()),
    )
    .unwrap();
    for path in ["/api/v1/runs", "/api/v1/employees", "/api/v1/projects"] {
        let result = response(&app, signed(&f.operator, "GET", path, "", false)).await;
        assert_eq!(
            result.0,
            StatusCode::NOT_FOUND,
            "missing binding for {path}"
        );
        assert_eq!(result.1, json!({"error":{"code":"not_found"}}));
    }
}
