//! Per-agent SSH credentials for Arche design repositories.
//!
//! Public keys, encrypted private keys and revocation state live in Postgres.
//! The AES-GCM master key stays outside the database in
//! `~/.foundry/arche-key-master`. The host sshd reads only the generated
//! `authorized_keys` through `AuthorizedKeysCommand`; each key is restricted to
//! `foundry-arche-stdio`, which validates the repo against that agent's
//! allowlist.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use sqlx::{PgPool, Row};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::Mutex;
use uuid::Uuid;

const FORCED_COMMAND: &str = "/usr/local/bin/foundry-arche-stdio";
const DEFAULT_HOST_KEY_FILES: &[&str] = &[
    "/etc/ssh/ssh_host_ed25519_key.pub",
    "/etc/ssh/ssh_host_ecdsa_key.pub",
    "/etc/ssh/ssh_host_rsa_key.pub",
];
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone)]
pub struct ArcheSshCredential {
    pub url: String,
    pub private_key: String,
    pub known_hosts: String,
}

pub struct ArcheSsh {
    server_root: PathBuf,
    base: String,
    repos_root: PathBuf,
    pool: PgPool,
    master: [u8; 32],
    lock: Mutex<()>,
}

impl ArcheSsh {
    pub fn new(
        foundry_dir: &Path,
        base: impl Into<String>,
        repos_root: PathBuf,
        pool: PgPool,
    ) -> Result<Self, String> {
        let server_root = foundry_dir.join("arche-sshd");
        ensure_dir(&server_root, 0o755)?;
        ensure_dir(&server_root.join("allowlists"), 0o755)?;
        let master = load_master_key(foundry_dir)?;
        Ok(Self {
            server_root,
            base: base.into().trim_end_matches('/').to_string(),
            repos_root: absolute_path(repos_root),
            pool,
            master,
            lock: Mutex::new(()),
        })
    }

    /// Rewrite the host sshd key file from the database. Called at startup so
    /// keys issued before a deployment change pick up the current forced
    /// command without waiting for the next credential request.
    pub async fn refresh_authorized_keys(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        self.write_authorized_keys().await
    }

    pub async fn credential(
        &self,
        account_id: Uuid,
        repo: &str,
    ) -> Result<ArcheSshCredential, String> {
        let _guard = self.lock.lock().await;
        let private_key = match self.load_key(account_id).await? {
            Some(private_key) => private_key,
            None => {
                let (public_key, private_key) = generate_keypair(account_id)?;
                let fingerprint = fingerprint(&public_key);
                let sealed = self.seal(private_key.as_bytes())?;
                sqlx::query(
                    "insert into agent_arche_keys \
                     (account_id, public_key, private_key_ciphertext, fingerprint) \
                     values ($1, $2, $3, $4) \
                     on conflict (account_id) do update set \
                     public_key = excluded.public_key, \
                     private_key_ciphertext = excluded.private_key_ciphertext, \
                     fingerprint = excluded.fingerprint, \
                     created_at = now(), revoked_at = null",
                )
                .bind(account_id)
                .bind(&public_key)
                .bind(&sealed)
                .bind(&fingerprint)
                .execute(&self.pool)
                .await
                .map_err(|error| format!("store agent SSH key: {error}"))?;
                private_key
            }
        };
        self.write_allowlist(account_id, repo)?;
        self.write_authorized_keys().await?;
        Ok(ArcheSshCredential {
            url: format!("{}/{repo}", self.base),
            private_key,
            known_hosts: self.known_hosts()?,
        })
    }

