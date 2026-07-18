use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::herdr;
use crate::html;
use crate::inject::is_allowed_key;
use crate::state::{AppState, INJECT_DELAY_MILLIS, MAX_TEXT_LENGTH};

#[derive(Deserialize)]
pub struct TokenQuery {
    t: Option<String>,
}

#[derive(Deserialize)]
pub struct InputBody {
    text: String,
    #[serde(default)]
    enter: bool,
}

#[derive(Serialize)]
struct OkResponse {
    status: &'static str,
    length: usize,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
pub struct KeyBody {
    key: String,
}

#[derive(Deserialize)]
pub struct FocusBody {
    pane_id: String,
}

#[derive(Deserialize)]
pub struct ReadQuery {
    pane_id: String,
    #[serde(default = "default_lines")]
    lines: u32,
}

fn default_lines() -> u32 {
    50
}

#[derive(Serialize)]
struct AliveResponse {
    status: &'static str,
}

fn error_json(status: StatusCode, msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
}

pub async fn get_root(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<TokenQuery>,
) -> impl IntoResponse {
    if state.is_allowed_ip(&addr.ip()) {
        let body = html::render(&state.session_token);
        return Html(body).into_response();
    }
    let token = query.t.unwrap_or_default();
    if !state.check_token(&token) {
        return error_json(StatusCode::FORBIDDEN, "invalid token").into_response();
    }
    if !state.try_register_ip(addr.ip()) {
        return error_json(StatusCode::FORBIDDEN, "already registered").into_response();
    }
    eprintln!("[AUTH] 端末を登録しました: {}", addr.ip());
    let body = html::render(&state.session_token);
    Html(body).into_response()
}

pub async fn get_ping(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if !state.is_allowed_ip(&addr.ip()) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    Json(AliveResponse { status: "alive" }).into_response()
}

pub async fn post_input(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let token = headers
        .get("X-VoiceBridge-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !state.is_allowed_ip(&addr.ip()) || !state.check_token(token) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    if !state.check_rate_limit() {
        return error_json(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    if body.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "empty body").into_response();
    }

    let input: InputBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_json(StatusCode::BAD_REQUEST, &format!("invalid JSON: {e}"))
                .into_response()
        }
    };

    if input.text.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "text field is required").into_response();
    }

    if input.text.len() > MAX_TEXT_LENGTH {
        return error_json(
            StatusCode::BAD_REQUEST,
            &format!("text too long (max {MAX_TEXT_LENGTH})"),
        )
        .into_response();
    }

    tokio::time::sleep(std::time::Duration::from_millis(INJECT_DELAY_MILLIS)).await;

    if let Err(e) = state.injector.inject(&input.text, input.enter) {
        return error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("inject failed: {e}"))
            .into_response();
    }

    let length = input.text.chars().count();
    Json(OkResponse {
        status: "ok",
        length,
    })
    .into_response()
}

pub async fn post_key(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let token = headers
        .get("X-VoiceBridge-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !state.is_allowed_ip(&addr.ip()) || !state.check_token(token) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    if !state.check_rate_limit() {
        return error_json(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    let input: KeyBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_json(StatusCode::BAD_REQUEST, &format!("invalid JSON: {e}"))
                .into_response()
        }
    };

    if !is_allowed_key(&input.key) {
        return error_json(StatusCode::BAD_REQUEST, &format!("key not allowed: {}", input.key))
            .into_response();
    }

    tokio::time::sleep(std::time::Duration::from_millis(INJECT_DELAY_MILLIS)).await;

    if let Err(e) = state.injector.send_key(&input.key) {
        return error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("key send failed: {e}"))
            .into_response();
    }

    Json(serde_json::json!({"status": "ok", "key": input.key})).into_response()
}

pub async fn get_herdr_agents(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if !state.is_allowed_ip(&addr.ip()) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    match herdr::list_agents() {
        Ok(agents) => Json(agents).into_response(),
        Err(e) => {
            error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("herdr error: {e}"))
                .into_response()
        }
    }
}

pub async fn post_herdr_focus(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let token = headers
        .get("X-VoiceBridge-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !state.is_allowed_ip(&addr.ip()) || !state.check_token(token) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let input: FocusBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_json(StatusCode::BAD_REQUEST, &format!("invalid JSON: {e}"))
                .into_response()
        }
    };

    match herdr::focus_agent(&input.pane_id) {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => {
            error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("focus failed: {e}"))
                .into_response()
        }
    }
}

