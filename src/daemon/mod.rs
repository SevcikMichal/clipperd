pub mod auth;
pub mod clipboard;
pub mod server;
pub mod tls;

use crate::config::Config;
use server::AppState;
use tracing::info;

pub async fn run(config: Config) -> anyhow::Result<()> {
    info!("Starting clipperd daemon...");

    let clipboard_state = clipboard::new_shared_state();

    // Start clipboard polling task
    let state_clone = clipboard_state.clone();
    tokio::spawn(async move {
        clipboard::poll_clipboard(state_clone).await;
    });

    // Run HTTPS server
    let app_state = AppState {
        clipboard: clipboard_state,
        token: config.token.clone(),
    };

    server::run_https_server(
        app_state,
        config.port,
        &config.cert_pem,
        &config.key_pem,
        config.bind_local_only,
    )
    .await?;

    Ok(())
}
