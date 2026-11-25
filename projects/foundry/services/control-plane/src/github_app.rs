//! GitHub App credential broker.
//!
//! The App private key lives only in this process. Agents receive short-lived
//! installation tokens scoped to one repository.

use foundry_shared::Config;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct GitCredential {
    /// HTTPS clone URL without credentials.
    pub url: String,
    /// Username used by the credential helper.
    pub username: String,
    pub token: String,
    pub expires_at: Option<String>,
}

pub struct GitHubApp {
    app_id: u64,
    api_base: String,
    key: EncodingKey,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct Claims {
    iat: u64,
    exp: u64,
    iss: u64,
}

#[derive(Debug, Deserialize)]
struct Installation {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct AccessToken {
    token: String,
    expires_at: Option<String>,
}

impl GitHubApp {
    pub fn from_config(config: &Config) -> Result<Option<Self>, String> {
        let (Some(app_id), Some(path)) = (
            config.github_app_id,
            config.github_app_private_key_file.as_deref(),
        ) else {
            return Ok(None);
        };
        let pem = std::fs::read(path)
            .map_err(|error| format!("read GitHub App private key {}: {error}", path.display()))?;
        let key = EncodingKey::from_rsa_pem(&pem)
            .map_err(|error| format!("parse GitHub App private key: {error}"))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| format!("build GitHub API client: {error}"))?;
        Ok(Some(Self {
            app_id,
            api_base: config.github_api_base.trim_end_matches('/').to_string(),
            key,
            client,
        }))
    }

    /// GitHub's API occasionally drops connections. Retry transport errors
    /// and transient server responses so one network hiccup does not fail an
    /// agent task before it starts.
    async fn send_retry(
        &self,
        request: reqwest::RequestBuilder,
        label: &str,
    ) -> Result<reqwest::Response, String> {
        let mut last = String::new();
        for attempt in 0..3 {
            let Some(request) = request.try_clone() else {
                return Err(format!("{label}: request body cannot be retried"));
            };
            match request.send().await {
                Ok(response)
                    if response.status().is_server_error()
                        || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS =>
                {
                    last = format!("HTTP {}", response.status());
                }
                Ok(response) => return Ok(response),
                Err(error) => last = error.to_string(),
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
            }
        }
        Err(format!("{label}: {last}"))
    }

    pub async fn token_for_repo(&self, repo_url: &str) -> Result<GitCredential, String> {
        let coordinates = repo_coordinates(repo_url)?;
        let jwt = self.jwt()?;
        let request = self
            .client
            .get(format!(
                "{}/repos/{}/{}/installation",
                self.api_base, coordinates.owner, coordinates.repo
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "foundry-control-plane");
        let installation = self
            .send_retry(request, "GitHub installation lookup")
            .await?;
        if !installation.status().is_success() {
            return Err(format!(
                "GitHub App is not installed for {}/{}: {}",
                coordinates.owner,
                coordinates.repo,
                installation.status()
            ));
        }
        let installation: Installation = installation
            .json()
            .await
            .map_err(|error| format!("GitHub installation response: {error}"))?;
        let request = self
            .client
            .post(format!(
                "{}/app/installations/{}/access_tokens",
                self.api_base, installation.id
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "foundry-control-plane")
            .json(&serde_json::json!({ "repositories": [coordinates.repo] }));
        let response = self.send_retry(request, "GitHub access token").await?;
        if !response.status().is_success() {
            return Err(format!(
                "GitHub access token request failed: {}",
                response.status()
            ));
        }
        let token: AccessToken = response
            .json()
            .await
            .map_err(|error| format!("GitHub access token response: {error}"))?;
        Ok(GitCredential {
            url: format!(
                "https://{}/{}/{}.git",
                coordinates.host, coordinates.owner, coordinates.repo
            ),
            username: "x-access-token".to_string(),
            token: token.token,
            expires_at: token.expires_at,
        })
    }

    fn jwt(&self) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs();
        encode(
            &Header::new(Algorithm::RS256),
            &Claims {
                iat: now.saturating_sub(60),
                exp: now + 540,
                iss: self.app_id,
            },
            &self.key,
        )
        .map_err(|error| format!("sign GitHub App JWT: {error}"))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RepoCoordinates {
    host: String,
    owner: String,
    repo: String,
}

fn repo_coordinates(url: &str) -> Result<RepoCoordinates, String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let (host, path) = if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest
            .split_once(':')
            .ok_or_else(|| format!("invalid git URL: {url}"))?;
        (host.to_string(), path.to_string())
    } else if let Some(rest) = url.split_once("://").map(|(_, rest)| rest) {
        let rest = rest.split_once('@').map(|(_, rest)| rest).unwrap_or(rest);
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| format!("invalid git URL: {url}"))?;
        (host.to_string(), path.to_string())
    } else {
        return Err(format!("unsupported git URL: {url}"));
    };
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let owner = parts
        .next()
        .ok_or_else(|| format!("missing owner: {url}"))?;
    let repo = parts
        .next()
        .ok_or_else(|| format!("missing repository: {url}"))?;
    Ok(RepoCoordinates {
        host,
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

#[allow(dead_code)]
fn key_exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_and_ssh_repository_urls() {
        assert_eq!(
            repo_coordinates("https://github.com/xuyan02/software-dev.git").unwrap(),
            RepoCoordinates {
                host: "github.com".into(),
                owner: "xuyan02".into(),
                repo: "software-dev".into(),
            }
        );
        assert_eq!(
            repo_coordinates("git@github.com:xuyan02/software-dev.git").unwrap(),
            RepoCoordinates {
                host: "github.com".into(),
                owner: "xuyan02".into(),
                repo: "software-dev".into(),
            }
        );
        assert_eq!(
            repo_coordinates("https://git.example.com/team/repo").unwrap(),
            RepoCoordinates {
                host: "git.example.com".into(),
                owner: "team".into(),
                repo: "repo".into(),
            }
        );
    }
}
