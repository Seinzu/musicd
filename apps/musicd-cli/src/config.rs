use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER: &str = "http://127.0.0.1:7878";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub renderer_location: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    /// API tokens from `musicdctl pair`, keyed by server base URL.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
}

impl CliConfig {
    pub fn server_url(&self) -> String {
        self.server_url
            .clone()
            .unwrap_or_else(|| DEFAULT_SERVER.to_string())
    }

    /// The token for `server_url`: `MUSICD_TOKEN` if set, else the one
    /// saved by `musicdctl pair` for that server.
    pub fn api_token(&self, server_url: &str) -> Option<String> {
        std::env::var("MUSICD_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| self.tokens.get(&token_key(server_url)).cloned())
    }

    pub fn set_api_token(&mut self, server_url: &str, token: Option<String>) {
        match token {
            Some(token) => {
                self.tokens.insert(token_key(server_url), token);
            }
            None => {
                self.tokens.remove(&token_key(server_url));
            }
        }
    }

    pub fn client_id(&mut self) -> String {
        if let Some(client_id) = self
            .client_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            return client_id.clone();
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let client_id = format!("cli-{now}-{}", std::process::id());
        self.client_id = Some(client_id.clone());
        client_id
    }
}

fn token_key(server_url: &str) -> String {
    server_url.trim().trim_end_matches('/').to_string()
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
    let body = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&body).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(config: &CliConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(config).context("serializing config")?;
    write_private(&path, &body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The config can hold API tokens, so keep it readable only by its owner.
#[cfg(unix)]
fn write_private(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(body.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    fs::write(path, body)
}

#[cfg(test)]
mod tests {
    use super::CliConfig;

    #[test]
    fn stores_tokens_per_server() {
        let mut config = CliConfig::default();
        config.set_api_token("http://musicd.local:7878/", Some("mdt_a".to_string()));
        config.set_api_token("http://other:7878", Some("mdt_b".to_string()));
        assert_eq!(
            config
                .tokens
                .get("http://musicd.local:7878")
                .map(String::as_str),
            Some("mdt_a")
        );

        let body = toml::to_string_pretty(&config).expect("serialize");
        let parsed: CliConfig = toml::from_str(&body).expect("parse");
        assert_eq!(parsed.tokens, config.tokens);

        config.set_api_token("http://musicd.local:7878", None);
        assert!(!config.tokens.contains_key("http://musicd.local:7878"));
    }

    #[test]
    fn older_configs_without_tokens_still_parse() {
        let parsed: CliConfig = toml::from_str("server_url = \"http://h:7878\"\n").expect("parse");
        assert!(parsed.tokens.is_empty());
    }
}
