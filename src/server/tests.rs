use super::*;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const PORT: u16 = 4747;

fn app() -> (tempfile::TempDir, Router, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        data_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    let state = AppState::new(cfg, PORT).unwrap();
    (dir, router(state.clone()), state)
}

fn req(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", format!("localhost:{PORT}"));
    if method != "GET" {
        b = b.header("origin", format!("http://localhost:{PORT}"));
    }
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn call(app: &Router, r: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(r).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn health_and_state() {
    let (_d, app, _) = app();
    let (s, v) = call(&app, req("GET", "/api/health", None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["app"], "parrot");
    let (_, v) = call(&app, req("GET", "/api/state", None)).await;
    assert_eq!(v["has_key"], false);
    assert_eq!(v["models"]["voice"], crate::gemini::VOICE_MODEL);
}

#[tokio::test]
async fn refuses_foreign_hosts_and_origins() {
    let (_d, app, _) = app();
    let r = Request::builder()
        .uri("/api/tree")
        .header("host", "evil.example:4747")
        .body(Body::empty())
        .unwrap();
    assert_eq!(call(&app, r).await.0, StatusCode::FORBIDDEN);

    let r = Request::builder()
        .method("POST")
        .uri("/api/topics")
        .header("host", format!("localhost:{PORT}"))
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"name":"x"}"#))
        .unwrap();
    assert_eq!(call(&app, r).await.0, StatusCode::FORBIDDEN);

    let r = Request::builder()
        .uri("/ws/agent")
        .header("host", format!("localhost:{PORT}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(call(&app, r).await.0, StatusCode::FORBIDDEN, "sockets need the app's origin");
}

#[tokio::test]
async fn notebook_crud() {
    let (_d, app, _) = app();
    let (s, v) = call(&app, req("POST", "/api/topics", Some(json!({ "name": "Sales" })))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["path"], "Sales");
    let (_, v) = call(&app, req("POST", "/api/topics", Some(json!({ "parent": "Sales", "name": "Pune" })))).await;
    assert_eq!(v["path"], "Sales/Pune");

    let (_, v) = call(
        &app,
        req(
            "POST",
            "/api/notes",
            Some(json!({ "parent": "Sales/Pune", "title": "Standup", "kind": "transcript" })),
        ),
    )
    .await;
    let note = v["path"].as_str().unwrap().to_string();
    assert_eq!(note, "Sales/Pune/Standup.md");

    let (_, doc) = call(&app, req("GET", "/api/note?path=Sales/Pune/Standup.md", None)).await;
    assert!(doc["content"].as_str().unwrap().contains("type: transcript"));

    let body = json!({ "path": note, "content": "hello", "expected_modified": doc["modified"] });
    let (s, saved) = call(&app, req("PUT", "/api/note", Some(body))).await;
    assert_eq!(s, StatusCode::OK);
    let stale = json!({ "path": note, "content": "again", "expected_modified": 1 });
    assert_eq!(call(&app, req("PUT", "/api/note", Some(stale))).await.0, StatusCode::CONFLICT);
    assert_eq!(saved["content"], "hello");

    let (_, v) = call(
        &app,
        req("POST", "/api/rename", Some(json!({ "path": "Sales/Pune", "name": "Pune office" }))),
    )
    .await;
    assert_eq!(v["path"], "Sales/Pune office");
    let (_, v) = call(
        &app,
        req("POST", "/api/move", Some(json!({ "path": "Sales/Pune office", "to": "" }))),
    )
    .await;
    assert_eq!(v["path"], "Pune office");

    let (_, trashed) = call(&app, req("POST", "/api/delete", Some(json!({ "path": "Pune office" })))).await;
    let (s, _) = call(&app, req("GET", "/api/note?path=Pune%20office/Standup.md", None)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, v) = call(&app, req("POST", "/api/restore", Some(json!({ "id": trashed["id"] })))).await;
    assert_eq!(v["path"], "Pune office");

    let (_, v) = call(&app, req("GET", "/api/tree", None)).await;
    let names: Vec<&str> = v["tree"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Pune office", "Sales"]);
}

#[tokio::test]
async fn refuses_escaping_paths() {
    let (_d, app, _) = app();
    let (s, v) = call(&app, req("GET", "/api/note?path=../../etc/passwd", None)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = call(&app, req("POST", "/api/notes", Some(json!({ "parent": "..", "title": "x" })))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn serves_the_app_shell() {
    let (_d, app, _) = app();
    let resp = app.clone().oneshot(req("GET", "/some/client/route", None)).await.unwrap();
    // index.html when the UI is built in, a clear message otherwise; never an API error.
    assert!(resp.status() == StatusCode::OK || resp.status() == StatusCode::NOT_FOUND);
    let (s, _) = call(&app, req("GET", "/api/nope", None)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[test]
fn host_and_origin_rules() {
    assert!(allowed_host("localhost:4747", 4747));
    assert!(allowed_host("127.0.0.1:4747", 4747));
    assert!(!allowed_host("localhost:4748", 4747));
    assert!(!allowed_host("parrot.localhost.evil.com:4747", 4747));
    assert!(allowed_origin("http://localhost:4747", 4747));
    assert!(!allowed_origin("http://localhost:4747.evil.com", 4747));
    assert!(!allowed_origin("null", 4747));
}

#[tokio::test]
async fn versions_api() {
    let (_d, app, _) = app();
    let (_, v) = call(
        &app,
        req("POST", "/api/notes", Some(json!({ "title": "Plan", "content": "first" }))),
    )
    .await;
    let path = v["path"].as_str().unwrap().to_string();
    let (_, doc) = call(&app, req("GET", "/api/note?path=Plan.md", None)).await;
    call(
        &app,
        req(
            "PUT",
            "/api/note",
            Some(json!({ "path": path, "content": "second", "expected_modified": doc["modified"] })),
        ),
    )
    .await;

    let (s, v) = call(&app, req("GET", "/api/versions?path=Plan.md", None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let versions = v["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["author"], "you");
    let first = versions[1]["id"].as_str().unwrap().to_string();

    let (_, one) = call(&app, req("GET", &format!("/api/version?path=Plan.md&id={first}"), None)).await;
    assert_eq!(one["content"], "first");
    let (s, restored) = call(
        &app,
        req("POST", "/api/versions/restore", Some(json!({ "path": "Plan.md", "id": first }))),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(restored["content"], "first");
    let (_, v) = call(&app, req("GET", "/api/versions?path=Plan.md", None)).await;
    assert_eq!(v["versions"][0]["author"], "restore");
    let (s, _) = call(&app, req("GET", "/api/version?path=Plan.md&id=../../x", None)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
