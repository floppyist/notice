use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub app: AppConfig,
}

impl Config {
    pub const LANGUAGES: [&str; 4] = ["de", "en", "es", "fr"];
    pub const THEMES: [&str; 2] = ["dark", "light"];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub notice: String,
    pub contacts: String,
    pub backup_dir: String,
    pub backup_keep: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_archive_enabled: bool,
    pub auto_archive_day: i64,
    pub trash_purge_enabled: bool,
    pub trash_purge_day: i64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 8080 }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            notice: "notice.db".into(),
            contacts: "contacts.db".into(),
            backup_dir: "backups".into(),
            backup_keep: 10,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "de".into(),
            theme: "dark".into(),
            auto_archive_enabled: false,
            auto_archive_day: 1,
            trash_purge_enabled: false,
            trash_purge_day: 30,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            app: AppConfig::default(),
        }
    }
}

impl Config {
    pub const FILE: &'static str = "config.toml";

    pub fn addr(&self) -> SocketAddr {
        let ip = self
            .server
            .host
            .parse::<IpAddr>()
            .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
        SocketAddr::new(ip, self.server.port)
    }

    /// Load the config from disk. Automatically creates the file with defaults
    /// when it does not exist yet. Returns (config, created_fresh).
    pub fn load(path: &str) -> (Config, bool) {
        let exists = Path::new(path).exists();
        let cfg = if exists {
            match std::fs::read_to_string(path) {
                Ok(s) => match toml::from_str::<Config>(&s) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Notice] Konfiguration nicht lesbar ({}): verwende Standardwerte.", e);
                        Config::default()
                    }
                },
                Err(e) => {
                    eprintln!("[Notice] Konfiguration nicht lesbar ({}): verwende Standardwerte.", e);
                    Config::default()
                }
            }
        } else {
            Config::default()
        };
        if !exists {
            let _ = cfg.save(path);
        }
        (cfg, !exists)
    }

    /// Atomically persist the config (write temp file, then rename).
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let data = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = format!("{}.tmp", path);
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Deep-merge `patch` (a JSON object) over `base`. Values on the patch win;
    /// objects are merged recursively, everything else is replaced.
    pub fn merge_json(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match (base, patch) {
            (Value::Object(b), Value::Object(p)) => {
                let mut out = b.clone();
                for (k, v) in p {
                    match out.get(k) {
                        Some(prev) => {
                            out.insert(k.clone(), Self::merge_json(prev, v));
                        }
                        None => {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
                Value::Object(out)
            }
            _ => patch.clone(),
        }
    }
}