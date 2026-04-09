use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::daemon::auth::require_auth;
use crate::daemon::clipboard::{
    parse_incoming, write_to_system_clipboard, ClipboardContent, SharedState,
};

#[derive(Clone)]
pub struct AppState {
    pub clipboard: SharedState,
    pub token: String,
}

/// GET /v1/clipboard — send current Linux clipboard to iOS
async fn get_clipboard(State(state): State<AppState>) -> Response {
    let content = {
        let guard = state.clipboard.read().unwrap();
        guard.content.clone()
    };

    match content.to_bytes() {
        Ok(bytes) => {
            let mime = content.mime_type();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                bytes,
            )
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /v1/clipboard — receive clipboard content from iOS
async fn post_clipboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .split(';')
        .next()
        .unwrap_or("text/plain")
        .trim()
        .to_string();

    if mime == "application/octet-stream" {
        let filename = headers
            .get("X-Filename")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("clipperd-file")
            .to_string();

        return match save_file(&filename, &body) {
            Ok(path) => {
                notify(&format!("Clipperd: File received → {}", path));
                (StatusCode::OK, format!("Saved to {}", path)).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
    }

    let content = match parse_incoming(&body, &mime) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    if let Err(e) = write_to_system_clipboard(&content) {
        warn!("Could not write to system clipboard: {}", e);
    }

    {
        let mut guard = state.clipboard.write().unwrap();
        guard.content = content.clone();
        guard.updated_at = std::time::Instant::now();
        guard.source = "remote".to_string();
    }

    let preview = match &content {
        ClipboardContent::Text(s) => {
            let truncated = if s.len() > 60 { format!("{}…", &s[..60]) } else { s.clone() };
            format!("Clipperd: \"{}\"", truncated)
        }
        ClipboardContent::Image { .. } => "Clipperd: Image received".to_string(),
        ClipboardContent::Empty => "Clipperd: Empty clipboard".to_string(),
    };
    notify(&preview);

    StatusCode::OK.into_response()
}

/// GET /health — unauthenticated health check
async fn health() -> &'static str {
    "ok"
}

fn save_file(filename: &str, data: &[u8]) -> anyhow::Result<String> {
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clipperd-file");

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let downloads = std::path::PathBuf::from(home).join("Downloads");
    std::fs::create_dir_all(&downloads)?;
    let path = downloads.join(safe_name);
    std::fs::write(&path, data)?;
    Ok(path.to_string_lossy().to_string())
}

fn notify(msg: &str) {
    let _ = std::process::Command::new("notify-send")
        .arg("Clipperd")
        .arg(msg)
        .spawn();
}

pub fn build_app(state: AppState) -> Router {
    let token = state.token.clone();
    let authed = Router::new()
        .route("/v1/clipboard", get(get_clipboard).post(post_clipboard))
        .layer(middleware::from_fn_with_state(token, require_auth))
        .with_state(state);

    Router::new()
        .route("/health", get(health))
        .merge(authed)
        .layer(TraceLayer::new_for_http())
}

pub async fn run_https_server(
    state: AppState,
    port: u16,
    cert_pem: &str,
    key_pem: &str,
    bind_local_only: bool,
) -> anyhow::Result<()> {
    let tls_config = RustlsConfig::from_pem(
        cert_pem.as_bytes().to_vec(),
        key_pem.as_bytes().to_vec(),
    )
    .await?;

    let app = build_app(state);

    let ip: std::net::IpAddr = if bind_local_only {
        local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap())
    } else {
        "0.0.0.0".parse().unwrap()
    };

    let addr = SocketAddr::new(ip, port);
    info!("HTTPS server listening on https://{}", addr);

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

pub async fn run_setup_server(
    port: u16,
    setup_routes: Router,
) -> anyhow::Result<()> {
    let ip = local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
    let addr = SocketAddr::new(ip, port);
    info!("Setup HTTP server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, setup_routes).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state(token: &str) -> AppState {
        AppState {
            clipboard: crate::daemon::clipboard::new_shared_state(),
            token: token.to_string(),
        }
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_app(test_state("token"));
        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn clipboard_get_requires_auth() {
        let app = build_app(test_state("secret"));
        let response = app
            .oneshot(Request::builder().uri("/v1/clipboard").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn clipboard_post_requires_auth() {
        let app = build_app(test_state("secret"));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/clipboard")
                    .body(Body::from("hello"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let app = build_app(test_state("correct-token"));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/clipboard")
                    .header("Authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_then_get_clipboard_text() {
        let state = test_state("tok");
        let app = build_app(state.clone());

        let post_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/clipboard")
                    .header("Authorization", "Bearer tok")
                    .header("Content-Type", "text/plain")
                    .body(Body::from("hello from iPhone"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post_response.status(), StatusCode::OK);

        let guard = state.clipboard.read().unwrap();
        assert!(
            matches!(&guard.content, ClipboardContent::Text(s) if s == "hello from iPhone"),
            "shared state should contain posted text"
        );
        assert_eq!(guard.source, "remote");
    }

    #[tokio::test]
    async fn get_clipboard_returns_posted_content() {
        let state = test_state("tok");

        {
            let mut guard = state.clipboard.write().unwrap();
            guard.content = ClipboardContent::Text("linux clipboard content".to_string());
        }

        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/clipboard")
                    .header("Authorization", "Bearer tok")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"linux clipboard content");
    }
}
