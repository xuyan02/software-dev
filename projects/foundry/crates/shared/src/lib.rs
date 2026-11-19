//! Cross-cutting configuration and telemetry shared by binaries.

use serde::Deserialize;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

pub const CONFIG_DIR: &str = ".foundry";
pub const DEFAULT_CONFIG_FILE: &str = "foundry.toml";
pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";
pub const DEFAULT_SHELL_IMAGE: &str = "localhost/openergasterion-shell:1";

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
    /// Base URL of the `ds-server` used to provision project design spaces.
    /// Project creation fails when this is not configured.
    pub design_space_url: Option<String>,
    /// Local ds-server repository collection root.
    pub design_space_root: PathBuf,
    /// Local ds-server listen address. It must be reachable from agent shells.
    pub design_space_listen: String,
    /// Bearer token file for the ds-server.
    pub design_space_token_file: PathBuf,
    /// URL project fields should use so agent shells can reach ds-server.
    /// Defaults to the detected host IP on the local ds-server port.
    pub design_space_agent_url: Option<String>,
    /// Built SPA assets for the workflow editor. The control plane serves
    /// `/workflows/editor/` from this directory and answers 503 while it is
    /// missing.
    pub editor_dir: PathBuf,
    /// GitHub App used to mint short-lived source repository tokens.
    pub github_app_id: Option<u64>,
    /// Path to the GitHub App private key. The key itself is never copied into
    /// the database or exposed to agents.
    pub github_app_private_key_file: Option<PathBuf>,
    /// GitHub API base; GitHub Enterprise uses `https://host/api/v3`.
    pub github_api_base: String,
    /// Container image used for an agent's persistent shell.
    pub shell_image: String,
    /// The file that was loaded, if any. `None` means defaults only.
    pub config_file: Option<PathBuf>,
}

/// Flat TOML file. Every key is optional so the file can set only what it
/// needs; missing keys fall back to the built-in defaults.
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
    design_space_url: Option<String>,
    design_space_root: Option<String>,
    design_space_listen: Option<String>,
    design_space_token_file: Option<String>,
    design_space_agent_url: Option<String>,
    editor_dir: Option<String>,
    github_app_id: Option<GithubAppId>,
    github_app_private_key_file: Option<String>,
    github_api_base: Option<String>,
    shell_image: Option<String>,
    /// Deprecated: human-review skipping is a project setting on the project
    /// page. Accepted and ignored so an older config file still starts.
    #[serde(default)]
    skip_human_review: Option<bool>,
}

/// TOML authors often quote the App ID even though GitHub shows a number.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum GithubAppId {
    Integer(u64),
    String(String),
}

