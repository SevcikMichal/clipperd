mod config;
mod daemon;
mod setup;

use clap::{Parser, Subcommand};
use rand::Rng;
use std::net::{IpAddr, SocketAddr};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "clipperd",
    about = "Seamless clipboard sync between iPhone and Linux",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate keys (first run) and start the pairing wizard
    Setup {
        /// Port for the HTTPS daemon (default: 7171)
        #[arg(long, default_value_t = 7171)]
        port: u16,

        /// Bind to 0.0.0.0 instead of the LAN IP (less secure)
        #[arg(long, default_value_t = false, conflicts_with = "bind_ip")]
        bind_all: bool,

        /// Bind only to this local interface address
        #[arg(long, conflicts_with = "bind_all")]
        bind_ip: Option<IpAddr>,
    },

    /// Run the clipboard sync daemon
    Run,

    /// Show daemon status and configuration
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("clipperd=info,warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Setup {
            port,
            bind_all,
            bind_ip,
        } => cmd_setup(port, bind_all, bind_ip).await,
        Commands::Run => cmd_run().await,
        Commands::Status => cmd_status(),
    }
}

async fn cmd_setup(
    port: u16,
    bind_all: bool,
    requested_bind_ip: Option<IpAddr>,
) -> anyhow::Result<()> {
    if requested_bind_ip.is_some_and(|ip| ip.is_unspecified()) {
        anyhow::bail!(
            "--bind-ip must be a specific local interface address, not an unspecified address"
        );
    }
    if let Some(ip) = requested_bind_ip {
        drop(tokio::net::TcpListener::bind(SocketAddr::new(ip, 0)).await?);
    }

    let (cfg, fingerprint) = if config::Config::is_configured() {
        println!("ℹ  Config already exists — reusing existing keys and token.");
        println!("   (Delete {} to generate fresh credentials.)", config::Config::config_path().display());
        println!();
        let cfg = config::Config::load()?;
        if let Some(ip) = requested_bind_ip {
            if cfg.bind_ip.as_deref() != Some(ip.to_string().as_str()) {
                anyhow::bail!(
                    "This config was not created for --bind-ip {ip}. Remove {} and run setup again to generate a matching certificate.",
                    config::Config::config_path().display()
                );
            }
        }
        if bind_all && (cfg.bind_local_only || cfg.bind_ip.is_some()) {
            anyhow::bail!(
                "This config was created for a different binding mode. Remove {} and run setup again to change its binding mode.",
                config::Config::config_path().display()
            );
        }
        let fingerprint = daemon::tls::cert_fingerprint(&cfg.ca_cert_pem)?;
        (cfg, fingerprint)
    } else {
        println!("🔐 Generating keys and certificate...");

        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "clipperd-host".to_string());

        let lan_ip = local_ip_address::local_ip()
            .unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
        let certs = daemon::tls::generate_certs(&hostname, lan_ip, bind_all, requested_bind_ip)?;
        let fingerprint = daemon::tls::cert_fingerprint(&certs.ca_cert_pem)?;

        let token_bytes: [u8; 32] = rand::rng().random();
        let token = hex::encode(token_bytes);
        let cert_ip = requested_bind_ip.unwrap_or(lan_ip);

        let cfg = config::Config {
            token: token.clone(),
            port,
            bind_local_only: !bind_all && requested_bind_ip.is_none(),
            bind_ip: requested_bind_ip.map(|ip| ip.to_string()),
            cert_pem: certs.cert_pem.clone(),
            key_pem: certs.key_pem.clone(),
            ca_cert_pem: certs.ca_cert_pem.clone(),
            cert_ip: cert_ip.to_string(),
        };
        cfg.save()?;

        println!("✅ Config saved to {}", config::Config::config_path().display());
        println!();
        println!("🌐 Server cert issued for IP: {}", cert_ip);
        println!("   iPhone MUST connect to this address — if wrong, re-run setup on the correct network");
        println!();

        (cfg, fingerprint)
    };

    println!("📱 CA Certificate Fingerprint:");
    println!("   {}", fingerprint);
    println!();

    let bind_ip = cfg.bind_ip.as_deref().map(str::parse).transpose()?;
    let setup_ip = bind_ip.unwrap_or_else(|| {
        local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap())
    });
    let setup_port = cfg
        .port
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("The setup port cannot be incremented"))?;
    let setup_url = format!("http://{}/setup", SocketAddr::new(setup_ip, setup_port));
    let pairing_url = format!("https://{}/pair", SocketAddr::new(setup_ip, cfg.port));
    let pairing_code: [u8; 16] = rand::rng().random();
    let pairing_code = hex::encode(pairing_code);
    let host_url = format!("https://{}", SocketAddr::new(setup_ip, cfg.port));

    let bootstrap_state =
        setup::build_bootstrap_state(&cfg.ca_cert_pem, fingerprint.clone())?;
    let pairing_state = setup::build_setup_state(&cfg.token, host_url, pairing_code.clone());

    println!("📷 Scan this QR code to install the CA certificate:");
    println!();
    print_qr(&setup_url);
    println!();
    println!("   Or open:  {}", setup_url);
    println!();
    println!("🔐 After installing and trusting the CA certificate, scan this QR code to pair:");
    println!();
    print_qr(&format!("{}#code={}", pairing_url, pairing_code));
    println!();
    println!("   Or open:");
    println!("   {}", pairing_url);
    println!(
        "   Pairing code (valid for 10 minutes, one use): {}",
        pairing_code
    );
    println!();
    println!("📡 Starting setup server... (Ctrl+C when done)");
    println!();

    daemon::server::run_setup_server(
        cfg.port,
        setup::bootstrap_router(bootstrap_state),
        setup::pairing_router(pairing_state),
        &cfg.cert_pem,
        &cfg.key_pem,
        cfg.bind_local_only,
        bind_ip,
    )
    .await?;

    Ok(())
}

async fn cmd_run() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    info!("Loaded config, port={}, local_only={}", config.port, config.bind_local_only);
    daemon::run(config).await
}

fn cmd_status() -> anyhow::Result<()> {
    if !config::Config::is_configured() {
        println!("Not configured. Run `clipperd setup` first.");
        return Ok(());
    }

    let config = config::Config::load()?;
    let ip = config
        .bind_ip
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(|| {
            local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap())
        });

    println!("Clipperd Status");
    println!("──────────────────");
    println!("Config:   {}", config::Config::config_path().display());
    println!("Bind IP:  {}", ip);
    println!("Port:     {}", config.port);
    let address = SocketAddr::new(ip, config.port);
    println!("URL:      https://{}", address);
    println!("Local:    {}", config.bind_local_only);

    let fingerprint = daemon::tls::cert_fingerprint(&config.ca_cert_pem)
        .unwrap_or_else(|_| "error".to_string());
    println!("Cert CA:  {}", fingerprint);
    println!("Cert IP:  {} (embedded in server cert SAN — iPhone must connect to this IP)", config.cert_ip);

    let health_url = format!("https://{}/health", address);
    println!("Health:   {}", health_url);

    Ok(())
}

fn print_qr(url: &str) {
    use qrcode::{QrCode, render::unicode};
    match QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let rendered = code.render::<unicode::Dense1x2>()
                .dark_color(unicode::Dense1x2::Dark)
                .light_color(unicode::Dense1x2::Light)
                .build();
            println!("{}", rendered);
        }
        Err(e) => {
            eprintln!("QR generation failed: {}", e);
        }
    }
}
