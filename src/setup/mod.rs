pub mod mobileconfig;

use axum::{
    body::Bytes,
    extract::State as AxumState,
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::daemon::auth::constant_time_eq;

#[derive(Clone)]
pub struct SetupState {
    pub host_url: String,
    pub token: String,
    pub pairing_code: String,
    pairing_expires_at: Instant,
    pairing_used: Arc<Mutex<bool>>,
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
        .with_state(Arc::new(state))
}

async fn bootstrap_page(AxumState(state): AxumState<Arc<BootstrapState>>) -> Response {
    html_no_store(render_bootstrap_html(&state))
}

async fn pairing_page(AxumState(_state): AxumState<Arc<SetupState>>) -> Response {
    let nonce: [u8; 16] = rand::rng().random();
    let nonce = hex::encode(nonce);
    let mut response = html_no_store(render_pairing_form(&nonce));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-{nonce}'; form-action 'self'; base-uri 'none'"
        ))
        .expect("generated CSP nonce must be a valid header"),
    );
    response
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

    let mut used = state.pairing_used.lock().unwrap();
    if !valid_code || Instant::now() >= state.pairing_expires_at || *used {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
            Html("<h1>Pairing code is invalid, expired, or already used.</h1>".to_string()),
        )
            .into_response();
    }

    *used = true;
    html_no_store(render_setup_html(&state))
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
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipperd Certificate Setup</title>
<style>
  body {{ font-family: -apple-system, sans-serif; max-width: 600px; margin: 40px auto; padding: 0 20px; background: #f5f5f7; color: #1d1d1f; }}
  h1 {{ font-size: 28px; }}
  .card {{ background: white; border-radius: 12px; padding: 24px; margin: 16px 0; box-shadow: 0 2px 8px rgba(0,0,0,.08); }}
  .step {{ display: flex; gap: 16px; align-items: flex-start; margin: 12px 0; }}
  .num {{ background: #007aff; color: white; border-radius: 50%; width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; font-weight: 700; flex-shrink: 0; }}
  a.btn {{ display: inline-block; background: #007aff; color: white; padding: 12px 24px; border-radius: 8px; text-decoration: none; font-weight: 600; margin: 8px 0; }}
  .mono {{ font-family: monospace; font-size: 13px; background: #f0f0f0; padding: 8px 12px; border-radius: 6px; word-break: break-all; }}
  .warn {{ color: #995700; font-size: 14px; }}
</style>
</head>
<body>
<h1>Clipperd Setup</h1>
<p>Start by installing the CA certificate on your iPhone.</p>
<div class="card">
  <h2>Install CA Certificate</h2>
  <div class="step"><div class="num">1</div><div>Tap <b>Download Certificate</b> to download the profile.</div></div>
  <div class="step"><div class="num">2</div><div>Verify the downloaded certificate's fingerprint against the value shown in your terminal, then install the profile in <b>Settings &gt; General &gt; VPN &amp; Device Management</b>.</div></div>
  <div class="step"><div class="num">3</div><div>Go to <b>Settings &gt; General &gt; About &gt; Certificate Trust Settings</b> and enable full trust for <b>Clipperd CA</b>.</div></div>
  <a class="btn" href="/setup.mobileconfig">Download Certificate</a>
  <p class="warn">Verify the downloaded certificate independently. The fingerprint shown on this HTTP page alone does not verify it.</p>
  <p><strong>Fingerprint:</strong></p>
  <div class="mono">{fingerprint}</div>
</div>
<div class="card">
  <h2>Continue to Pairing</h2>
  <p>After trusting the certificate, scan the pairing QR code in your terminal, or open the HTTPS pairing URL and enter the code manually.</p>
</div>
</body>
</html>"#,
        fingerprint = state.fingerprint,
    )
}

fn render_pairing_form(nonce: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipperd Pairing</title>
<style>
  body {{ font-family: -apple-system, sans-serif; max-width: 600px; margin: 40px auto; padding: 0 20px; background: #f5f5f7; color: #1d1d1f; }}
  h1 {{ font-size: 28px; }}
  .card {{ background: white; border-radius: 12px; padding: 24px; margin: 16px 0; box-shadow: 0 2px 8px rgba(0,0,0,.08); }}
  label {{ display: block; font-weight: 600; margin-bottom: 8px; }}
  input {{ box-sizing: border-box; width: 100%; padding: 12px; border: 1px solid #a1a1a6; border-radius: 8px; font-family: monospace; font-size: 16px; }}
  input:focus {{ outline: 2px solid #007aff; outline-offset: 2px; }}
  button {{ background: #007aff; color: white; border: 0; border-radius: 8px; padding: 12px 24px; font: inherit; font-weight: 600; margin-top: 16px; cursor: pointer; }}
</style>
</head>
<body>
<h1>Clipperd Pairing</h1>
<p>Pair your iPhone with Clipperd.</p>
<div class="card">
<h2>Pair Device</h2>
<p>Scan the pairing QR code or enter the temporary code shown in the terminal.</p>
<form method="post" action="/pair">
<label for="code">Pairing code</label>
<input id="code" name="code" type="text" autocomplete="off" autocapitalize="none" spellcheck="false" required>
<button type="submit">Pair device</button>
</form>
</div>
<script nonce="{nonce}">
  const match = /^#code=([0-9a-f]{{32}})$/.exec(window.location.hash);
  if (match) {{
    document.getElementById('code').value = match[1];
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
  }}
</script>
</body>
</html>"#
    )
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
  .mono {{ font-family: monospace; font-size: 13px; background: #f0f0f0; padding: 8px 12px; border-radius: 6px; word-break: break-all; }}
  .warn {{ color: #ff9500; font-size: 13px; }}
</style>
</head>
<body>
<h1>Clipperd Setup</h1>
<p>Certificate verified. Follow the steps below to configure clipboard sync.</p>
<p>Create the shortcuts using the instructions below.</p>

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
        pairing_used: Arc::new(Mutex::new(false)),
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
        let csp = response.headers().get(header::CONTENT_SECURITY_POLICY).unwrap()
            .to_str().unwrap().to_string();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let nonce = body.split("<script nonce=\"").nth(1).unwrap()
            .split('"').next().unwrap();

        assert!(!body.contains("secret-token"));
        assert!(csp.contains(&format!("script-src 'nonce-{nonce}'")));
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
        assert_eq!(valid.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert!(valid.headers().get(header::SET_COOKIE).is_none());
        let body = to_bytes(valid.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Bearer secret-token"));
        assert!(body.contains("https://100.64.0.1:7171/v1/clipboard"));
        assert!(!body.contains("/shortcuts/"));

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
    async fn shortcut_downloads_are_not_available() {
        let app = pairing_app();
        for path in ["/shortcuts/send.shortcut", "/shortcuts/get.shortcut"] {
            let response = app.clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn expired_pairing_code_is_rejected() {
        let mut state = build_setup_state(
            "secret-token",
            "https://100.64.0.1:7171".to_string(),
            "a1b2c3d4".to_string(),
        );
        state.pairing_expires_at = Instant::now();
        let response = pairing_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair")
                    .body(Body::from("code=a1b2c3d4"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
