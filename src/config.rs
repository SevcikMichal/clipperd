use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    pub port: u16,
    pub bind_local_only: bool,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
    /// LAN IP that was embedded in the server cert's SAN at setup time
    pub cert_ip: String,
}

impl Config {
    pub fn config_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".config").join("clipperd")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path();
        let content = std::fs::read_to_string(&path)
            .map_err(|_| anyhow::anyhow!("Config not found at {}. Run `clipperd setup` first.", path.display()))?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let content = toml::to_string_pretty(self)?;
        // Restrict permissions: owner read/write only
        let path = Self::config_path();
        std::fs::write(&path, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn is_configured() -> bool {
        Self::config_path().exists()
    }
}
