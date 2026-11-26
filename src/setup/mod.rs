pub mod mobileconfig;
pub mod shortcut;

use axum::{
    body::Bytes,
    extract::State as AxumState,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::daemon::auth::constant_time_eq;

const SESSION_COOKIE: &str = "__Host-clipperd_pairing";
const SESSION_DURATION: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct SetupState {
    pub host_url: String,
    pub token: String,
    pub pairing_code: String,
    pairing_expires_at: Instant,
    pairing_session: Arc<Mutex<Option<PairingSession>>>,
}

struct PairingSession {
    token: String,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct BootstrapState {
    pub mobileconfig: String,
    pub fingerprint: String,
}

pub fn bootstrap_router(state: BootstrapState) -> Router {
    Router::new()
        .route("/setup", get(bootstrap_page))
        .route("/setup.mobileconfig", get(serve_mobileconfig))
        .with_state(Arc::new(state))
}

pub fn pairing_router(state: SetupState) -> Router {
    Router::new()
        .route("/pair", get(pairing_page).post(submit_pairing_code))
        .route("/shortcuts/send.shortcut", get(serve_send_shortcut))
        .route("/shortcuts/get.shortcut", get(serve_get_shortcut))
        .with_state(Arc::new(state))
}

async fn bootstrap_page(AxumState(state): AxumState<Arc<BootstrapState>>) -> Response {
    html_no_store(render_bootstrap_html(&state))
}

async fn pairing_page(AxumState(_state): AxumState<Arc<SetupState>>) -> Response {
    html_no_store(render_pairing_form())
}

async fn submit_pairing_code(
    AxumState(state): AxumState<Arc<SetupState>>,
    body: Bytes,
) -> Response {
    let submitted = body
        .strip_prefix(b"code=")
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .filter(|code| code.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let valid_code = submitted
        .is_some_and(|submitted| constant_time_eq(state.pairing_code.as_bytes(), submitted.as_bytes()));

    let mut session = state.pairing_session.lock().unwrap();
    if !valid_code || Instant::now() >= state.pairing_expires_at || session.is_some() {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
            Html("<h1>Pairing code is invalid, expired, or already used.</h1>".to_string()),
        )
            .into_response();
    }

    let random: [u8; 32] = rand::rng().random();
    let token = hex::encode(random);
    let cookie = HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age=600; Secure; HttpOnly; SameSite=Strict"
    ))
    .expect("generated session cookie must be a valid header");
    *session = Some(PairingSession {
        token,
        expires_at: Instant::now() + SESSION_DURATION,
    });

    let mut response = html_no_store(render_setup_html(&state));
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

fn authenticated_session(state: &SetupState, headers: &HeaderMap) -> bool {
    let session = state.pairing_session.lock().unwrap();
    let Some(session) = session.as_ref().filter(|session| Instant::now() < session.expires_at) else {
        return false;
    };

    headers.get_all(header::COOKIE).iter().filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .any(|(name, token)| {
            name == SESSION_COOKIE && constant_time_eq(session.token.as_bytes(), token.as_bytes())
        })
}

async fn serve_send_shortcut(
    AxumState(state): AxumState<Arc<SetupState>>,
    headers: HeaderMap,
) -> Response {
    if !authenticated_session(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    shortcut_download(
        "attachment; filename=\"Clipperd-Send.shortcut\"",
        shortcut::generate_send_shortcut(&state.host_url, &state.token),
    )
}

async fn serve_get_shortcut(
    AxumState(state): AxumState<Arc<SetupState>>,
    headers: HeaderMap,
) -> Response {
    if !authenticated_session(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    shortcut_download(
        "attachment; filename=\"Clipperd-Get.shortcut\"",
        shortcut::generate_get_shortcut(&state.host_url, &state.token),
    )
}

fn shortcut_download(disposition: &'static str, contents: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        contents,
    )
        .into_response()
}

fn html_no_store(html: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(
                    "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'",
                ),
            ),
        ],
        Html(html),
    )
        .into_response()
}

async fn serve_mobileconfig(AxumState(state): AxumState<Arc<BootstrapState>>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_TYPE, "application/x-apple-aspen-config"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"clipperd-setup.mobileconfig\"",
            ),
        ],
        state.mobileconfig.clone(),
    )
        .into_response()
}