impl GithubAppId {
    fn into_text(self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::String(value) => value,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid address: {0}")]
    InvalidAddr(String),
    #[error("invalid {name}: {value}")]
    InvalidNumber { name: &'static str, value: String },
    #[error(
        "database_url must be set in ~/.foundry/foundry.toml (for example postgres://foundry:foundry@localhost:5432/foundry)"
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
    #[error("invalid github_app_id: {0}")]
    InvalidGithubAppId(String),
    #[error("cannot read GitHub App private key {path}: {source}")]
    GithubPrivateKey {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl Config {
    /// Load configuration from `~/.foundry/foundry.toml`.
    ///
    /// Foundry does not read configuration from environment variables. A
    /// deployment that needs a different file must mount it at the default
    /// location.
    pub fn from_file() -> Result<Self, ConfigError> {
        let path = default_config_path().filter(|path| path.is_file());
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
        Self::resolve(file, path)
    }

    fn resolve(file: ConfigFile, config_file: Option<PathBuf>) -> Result<Self, ConfigError> {
        let addr_text = file.addr.unwrap_or_else(|| DEFAULT_ADDR.to_string());
        let addr = addr_text
            .trim()
            .parse()
            .map_err(|_| ConfigError::InvalidAddr(addr_text))?;
        let session_ttl_hours = file.session_ttl_hours.unwrap_or(168);
        let database_url = file
            .database_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or(ConfigError::MissingDatabaseUrl)?;
        let github_app_id = file
            .github_app_id
            .map(GithubAppId::into_text)
            .map(|value| {
                value
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| ConfigError::InvalidGithubAppId(value))
            })
            .transpose()?;
        let github_app_private_key_file = file
            .github_app_private_key_file
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty());
        if let Some(path) = &github_app_private_key_file {
            std::fs::read(path).map_err(|source| ConfigError::GithubPrivateKey {
                path: path.display().to_string(),
                source,
            })?;
        }
        let github_api_base = file
            .github_api_base
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "https://api.github.com".to_string());
        let shell_image = file
            .shell_image
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_SHELL_IMAGE.to_string());
        let foundry_dir = default_config_path()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(".foundry"));
        let design_space_root = file
            .design_space_root
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| foundry_dir.join("design-spaces"));
        let design_space_listen = file
            .design_space_listen
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "0.0.0.0:7700".to_string());
        let design_space_token_file = file
            .design_space_token_file
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| foundry_dir.join("design-space-token"));
        let design_space_agent_url = file
            .design_space_agent_url
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        let _ = file.skip_human_review;
        Ok(Self {
            addr,
            database_url,
            bootstrap_handle: file.bootstrap_handle.unwrap_or_else(|| "admin".to_string()),
            bootstrap_name: file
                .bootstrap_name
                .unwrap_or_else(|| "Administrator".to_string()),
            // The initial password is compared verbatim, so do not trim it:
            // leading and trailing spaces are part of the secret if the
            // operator put them there.
            bootstrap_password: file.bootstrap_password.filter(|value| !value.is_empty()),
            session_ttl_hours,
            openai_api_key: file.openai_api_key.filter(|value| !value.is_empty()),
            openai_base_url: file.openai_base_url.filter(|value| !value.is_empty()),
            openai_model: file.openai_model.filter(|value| !value.is_empty()),
            design_space_url: file
                .design_space_url
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty()),
            design_space_root,
            design_space_listen,
            design_space_token_file,
            design_space_agent_url,
            editor_dir: PathBuf::from(
                file.editor_dir
                    .unwrap_or_else(|| "web/workflow-editor/dist".to_string()),
            ),
            github_app_id,
            github_app_private_key_file,
            github_api_base,
            shell_image,
            config_file,
        })
    }
}

/// The user config file path: `~/.foundry/foundry.toml`. Returns `None` when
/// `HOME` is not set.
pub fn default_config_path() -> Option<PathBuf> {
    config_home(std::env::var_os("HOME")).map(|dir| dir.join(CONFIG_DIR).join(DEFAULT_CONFIG_FILE))
}

fn config_home(home: Option<OsString>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// The per-user Foundry data directory: `~/.foundry`, falling back to
/// `.foundry`.
pub fn default_foundry_dir() -> PathBuf {
    default_config_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(CONFIG_DIR))
}

/// The persistent host directory mounted at `/work` in one agent's shell.
pub fn default_agent_workspace_dir(agent_id: &str) -> PathBuf {
    default_foundry_dir().join("workspaces").join(agent_id)
}

