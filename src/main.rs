mod config;
mod daemon;
mod setup;

use clap::{Parser, Subcommand};
use rand::Rng;
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

        /// Bind to 0.0.0.0 instead of the LAN IP and include all local interfaces in cert SAN
        #[arg(long, default_value_t = false, conflicts_with = "ext_ip")]
        bind_all: bool,

        /// Optional external IP (e.g. Tailscale/VPN) to bind and include in the TLS certificate SAN
        #[arg(long)]
        ext_ip: Option<String>,
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
            ext_ip,
        } => cmd_setup(port, bind_all, ext_ip).await,
        Commands::Run => cmd_run().await,
        Commands::Status => cmd_status(),
    }
}

async fn cmd_setup(port: u16, bind_all: bool, ext_ip: Option<String>) -> anyhow::Result<()> {
    let (cfg, fingerprint) = if config::Config::is_configured() {
        println!("ℹ  Config already exists — reusing existing keys and token.");
        println!(
            "   (Delete {} to generate fresh credentials.)",
            config::Config::config_path().display()
        );
        println!();
        let cfg = config::Config::load()?;
        let fingerprint = daemon::tls::cert_fingerprint(&cfg.ca_cert_pem)?;
        (cfg, fingerprint)
    } else {
        println!("🔐 Generating keys and certificate...");

        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "clipperd-host".to_string());

        let lan_ip = local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap());

        let extra_ip: Option<std::net::IpAddr> = ext_ip.as_ref().and_then(|ip| ip.parse().ok());

        let certs = daemon::tls::generate_certs(&hostname, lan_ip, bind_all, extra_ip)?;
        let fingerprint = daemon::tls::cert_fingerprint(&certs.ca_cert_pem)?;

        let token_bytes: [u8; 32] = rand::rng().random();
        let token = hex::encode(token_bytes);

        let display_ip = ext_ip.clone().unwrap_or_else(|| lan_ip.to_string());

        let bind_local_only = !bind_all && ext_ip.is_none();

        let bind_ip = if bind_all {
            Some("0.0.0.0".to_string())
        } else if let Some(ref eip) = ext_ip {
            Some(eip.clone())
        } else {
            None
        };

        let cfg = config::Config {
            token: token.clone(),
            port,
            bind_local_only,
            bind_ip,
            cert_pem: certs.cert_pem.clone(),
            key_pem: certs.key_pem.clone(),
            ca_cert_pem: certs.ca_cert_pem.clone(),
            cert_ip: display_ip.clone(),
        };
        cfg.save()?;

        println!(
            "✅ Config saved to {}",
            config::Config::config_path().display()
        );
        println!();
        println!("🌐 Server cert issued for IP: {}", display_ip);
        println!("   iPhone MUST connect to this address — if wrong, re-run setup on the correct network");
        println!();

        (cfg, fingerprint)
    };

    println!("📱 CA Certificate Fingerprint:");
    println!("   {}", fingerprint);
    println!();

    let setup_state = setup::build_setup_state(&cfg.ca_cert_pem, &cfg.token, cfg.port)?;

    let setup_ip = ext_ip.clone().unwrap_or_else(|| {
        local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string())
    });

    let setup_url = format!("http://{}:{}/setup", setup_ip, port);

    // Print QR code
    println!("📷 Scan this QR code on your iPhone:");
    println!();
    print_qr(&setup_url);
    println!();
    println!("   Or open:  {}", setup_url);
    println!();
    println!("📡 Starting setup server... (Ctrl+C when done)");
    println!();

    let router = setup::setup_router(setup_state);
    daemon::server::run_setup_server(port, router, bind_all, ext_ip.as_deref()).await?;

    Ok(())
}

async fn cmd_run() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    info!(
        "Loaded config, port={}, local_only={}",
        config.port, config.bind_local_only
    );
    daemon::run(config).await
}

fn cmd_status() -> anyhow::Result<()> {
    if !config::Config::is_configured() {
        println!("Not configured. Run `clipperd setup` first.");
        return Ok(());
    }

    let config = config::Config::load()?;
    let ip = local_ip_address::local_ip().unwrap_or_else(|_| "127.0.0.1".parse().unwrap());

    println!("Clipperd Status");
    println!("──────────────────");
    println!("Config:   {}", config::Config::config_path().display());
    println!("LAN IP:   {}", ip);
    println!("Port:     {}", config.port);
    println!("URL:      https://{}:{}", ip, config.port);
    println!("Local:    {}", config.bind_local_only);

    let fingerprint =
        daemon::tls::cert_fingerprint(&config.ca_cert_pem).unwrap_or_else(|_| "error".to_string());
    println!("Cert CA:  {}", fingerprint);
    println!(
        "Cert IP:  {} (embedded in server cert SAN — iPhone must connect to this IP)",
        config.cert_ip
    );

    let health_url = format!("https://{}:{}/health", ip, config.port);
    println!("Health:   {}", health_url);

    Ok(())
}

fn print_qr(url: &str) {
    use qrcode::{render::unicode, QrCode};
    match QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<unicode::Dense1x2>()
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