fn render_bootstrap_html(state: &BootstrapState) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipperd Certificate Setup</title></head>
<body>
<h1>Clipperd Certificate Setup</h1>
<p>Install the CA certificate, then verify its fingerprint against the value shown in the terminal.</p>
<p><strong>Fingerprint:</strong> <code>{fingerprint}</code></p>
<p><a href="/setup.mobileconfig">Download CA certificate</a></p>
<p>After installing and trusting the certificate, open the HTTPS pairing URL printed in the terminal.</p>
</body>
</html>"#,
        fingerprint = state.fingerprint,
    )
}

fn render_pairing_form() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipperd Pairing</title></head>
<body>
<h1>Clipperd Pairing</h1>
<p>Enter the temporary code shown in the terminal.</p>
<form method="post" action="/pair">
<label for="code">Pairing code</label>
<input id="code" name="code" type="text" autocomplete="off" required>
<button type="submit">Pair device</button>
</form>
</body>
</html>"#
        .to_string()
}

fn render_setup_html(state: &SetupState) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipperd Setup</title>
<style>
  body {{ font-family: -apple-system, sans-serif; max-width: 600px; margin: 40px auto; padding: 0 20px; background: #f5f5f7; color: #1d1d1f; }}
  h1 {{ font-size: 28px; }}
  .card {{ background: white; border-radius: 12px; padding: 24px; margin: 16px 0; box-shadow: 0 2px 8px rgba(0,0,0,.08); }}
  .step {{ display: flex; gap: 16px; align-items: flex-start; margin: 12px 0; }}
  .num {{ background: #007aff; color: white; border-radius: 50%; width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; font-weight: 700; flex-shrink: 0; }}
  a.btn {{ display: inline-block; background: #007aff; color: white; padding: 12px 24px; border-radius: 8px; text-decoration: none; font-weight: 600; margin: 8px 4px; }}
  a.btn.green {{ background: #34c759; }}
  .mono {{ font-family: monospace; font-size: 13px; background: #f0f0f0; padding: 8px 12px; border-radius: 6px; word-break: break-all; }}
  .warn {{ color: #ff9500; font-size: 13px; }}
</style>
</head>
<body>
<h1>Clipperd Setup</h1>
<p>Certificate verified. Follow the steps below to configure clipboard sync.</p>
<p>Download the shortcuts, or create them manually using the instructions below.
Downloads are available for 10 minutes after pairing.</p>
<p><a class="btn" href="/shortcuts/send.shortcut">Download Clipperd Send</a>
<a class="btn green" href="/shortcuts/get.shortcut">Download Clipperd Get</a></p>

<div class="card">
  <h2>Step 1 — Create "Clipperd Send" (iPhone → Linux)</h2>
  <p>Open the <b>Shortcuts</b> app → tap <b>+</b> → tap <b>Add Action</b></p>
  <div class="step"><div class="num">1</div><div>Search for <b>Get Clipboard</b> and add it.</div></div>
  <div class="step"><div class="num">2</div><div>Search for <b>Get Contents of URL</b> and add it, then configure:
    <ul style="margin-top:8px">
      <li>URL: <span class="mono">{host_url}/v1/clipboard</span></li>
      <li>Method: <b>POST</b></li>
      <li>Tap <b>Headers</b> → Add: <span class="mono">Authorization</span> = <span class="mono">Bearer {token}</span></li>
      <li>Request Body: <b>File</b> → select <i>Clipboard</i></li>
    </ul>
  </div></div>
  <div class="step"><div class="num">3</div><div>Tap the shortcut name at the top → rename to <b>Clipperd Send</b> → tap <b>Done</b>.</div></div>
  <details style="margin-top:16px">
    <summary style="cursor:pointer;font-weight:600;color:#007aff">Optional: strip rich text formatting</summary>
    <p style="margin-top:8px">If you copy from apps that produce rich text (Mail, Notes, Safari) and want Linux to receive plain text, replace step 1–2 above with these actions:</p>
    <div class="step"><div class="num">1</div><div>Search for <b>Get Clipboard</b> and add it.</div></div>
    <div class="step"><div class="num">2</div><div>Search for <b>Get Type of Input</b> and add it — input auto-fills to <i>Clipboard</i>.</div></div>
    <div class="step"><div class="num">3</div><div>Search for <b>If</b> and add it. Set condition to <b>is</b> → <b>Rich Text</b>.</div></div>
    <div class="step"><div class="num">4</div><div>Inside the <b>If</b> block, search for <b>Text</b> and add it. Tap the text field and insert the <i>Clipboard</i> magic variable — Shortcuts will convert it to plain text.</div></div>
    <div class="step"><div class="num">5</div><div>Tap <b>End If</b> to move past it, then add <b>Get Contents of URL</b> and configure it as above, but set Request Body to <b>If Result</b> — this will be the stripped text when rich, or the original clipboard otherwise.</div></div>
  </details>
</div>

<div class="card">
  <h2>Step 2 — Create "Clipperd Get" (Linux → iPhone)</h2>
  <p>Open the <b>Shortcuts</b> app → tap <b>+</b> → tap <b>Add Action</b></p>
  <div class="step"><div class="num">1</div><div>Search for <b>Get Contents of URL</b> and add it, then configure:
    <ul style="margin-top:8px">
      <li>URL: <span class="mono">{host_url}/v1/clipboard</span></li>
      <li>Method: <b>GET</b></li>
      <li>Tap <b>Headers</b> → Add: <span class="mono">Authorization</span> = <span class="mono">Bearer {token}</span></li>
    </ul>
  </div></div>
  <div class="step"><div class="num">2</div><div>Search for <b>Set Clipboard</b> and add it (input auto-fills from previous step).</div></div>
  <div class="step"><div class="num">3</div><div>Tap the shortcut name at the top → rename to <b>Clipperd Get</b> → tap <b>Done</b>.</div></div>
</div>

<div class="card">
  <h2>Step 3 — Set Up Quick Access</h2>
  <p>Assign shortcuts to <b>Back Tap</b> for fastest access:</p>
  <p><b>Settings → Accessibility → Touch → Back Tap</b></p>
</div>

<div class="card">
  <h2>Done!</h2>
  <p>Your iPhone and Linux clipboard are paired. One tap syncs in under a second.</p>
</div>
</body>
</html>
"#,
        host_url = state.host_url,
        token = state.token,
    )
}

pub fn build_bootstrap_state(
    ca_cert_pem: &str,
    fingerprint: String,
) -> anyhow::Result<BootstrapState> {
    Ok(BootstrapState {
        mobileconfig: mobileconfig::generate_mobileconfig(ca_cert_pem)?,
        fingerprint,
    })
}

pub fn build_setup_state(token: &str, host_url: String, pairing_code: String) -> SetupState {
    SetupState {
        host_url,
        token: token.to_string(),
        pairing_code,
        pairing_expires_at: Instant::now() + Duration::from_secs(10 * 60),
        pairing_session: Arc::new(Mutex::new(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn pairing_app() -> Router {
        pairing_router(build_setup_state(
            "secret-token",
            "https://100.64.0.1:7171".to_string(),
            "a1b2c3d4".to_string(),
        ))
    }

    #[tokio::test]
    async fn pairing_form_does_not_expose_token() {
        let response = pairing_app()
            .oneshot(Request::builder().uri("/pair").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert!(!String::from_utf8_lossy(&body).contains("secret-token"));
    }

    #[tokio::test]
    async fn pairing_code_is_required_and_only_works_once() {
        let app = pairing_app();
        let invalid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair")
                    .body(Body::from("code=wrong"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

        let valid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair")
                    .body(Body::from("code=a1b2c3d4"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid.status(), StatusCode::OK);
        let body = to_bytes(valid.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Bearer secret-token"));

        let replay = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair")
                    .body(Body::from("code=a1b2c3d4"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn shortcut_downloads_require_pairing_and_valid_session() {
        let app = pairing_app();
        let paths = ["/shortcuts/send.shortcut", "/shortcuts/get.shortcut"];

        for path in paths {
            let response = app.clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        let paired = app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair")
                    .body(Body::from("code=a1b2c3d4"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = paired.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        let session_cookie = cookie.split(';').next().unwrap();

        for path in paths {
            let response = app.clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::COOKIE, session_cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body = String::from_utf8(body.to_vec()).unwrap();
            assert!(body.contains("Bearer secret-token"));
            assert!(body.contains("https://100.64.0.1:7171/v1/clipboard"));
        }

        let response = app
            .oneshot(
                Request::builder()
                    .uri(paths[0])
                    .header(header::COOKIE, format!("{SESSION_COOKIE}=invalid"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn expired_session_cannot_download_shortcuts() {
        let state = build_setup_state(
            "secret-token",
            "https://100.64.0.1:7171".to_string(),
            "a1b2c3d4".to_string(),
        );
        *state.pairing_session.lock().unwrap() = Some(PairingSession {
            token: "expired-token".to_string(),
            expires_at: Instant::now(),
        });
        let response = pairing_router(state)
            .oneshot(
                Request::builder()
                    .uri("/shortcuts/send.shortcut")
                    .header(header::COOKIE, format!("{SESSION_COOKIE}=expired-token"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