/// Create a directory readable and writable only by the current user.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
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

    /// Cheap change detector for the log viewer: size and mtime.
    pub fn fingerprint(&self, agent_id: &str) -> std::io::Result<String> {
        match std::fs::metadata(self.path(agent_id)) {
            Ok(metadata) => {
                let modified = metadata
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                Ok(format!("{}:{}", metadata.len(), modified.as_nanos()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("none".to_string()),
            Err(error) => Err(error),
        }
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

/// Initialize tracing for agent processes captured into the console log.
/// The agent log viewer only shows message content, so omit timestamp, level,
/// target and source location.
pub fn init_agent_tracing(default_filter: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .with_ansi(false)
        .try_init();
}

/// Remove the tracing prefix from captured agent log lines.
///
/// Old log files still contain `timestamp LEVEL target: message`; the viewer
/// strips it so it can show only message content.
pub fn strip_agent_log_prefixes(log: &str) -> String {
    log.lines()
        .map(strip_agent_log_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_agent_log_line(line: &str) -> &str {
    let trimmed = line.trim_start();
    let Some(space) = trimmed.find(|character: char| character.is_whitespace()) else {
        return line;
    };
    let timestamp = &trimmed[..space];
    if !looks_like_timestamp(timestamp) {
        return line;
    }
    let rest = trimmed[space..].trim_start();
    let mut level_rest = None;
    for level in ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"] {
        if let Some(value) = rest.strip_prefix(level) {
            level_rest = Some(value);
            break;
        }
    }
    let Some(rest) = level_rest else {
        return line;
    };
    let rest = rest.trim_start();
    let message = rest
        .split_once(": ")
        .map(|(_, message)| message)
        .unwrap_or(rest);
    strip_shell_output_prefix(message)
}

fn strip_shell_output_prefix(message: &str) -> &str {
    let Some(rest) = message.strip_prefix("shell output stream=\"") else {
        return message;
    };
    let Some(index) = rest.find("\" line=") else {
        return message;
    };
    &rest[index + "\" line=".len()..]
}

fn looks_like_timestamp(value: &str) -> bool {
    value.len() >= 20
        && value.starts_with(|character: char| character.is_ascii_digit())
        && value.contains('T')
        && value.ends_with('Z')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> ConfigFile {
        toml::from_str(text).expect("valid config file")
    }

    fn resolve(config: ConfigFile) -> Result<Config, ConfigError> {
        Config::resolve(config, None)
    }

    #[test]
    fn file_values_are_used_and_defaults_fill_the_rest() {
        let config = resolve(file(
            r#"
                addr = "0.0.0.0:9000"
                database_url = "postgres://foundry@db/foundry"
                session_ttl_hours = 24
                bootstrap_password = "file-setup-pass"
                "#,
        ))
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
    fn shell_image_has_a_default_and_can_be_overridden() {
        let config = resolve(file("database_url = \"postgres://foundry@db/foundry\"")).unwrap();
        assert_eq!(config.shell_image, DEFAULT_SHELL_IMAGE);

        let config = resolve(
            file(
                "database_url = \"postgres://foundry@db/foundry\"\nshell_image = \"ghcr.io/acme/shell:1\"",
            ),
        )
        .unwrap();
        assert_eq!(config.shell_image, "ghcr.io/acme/shell:1");
    }

    #[test]
    fn github_app_settings_are_loaded_and_the_key_path_is_checked() {
        let key = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let config = resolve(file(&format!(
            r#"
                database_url = "postgres://foundry@db/foundry"
                github_app_id = 123456
                github_app_private_key_file = "{}"
                github_api_base = "https://git.example.com/api/v3/"
                "#,
            key.display()
        )))
        .unwrap();
        assert_eq!(config.github_app_id, Some(123456));
        assert_eq!(
            config.github_app_private_key_file.as_deref(),
            Some(key.as_path())
        );
        assert_eq!(config.github_api_base, "https://git.example.com/api/v3");
    }

    #[test]
    fn missing_github_app_private_key_is_rejected() {
        let error = resolve(file(
            r#"
                database_url = "postgres://foundry@db/foundry"
                github_app_private_key_file = "/definitely/missing/github-app.pem"
                "#,
        ))
        .unwrap_err();
        assert!(matches!(error, ConfigError::GithubPrivateKey { .. }));
    }

    #[test]
    fn model_settings_come_from_the_file() {
        let config = resolve(file(
            r#"
                database_url = "postgres://foundry@db/foundry"
                openai_api_key = "file-key"
                openai_base_url = "https://api.deepseek.com/v1"
                openai_model = "deepseek-chat"
                "#,
        ))
        .unwrap();
        assert_eq!(config.openai_api_key.as_deref(), Some("file-key"));
        assert_eq!(
            config.openai_base_url.as_deref(),
            Some("https://api.deepseek.com/v1")
        );
        assert_eq!(config.openai_model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn database_url_is_required() {
        assert!(matches!(
            resolve(ConfigFile::default()),
            Err(ConfigError::MissingDatabaseUrl)
        ));
        assert!(matches!(
            resolve(file("database_url = \"\"")),
            Err(ConfigError::MissingDatabaseUrl)
        ));
    }

    #[test]
    fn obsolete_project_seed_keys_are_accepted_but_ignored() {
        let config = resolve(file(
            r#"
                database_url = "postgres://foundry@db/foundry"
                project = "default"
                project_name = "Default"
                "#,
        ))
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
    fn strips_agent_log_prefixes() {
        let log = "2026-11-16T16:31:01.059632Z  INFO foundry_agentd: dispatcher sweep started\n\
2026-11-16T16:31:01.583506Z  INFO foundry_agent_runtime::tools: count=0\n\
2026-11-16T16:31:01.600000Z  INFO foundry_agentd::shell: shell output stream=\"stdout\" line=contents\n\
plain output\n";
        assert_eq!(
            strip_agent_log_prefixes(log),
            "dispatcher sweep started\ncount=0\ncontents\nplain output"
        );
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