    pub async fn revoke_account(&self, account_id: Uuid) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let allowlist = self
            .server_root
            .join("allowlists")
            .join(account_id.to_string());
        match fs::remove_file(&allowlist) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove allowlist: {error}")),
        }
        self.write_authorized_keys().await
    }

    async fn load_key(&self, account_id: Uuid) -> Result<Option<String>, String> {
        let row = sqlx::query(
            "select private_key_ciphertext from agent_arche_keys \
             where account_id = $1 and revoked_at is null",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| format!("load agent SSH key: {error}"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let sealed: Vec<u8> = row
            .try_get("private_key_ciphertext")
            .map_err(|error| format!("read agent SSH key: {error}"))?;
        Ok(Some(self.open(&sealed)?))
    }

    async fn write_authorized_keys(&self) -> Result<(), String> {
        let rows = sqlx::query(
            "select account_id, public_key from agent_arche_keys \
             where revoked_at is null order by account_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| format!("list agent SSH keys: {error}"))?;
        let mut text = String::new();
        for row in rows {
            let account_id: Uuid = row
                .try_get("account_id")
                .map_err(|error| format!("read agent account: {error}"))?;
            let public_key: String = row
                .try_get("public_key")
                .map_err(|error| format!("read agent public key: {error}"))?;
            let allowlist = self
                .server_root
                .join("allowlists")
                .join(account_id.to_string());
            let repo = fs::read_to_string(&allowlist).unwrap_or_default();
            let repo = repo.trim();
            if !repo.is_empty() && self.repos_root.join(repo).join(".arche").is_dir() {
                text.push_str(&authorized_keys_line(&public_key, &self.repos_root, repo)?);
            }
        }
        let path = self.server_root.join("authorized_keys");
        write_atomic(&path, text.as_bytes())?;
        set_mode(&path, 0o644)
    }

    fn write_allowlist(&self, account_id: Uuid, repo: &str) -> Result<(), String> {
        let path = self
            .server_root
            .join("allowlists")
            .join(account_id.to_string());
        write_atomic(&path, format!("{repo}\n").as_bytes())?;
        set_mode(&path, 0o644)
    }

    fn known_hosts(&self) -> Result<String, String> {
        let (host, port) = parse_base(&self.base)?;
        let host = if port == 22 {
            host
        } else {
            format!("[{host}]:{port}")
        };
        let mut text = String::new();
        for path in DEFAULT_HOST_KEY_FILES {
            if let Ok(public) = fs::read_to_string(path) {
                text.push_str(&format!("{host} {}\n", public.trim()));
            }
        }
        if text.is_empty() {
            return Err(format!(
                "no host SSH public key found; install OpenSSH server and run the \
                 Arche SSH installer from projects/arche/deploy (looked in {})",
                DEFAULT_HOST_KEY_FILES.join(", ")
            ));
        }
        Ok(text)
    }

    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| "generate encryption nonce".to_string())?;
        let key = aead_key(&self.master)?;
        let mut ciphertext = plaintext.to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| "encrypt agent SSH key".to_string())?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn open(&self, sealed: &[u8]) -> Result<String, String> {
        if sealed.len() <= NONCE_LEN {
            return Err("invalid encrypted agent SSH key".to_string());
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let key = aead_key(&self.master)?;
        let mut plaintext = ciphertext.to_vec();
        let opened = key
            .open_in_place(
                Nonce::try_assume_unique_for_key(nonce)
                    .map_err(|_| "invalid encryption nonce".to_string())?,
                Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| "decrypt agent SSH key".to_string())?;
        String::from_utf8(opened.to_vec()).map_err(|_| "agent SSH key is not UTF-8".to_string())
    }
}

fn aead_key(master: &[u8; 32]) -> Result<LessSafeKey, String> {
    let key = UnboundKey::new(&AES_256_GCM, master)
        .map_err(|_| "invalid AES-GCM master key".to_string())?;
    Ok(LessSafeKey::new(key))
}

fn load_master_key(foundry_dir: &Path) -> Result<[u8; 32], String> {
    let path = foundry_dir.join("arche-key-master");
    if !path.exists() {
        // Preserve keys issued before the Arche rename.
        let legacy = foundry_dir.join("ds-key-master");
        if legacy.is_file() {
            let _ = fs::rename(&legacy, &path);
        }
    }
    if let Ok(bytes) = fs::read(&path) {
        return bytes
            .try_into()
            .map_err(|_| format!("{} must contain exactly 32 bytes", path.display()));
    }
    let mut key = [0u8; 32];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| "generate Arche key master".to_string())?;
    write_atomic(&path, &key)?;
    set_mode(&path, 0o600)?;
    Ok(key)
}

