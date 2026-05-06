use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER: &str = "http://127.0.0.1:7878";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub renderer_location: Option<String>,
}

impl CliConfig {
    pub fn server_url(&self) -> String {
        self.server_url
            .clone()
            .unwrap_or_else(|| DEFAULT_SERVER.to_string())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("locating user config dir")?;
    Ok(base.join("musicd").join("cli.toml"))
}

pub fn load() -> Result<CliConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let body = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&body).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(config: &CliConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(config).context("serializing config")?;
    fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
