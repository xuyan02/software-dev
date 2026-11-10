//! Cross-cutting configuration and telemetry shared by binaries.

use serde::Deserialize;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Overrides the config file path. When unset, Foundry reads
/// `~/.foundry/foundry.toml` if it exists.
pub const CONFIG_FILE_ENV: &str = "FOUNDRY_CONFIG";
pub const CONFIG_DIR: &str = ".foundry";
pub const DEFAULT_CONFIG_FILE: &str = "foundry.toml";
pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";

#[derive(Debug, Clone)]
pub struct Config {
    pub addr: SocketAddr,
    /// Postgres connection string. State is durable; the in-memory adapter is
    /// only used by tests.
    pub database_url: String,
    /// Initial account used to authorize the first-run setup flow. This is an
    /// authorization identity, not an account; it is never stored.
    pub bootstrap_handle: String,
    /// Default display name prefilled on the new-administrator form.
    pub bootstrap_name: String,
    /// Initial password paired with `bootstrap_handle`. The pair gates the
    /// first-run setup flow: only someone who holds the configured values can
    /// create the first administrator. It is never the administrator's
    /// password and is required only while the account table is empty.
    pub bootstrap_password: Option<String>,
    pub session_ttl_hours: i64,
    /// Platform model API key used by managed agents. It is never written to
    /// the database; env `OPENAI_API_KEY` takes precedence over the file.
    pub openai_api_key: Option<String>,
    /// Optional platform model base URL and model name. Managed agent
    /// definitions are reconciled from these values at startup.
    pub openai_base_url: Option<String>,
    pub openai_model: Option<String>,
    /// Built SPA assets for the workflow editor. The control plane serves
    /// `/workflows/editor/` from this directory and answers 503 while it is
    /// missing.
    pub editor_dir: PathBuf,
    /// The file that was loaded, if any. `None` means environment + defaults.
    pub config_file: Option<PathBuf>,
}