fn generate_keypair(account_id: Uuid) -> Result<(String, String), String> {
    let dir = std::env::temp_dir().join(format!("foundry-arche-key-{}", Uuid::now_v7()));
    ensure_private_dir(&dir)?;
    let key = dir.join("key");
    let result = (|| {
        run_ssh_keygen(&key, &format!("foundry-agent-{account_id}"))?;
        let public = fs::read_to_string(key.with_extension("pub"))
            .map_err(|error| format!("read generated public key: {error}"))?;
        let private = fs::read_to_string(&key)
            .map_err(|error| format!("read generated private key: {error}"))?;
        Ok((public, private))
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn fingerprint(public_key: &str) -> String {
    digest::digest(&digest::SHA256, public_key.trim().as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run_ssh_keygen(path: &Path, comment: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let output = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
        .arg(path)
        .output()
        .map_err(|error| format!("run ssh-keygen: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    set_mode(path, 0o600)?;
    set_mode(&path.with_extension("pub"), 0o644)?;
    Ok(())
}

fn authorized_keys_line(public_key: &str, repos_root: &Path, repo: &str) -> Result<String, String> {
    Ok(format!(
        "restrict,command=\"{} {} {}\" {}\n",
        FORCED_COMMAND,
        shell_arg(repos_root)?,
        shell_arg(Path::new(repo))?,
        public_key.trim()
    ))
}

/// Quote a path for the forced command stored in `authorized_keys`.
///
/// sshd wraps the whole `command=` value in double quotes, so the path cannot
/// contain a double quote or backslash. Single quotes delimit shell arguments,
/// so they are rejected too rather than escaped.
fn shell_arg(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    if text.is_empty()
        || text
            .bytes()
            .any(|byte| matches!(byte, b'\'' | b'"' | b'\\' | b'\n' | b'\r'))
    {
        return Err(format!(
            "path cannot be used in an SSH forced command: {}",
            path.display()
        ));
    }
    Ok(format!("'{text}'"))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "missing parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let temp = parent.join(format!(".{}.tmp", Uuid::now_v7()));
    {
        let mut file = fs::File::create(&temp)
            .map_err(|error| format!("write {}: {error}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("write {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("sync {}: {error}", temp.display()))?;
    }
    fs::rename(&temp, path).map_err(|error| format!("rename {}: {error}", path.display()))?;
    Ok(())
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|directory| directory.join(&path))
        .unwrap_or(path)
}

fn ensure_dir(path: &Path, mode: u32) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    set_mode(path, mode)
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    ensure_dir(path, 0o700)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("chmod {}: {error}", path.display()))?;
    }
    let _ = mode;
    Ok(())
}

fn parse_base(base: &str) -> Result<(String, u16), String> {
    let rest = base
        .strip_prefix("ssh://")
        .ok_or_else(|| format!("arche_ssh_url must start with ssh://: {base}"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (
            host.to_owned(),
            port.parse::<u16>()
                .map_err(|error| format!("invalid SSH port: {error}"))?,
        ),
        None => (host_port.to_owned(), 22),
    };
    if host.is_empty() {
        return Err("arche_ssh_url has an empty host".to_string());
    }
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_private_keys() {
        let master = [7u8; 32];
        let key = aead_key(&master).unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).unwrap();
        let mut ciphertext = b"private key".to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut ciphertext,
        )
        .unwrap();
        let mut sealed = nonce.to_vec();
        sealed.extend_from_slice(&ciphertext);
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let key = aead_key(&master).unwrap();
        let mut plaintext = ciphertext.to_vec();
        let opened = key
            .open_in_place(
                Nonce::try_assume_unique_for_key(nonce).unwrap(),
                Aad::empty(),
                &mut plaintext,
            )
            .unwrap();
        assert_eq!(opened, b"private key");
    }

    #[test]
    fn parses_ssh_base_with_a_custom_port() {
        assert_eq!(
            parse_base("ssh://foundry-arche@127.0.0.1:2222").unwrap(),
            ("127.0.0.1".to_string(), 2222)
        );
    }

    #[test]
    fn parses_ssh_base_with_the_default_port() {
        assert_eq!(
            parse_base("ssh://foundry-arche@127.0.0.1").unwrap(),
            ("127.0.0.1".to_string(), 22)
        );
    }

    #[test]
    fn builds_a_forced_command_with_the_host_paths() {
        let line = authorized_keys_line(
            "ssh-ed25519 AAAA agent\n",
            Path::new("/home/foundry/.foundry/design-spaces"),
            "project-a",
        )
        .unwrap();
        assert_eq!(
            line,
            "restrict,command=\"/usr/local/bin/foundry-arche-stdio \
             '/home/foundry/.foundry/design-spaces' 'project-a'\" \
             ssh-ed25519 AAAA agent\n"
        );
    }

    #[test]
    fn rejects_paths_that_cannot_be_quoted_for_ssh() {
        assert!(shell_arg(Path::new("/tmp/has space")).is_ok());
        assert!(shell_arg(Path::new("/tmp/has'quote")).is_err());
        assert!(shell_arg(Path::new("/tmp/has\"quote")).is_err());
        assert!(shell_arg(Path::new("")).is_err());
    }
}