pub async fn get_herdr_read(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<ReadQuery>,
) -> impl IntoResponse {
    if !state.is_allowed_ip(&addr.ip()) {
        return error_json(StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let lines = query.lines.min(200);
    match herdr::read_agent(&query.pane_id, lines) {
        Ok(html) => Json(serde_json::json!({"html": html})).into_response(),
        Err(e) => {
            error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("read failed: {e}"))
                .into_response()
        }
    }
}

pub fn build_router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/", get(get_root))
        .route("/index.html", get(get_root))
        .route("/ping", get(get_ping))
        .route("/input", post(post_input))
        .route("/key", post(post_key))
        .route("/herdr/agents", get(get_herdr_agents))
        .route("/herdr/focus", post(post_herdr_focus))
        .route("/herdr/read", get(get_herdr_read))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::TextInjector;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use hyper::Request;
    use std::sync::atomic::Ordering;
    use std::time::Instant;
    use tower::ServiceExt;

    struct MockInjector;
    impl TextInjector for MockInjector {
        fn inject(&self, _text: &str, _enter: bool) -> Result<(), String> {
            Ok(())
        }
        fn send_key(&self, _key: &str) -> Result<(), String> {
            Ok(())
        }
    }

    struct FailInjector;
    impl TextInjector for FailInjector {
        fn inject(&self, _text: &str, _enter: bool) -> Result<(), String> {
            Err("mock error".to_string())
        }
        fn send_key(&self, _key: &str) -> Result<(), String> {
            Err("mock error".to_string())
        }
    }

    fn make_state() -> Arc<AppState> {
        Arc::new(AppState::new(Box::new(MockInjector)))
    }

    fn make_state_with_fail_injector() -> Arc<AppState> {
        Arc::new(AppState::new(Box::new(FailInjector)))
    }

    fn app(state: Arc<AppState>) -> axum::Router {
        build_router(state)
    }

    async fn response_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn response_text(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn get_request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))))
            .body(Body::empty())
            .unwrap()
    }

    fn post_request(uri: &str, token: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("X-VoiceBridge-Token", token)
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn post_request_raw(uri: &str, token: &str, body: &[u8]) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("X-VoiceBridge-Token", token)
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))))
            .body(Body::from(body.to_vec()))
            .unwrap()
    }

    // --- GET / ---

    #[tokio::test]
    async fn test_get_root_valid_token_returns_200() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_text(resp).await;
        assert!(body.contains("VoiceBridge"));
    }

    #[tokio::test]
    async fn test_get_root_registers_ip() {
        let state = make_state();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();
        assert!(state.ip_locked.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_get_root_html_contains_token() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();
        let body = response_text(resp).await;
        assert!(body.contains(&state.session_token));
        assert!(!body.contains("__SESSION_TOKEN__"));
    }

    #[tokio::test]
    async fn test_get_index_html_also_works() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request(&format!("/index.html?t={}", state.session_token)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_root_invalid_token_returns_403() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request("/?t=wrongtoken"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_root_empty_token_returns_403() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request("/"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_root_partial_token_returns_403() {
        let state = make_state();
        let partial = state.session_token[..8].to_string();
        let resp = app(state.clone())
            .oneshot(get_request(&format!("/?t={partial}")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_root_reload_after_registration_returns_200() {
        let state = make_state();
        let url = format!("/?t={}", state.session_token);

        let resp = app(state.clone())
            .oneshot(get_request(&url))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // リロード（トークンなしでも登録済みIPなら200）
        let resp = app(state.clone())
            .oneshot(get_request("/"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_text(resp).await;
        assert!(body.contains("VoiceBridge"));
    }

    #[tokio::test]
    async fn test_get_root_different_ip_after_registration_returns_403() {
        let state = make_state();
        let url = format!("/?t={}", state.session_token);

        app(state.clone())
            .oneshot(get_request(&url))
            .await
            .unwrap();

        // 別IPからのアクセスは403
        let req = Request::builder()
            .uri(&url)
            .extension(ConnectInfo(SocketAddr::from(([192, 168, 1, 99], 0))))
            .body(Body::empty())
            .unwrap();
        let resp = app(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // --- GET /ping ---

    #[tokio::test]
    async fn test_ping_registered_ip_returns_alive() {
        let state = make_state();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        let resp = app(state.clone())
            .oneshot(get_request("/ping"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["status"], "alive");
    }

    #[tokio::test]
    async fn test_ping_unregistered_returns_403() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request("/ping"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // --- GET unknown ---

    #[tokio::test]
    async fn test_unknown_path_returns_404() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(get_request("/unknown"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- POST /input ---

    async fn register_and_post(state: &Arc<AppState>, body: &str) -> axum::response::Response {
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        app(state.clone())
            .oneshot(post_request("/input", &state.session_token, body))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_post_input_normal_returns_200() {
        let state = make_state();
        let resp = register_and_post(&state, r#"{"text":"hello"}"#).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["length"], 5);
    }

    #[tokio::test]
    async fn test_post_input_japanese_returns_char_count() {
        let state = make_state();
        let resp = register_and_post(&state, r#"{"text":"音声テスト"}"#).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["length"], 5);
    }

    #[tokio::test]
    async fn test_post_input_wrong_token_returns_403() {
        let state = make_state();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        let resp = app(state.clone())
            .oneshot(post_request("/input", "badtoken", r#"{"text":"hi"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_post_input_unregistered_ip_returns_403() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(post_request("/input", &state.session_token, r#"{"text":"hi"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_post_input_empty_text_returns_400() {
        let state = make_state();
        let resp = register_and_post(&state, r#"{"text":""}"#).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_post_input_missing_text_returns_400() {
        let state = make_state();
        let resp = register_and_post(&state, r#"{"other":"value"}"#).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_post_input_text_too_long_returns_400() {
        let state = make_state();
        let long_text = "a".repeat(MAX_TEXT_LENGTH + 1);
        let body = format!(r#"{{"text":"{long_text}"}}"#);
        let resp = register_and_post(&state, &body).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = response_body(resp).await;
        assert!(json["error"].as_str().unwrap().contains("too long"));
    }

    #[tokio::test]
    async fn test_post_input_max_length_ok() {
        let state = make_state();
        let text = "x".repeat(MAX_TEXT_LENGTH);
        let body = format!(r#"{{"text":"{text}"}}"#);
        let resp = register_and_post(&state, &body).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_post_input_rate_limit() {
        let state = make_state();
        // First request
        let resp = register_and_post(&state, r#"{"text":"first"}"#).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Second immediately
        let resp = app(state.clone())
            .oneshot(post_request("/input", &state.session_token, r#"{"text":"second"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_post_input_rate_limit_after_cooldown() {
        let state = make_state();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        {
            let mut last = state.last_inject_time.lock().unwrap();
            *last = Instant::now() - std::time::Duration::from_secs(2);
        }

        let resp = app(state.clone())
            .oneshot(post_request("/input", &state.session_token, r#"{"text":"ok"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_post_input_empty_body_returns_400() {
        let state = make_state();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        let resp = app(state.clone())
            .oneshot(post_request_raw("/input", &state.session_token, b""))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_post_input_invalid_json_returns_400() {
        let state = make_state();
        let resp = register_and_post(&state, "not json").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_post_input_inject_failure_returns_500() {
        let state = make_state_with_fail_injector();
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        let resp = app(state.clone())
            .oneshot(post_request("/input", &state.session_token, r#"{"text":"hi"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_post_input_single_char() {
        let state = make_state();
        let resp = register_and_post(&state, r#"{"text":"a"}"#).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["length"], 1);
    }

    #[tokio::test]
    async fn test_post_wrong_path_returns_404() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(post_request("/other", "", r#"{"text":"hi"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- POST /key ---

    async fn register_and_post_key(state: &Arc<AppState>, body: &str) -> axum::response::Response {
        app(state.clone())
            .oneshot(get_request(&format!("/?t={}", state.session_token)))
            .await
            .unwrap();

        app(state.clone())
            .oneshot(post_request("/key", &state.session_token, body))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_post_key_escape_returns_200() {
        let state = make_state();
        let resp = register_and_post_key(&state, r#"{"key":"Escape"}"#).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["key"], "Escape");
    }

    #[tokio::test]
    async fn test_post_key_disallowed_returns_400() {
        let state = make_state();
        let resp = register_and_post_key(&state, r#"{"key":"F1"}"#).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"].as_str().unwrap().contains("not allowed"));
    }

    #[tokio::test]
    async fn test_post_key_unauthorized_returns_403() {
        let state = make_state();
        let resp = app(state.clone())
            .oneshot(post_request("/key", &state.session_token, r#"{"key":"Escape"}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_post_key_failure_returns_500() {
        let state = make_state_with_fail_injector();
        let resp = register_and_post_key(&state, r#"{"key":"Escape"}"#).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
