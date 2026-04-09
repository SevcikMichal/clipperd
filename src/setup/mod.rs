pub mod mobileconfig;
pub mod shortcut;

use axum::{
    extract::State as AxumState,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::Arc;

use crate::daemon::tls::cert_fingerprint;

#[derive(Clone)]
pub struct SetupState {
    pub mobileconfig: String,
    pub send_shortcut: String,
    pub get_shortcut: String,
    pub fingerprint: String,
    pub host_url: String,
    pub token: String,
}

pub fn setup_router(state: SetupState) -> Router {
    Router::new()
        .route("/setup", get(setup_page))
        .route("/setup.mobileconfig", get(serve_mobileconfig))
        .route("/shortcuts/send.shortcut", get(serve_send_shortcut))
        .route("/shortcuts/get.shortcut", get(serve_get_shortcut))
        .with_state(Arc::new(state))
}

async fn setup_page(AxumState(state): AxumState<Arc<SetupState>>) -> Html<String> {
    Html(render_setup_html(&state))
}

async fn serve_mobileconfig(AxumState(state): AxumState<Arc<SetupState>>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-apple-aspen-config"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"clipperd-setup.mobileconfig\""),
        ],
        state.mobileconfig.clone(),
    )
        .into_response()
}

async fn serve_send_shortcut(AxumState(state): AxumState<Arc<SetupState>>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"Clipperd-Send.shortcut\""),
        ],
        state.send_shortcut.clone(),
    )
        .into_response()
}

async fn serve_get_shortcut(AxumState(state): AxumState<Arc<SetupState>>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"Clipperd-Get.shortcut\""),
        ],
        state.get_shortcut.clone(),
    )
        .into_response()
}

fn render_setup_html(state: &SetupState) -> String {
    format!(r#"<!DOCTYPE html>
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
<p>Three steps to pair your iPhone with Linux.</p>

<div class="card">
  <h2>Step 1 — Install CA Certificate</h2>
  <div class="step"><div class="num">1</div><div>Tap <b>Install Certificate</b> — Safari downloads the profile.</div></div>
  <div class="step"><div class="num">2</div><div>Go to <b>Settings → General → VPN &amp; Device Management</b> and install it.</div></div>
  <div class="step"><div class="num">3</div><div>Go to <b>Settings → General → About → Certificate Trust Settings</b> and enable full trust for <b>Clipperd CA</b>.</div></div>
  <a class="btn" href="/setup.mobileconfig">Install Certificate</a>
  <p class="warn">⚠ Verify fingerprint matches your terminal:</p>
  <div class="mono">{fingerprint}</div>
</div>

<div class="card">
  <h2>Step 2 — Create "Clipperd Send" (iPhone → Linux)</h2>
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
</div>

<div class="card">
  <h2>Step 3 — Create "Clipperd Get" (Linux → iPhone)</h2>
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
  <h2>Step 4 — Set Up Quick Access</h2>
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
        fingerprint = state.fingerprint,
        host_url = state.host_url,
        token = state.token,
    )
}

pub fn build_setup_state(
    ca_cert_pem: &str,
    token: &str,
    port: u16,
) -> anyhow::Result<SetupState> {
    let ip = local_ip_address::local_ip()
        .unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
    let host_url = format!("https://{}:{}", ip, port);

    let mobileconfig = mobileconfig::generate_mobileconfig(ca_cert_pem)?;
    let fingerprint = cert_fingerprint(ca_cert_pem)?;
    let send_shortcut = shortcut::generate_send_shortcut(&host_url, token);
    let get_shortcut = shortcut::generate_get_shortcut(&host_url, token);

    Ok(SetupState {
        mobileconfig,
        send_shortcut,
        get_shortcut,
        fingerprint,
        host_url,
        token: token.to_string(),
    })
}