/// Flat TOML file. Every key is optional so the file can set only what it
/// needs; environment variables take precedence over file values.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    addr: Option<String>,
    database_url: Option<String>,
    /// Deprecated: projects are created explicitly after setup. Accepted so an
    /// older config file still starts, but ignored.
    #[serde(default)]
    project: Option<String>,
    /// Deprecated, see `project`.
    #[serde(default)]
    project_name: Option<String>,
    bootstrap_handle: Option<String>,
    bootstrap_name: Option<String>,
    bootstrap_password: Option<String>,
    session_ttl_hours: Option<i64>,
    openai_api_key: Option<String>,
    openai_base_url: Option<String>,
    openai_model: Option<String>,
    editor_dir: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid address: {0}")]
    InvalidAddr(String),
    #[error("invalid {name}: {value}")]
    InvalidNumber { name: &'static str, value: String },
    #[error(
        "DATABASE_URL must be set in the environment or the Foundry config file (for example postgres://foundry:foundry@localhost:5432/foundry)"
    )]
    MissingDatabaseUrl,
    #[error("failed to read config file {path}: {source}")]
    ReadConfig {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    ParseConfig {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

impl Config {
    /// Load configuration with this precedence:
    ///
    /// 1. environment variables,
    /// 2. `FOUNDRY_CONFIG`, or `~/.foundry/foundry.toml` when present,
    /// 3. built-in defaults.
    pub fn from_env() -> Result<Self, ConfigError> {
        let path = config_path();
        let file = match &path {
            Some(path) => load_config_file(path)?,
            None => ConfigFile::default(),
        };
        if file.project.is_some() || file.project_name.is_some() {
            tracing::warn!(
                "config keys `project` and `project_name` are obsolete and ignored; \
                 projects are created explicitly after first-run setup"
            );
        }
        Self::resolve(|name| std::env::var(name).ok(), file, path)
    }

    fn resolve(
        env: impl Fn(&str) -> Option<String>,
        file: ConfigFile,
        config_file: Option<PathBuf>,
    ) -> Result<Self, ConfigError> {
        let pick = |name: &str, file_value: Option<String>| {
            env(name).filter(|value| !value.is_empty()).or(file_value)
        };
        let addr_text = pick("FOUNDRY_ADDR", file.addr).unwrap_or_else(|| DEFAULT_ADDR.to_string());
        let addr = addr_text
            .trim()
            .parse()
            .map_err(|_| ConfigError::InvalidAddr(addr_text))?;
        let ttl_text = pick(
            "FOUNDRY_SESSION_TTL_HOURS",
            file.session_ttl_hours.map(|value| value.to_string()),
        );
        let session_ttl_hours = match ttl_text {
            Some(value) => value
                .trim()
                .parse()
                .map_err(|_| ConfigError::InvalidNumber {
                    name: "FOUNDRY_SESSION_TTL_HOURS",
                    value,
                })?,
            None => 168,
        };
        let database_url = pick("DATABASE_URL", file.database_url)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or(ConfigError::MissingDatabaseUrl)?;
        Ok(Self {
            addr,
            database_url,
            bootstrap_handle: pick("FOUNDRY_BOOTSTRAP_HANDLE", file.bootstrap_handle)
                .unwrap_or_else(|| "admin".to_string()),
            bootstrap_name: pick("FOUNDRY_BOOTSTRAP_NAME", file.bootstrap_name)
                .unwrap_or_else(|| "Administrator".to_string()),
            // The initial password is compared verbatim, so do not trim it:
            // leading and trailing spaces are part of the secret if the
            // operator put them there.
            bootstrap_password: pick("FOUNDRY_BOOTSTRAP_PASSWORD", file.bootstrap_password)
                .filter(|value| !value.is_empty()),
            session_ttl_hours,
            openai_api_key: pick("OPENAI_API_KEY", file.openai_api_key)
                .filter(|value| !value.is_empty()),
            openai_base_url: pick("OPENAI_BASE_URL", file.openai_base_url)
                .filter(|value| !value.is_empty()),
            openai_model: pick("OPENAI_MODEL", file.openai_model).filter(|value| !value.is_empty()),
            editor_dir: PathBuf::from(
                pick("FOUNDRY_EDITOR_DIR", file.editor_dir)
                    .unwrap_or_else(|| "web/workflow-editor/dist".to_string()),
            ),
            config_file,
        })
    }
}

fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(CONFIG_FILE_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    default_config_path().filter(|path| path.is_file())
}

/// The user config file path: `~/.foundry/foundry.toml`. Returns `None` when
/// `HOME` is not set.
pub fn default_config_path() -> Option<PathBuf> {
    config_home(std::env::var_os("HOME")).map(|dir| dir.join(CONFIG_DIR).join(DEFAULT_CONFIG_FILE))
}

fn config_home(home: Option<OsString>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn load_config_file(path: &Path) -> Result<ConfigFile, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadConfig {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::ParseConfig {
        path: path.display().to_string(),
        source,
    })
}

/// One plain-text log stream per managed agent process.
///
/// The control plane captures `foundry-agentd`'s stdout/stderr into these
/// files; the console tails them. There is no structured record layer: the
/// existing `tracing` output is the log.
#[derive(Debug, Clone)]
pub struct AgentLogFiles {
    dir: PathBuf,
}

impl AgentLogFiles {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `~/.foundry/logs/agents`, falling back to `logs/agents`.
    pub fn default_dir() -> PathBuf {
        default_config_path()
            .and_then(|path| path.parent().map(|dir| dir.join("logs").join("agents")))
            .unwrap_or_else(|| PathBuf::from("logs").join("agents"))
    }

    pub fn path(&self, agent_id: &str) -> PathBuf {
        self.dir.join(format!("{agent_id}.log"))
    }

    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)
    }

    /// Read at most `max_bytes` from the end of the log. A partial first line
    /// is dropped so the viewer starts on a line boundary.
    pub fn tail(&self, agent_id: &str, max_bytes: u64) -> std::io::Result<String> {
        let mut file = match std::fs::File::open(self.path(agent_id)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => return Err(error),
        };
        let len = file.metadata()?.len();
        let start = len.saturating_sub(max_bytes);
        if start > 0 {
            file.seek(SeekFrom::Start(start))?;
        }
        let mut buffer = Vec::new();
        file.take(max_bytes).read_to_end(&mut buffer)?;
        if start > 0
            && let Some(index) = buffer.iter().position(|byte| *byte == b'\n')
        {
            buffer.drain(..=index);
        }
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }

    /// Start each process with a bounded file. The previous file is kept as
    /// `<id>.log.1`, replacing any older rotation.
    pub fn rotate_if_larger(&self, agent_id: &str, max_bytes: u64) -> std::io::Result<()> {
        let path = self.path(agent_id);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.len() > max_bytes => {
                std::fs::rename(&path, path.with_extension("log.1"))?;
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Initialize tracing once. Safe to call from tests.
pub fn init_tracing(default_filter: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(std::io::stdout().is_terminal())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn file(text: &str) -> ConfigFile {
        toml::from_str(text).expect("valid config file")
    }

    fn resolve(env: &[(&str, &str)], config: ConfigFile) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = env
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        Config::resolve(|name| env.get(name).cloned(), config, None)
    }

    #[test]
    fn file_values_are_used_and_defaults_fill_the_rest() {
        let config = resolve(
            &[],
            file(
                r#"
                addr = "0.0.0.0:9000"
                database_url = "postgres://foundry@db/foundry"
                session_ttl_hours = 24
                bootstrap_password = "file-setup-pass"
                "#,
            ),
        )
        .unwrap();
        assert_eq!(config.addr.to_string(), "0.0.0.0:9000");
        assert_eq!(config.database_url, "postgres://foundry@db/foundry");
        assert_eq!(config.session_ttl_hours, 24);
        assert_eq!(config.editor_dir, PathBuf::from("web/workflow-editor/dist"));
        assert_eq!(config.bootstrap_handle, "admin");
        assert_eq!(
            config.bootstrap_password.as_deref(),
            Some("file-setup-pass")
        );
        assert!(config.config_file.is_none());
    }

    #[test]
    fn model_settings_come_from_the_file_and_env_wins() {
        let config = resolve(
            &[("OPENAI_API_KEY", "env-key")],
            file(
                r#"
                database_url = "postgres://foundry@db/foundry"
                openai_api_key = "file-key"
                openai_base_url = "https://api.deepseek.com/v1"
                openai_model = "deepseek-chat"
                "#,
            ),
        )
        .unwrap();
        assert_eq!(config.openai_api_key.as_deref(), Some("env-key"));
        assert_eq!(
            config.openai_base_url.as_deref(),
            Some("https://api.deepseek.com/v1")
        );
        assert_eq!(config.openai_model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn environment_overrides_the_file() {
        let config = resolve(
            &[
                ("FOUNDRY_ADDR", "127.0.0.1:7000"),
                ("DATABASE_URL", "postgres://env@db/foundry"),
                ("FOUNDRY_BOOTSTRAP_HANDLE", "from-env"),
                ("FOUNDRY_BOOTSTRAP_PASSWORD", "env-setup-pass"),
                ("FOUNDRY_SESSION_TTL_HOURS", "1"),
                ("FOUNDRY_EDITOR_DIR", "/srv/foundry-editor"),
            ],
            file(
                r#"
                addr = "0.0.0.0:9000"
                database_url = "postgres://file@db/foundry"
                bootstrap_handle = "from-file"
                bootstrap_password = "file-setup-pass"
                session_ttl_hours = 24
                "#,
            ),
        )
        .unwrap();
        assert_eq!(config.addr.to_string(), "127.0.0.1:7000");
        assert_eq!(config.database_url, "postgres://env@db/foundry");
        assert_eq!(config.bootstrap_handle, "from-env");
        assert_eq!(config.bootstrap_password.as_deref(), Some("env-setup-pass"));
        assert_eq!(config.session_ttl_hours, 1);
        assert_eq!(config.editor_dir, PathBuf::from("/srv/foundry-editor"));
    }

    #[test]
    fn database_url_is_required() {
        assert!(matches!(
            resolve(&[], ConfigFile::default()),
            Err(ConfigError::MissingDatabaseUrl)
        ));
        assert!(matches!(
            resolve(&[("DATABASE_URL", "")], ConfigFile::default()),
            Err(ConfigError::MissingDatabaseUrl)
        ));
    }

    #[test]
    fn obsolete_project_seed_keys_are_accepted_but_ignored() {
        let config = resolve(
            &[],
            file(
                r#"
                database_url = "postgres://foundry@db/foundry"
                project = "default"
                project_name = "Default"
                "#,
            ),
        )
        .unwrap();
        assert_eq!(config.database_url, "postgres://foundry@db/foundry");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<ConfigFile>("unknown = 1").is_err());
    }

    #[test]
    fn config_home_uses_home() {
        assert_eq!(
            config_home(Some(OsString::from("/home/tester"))),
            Some(PathBuf::from("/home/tester"))
        );
        assert_eq!(config_home(Some(OsString::new())), None);
        assert_eq!(config_home(None), None);
    }

    #[test]
    fn agent_log_files_tail_and_truncate() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "foundry-agent-logs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let logs = AgentLogFiles::new(dir.clone());
        logs.ensure_dir().unwrap();
        let path = logs.path("agent");
        std::fs::write(&path, "line one\nline two\nline three\n").unwrap();

        assert_eq!(logs.tail("agent", 12).unwrap(), "line three\n");
        logs.rotate_if_larger("agent", 5).unwrap();
        assert!(!path.exists());
        assert!(path.with_extension("log.1").exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}
