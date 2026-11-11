//! `foundry` CLI: a thin client over the control-plane HTTP API.
//!
//! The CLI is what a human uses in a terminal; it holds no business logic.
//! Anything it can do, curl or an agent can do through the same endpoints.
//!
//! Authentication uses a bearer token from `foundry login` (or
//! `FOUNDRY_TOKEN`). The target project comes from `--project` /
//! `FOUNDRY_PROJECT` and is sent as `X-Foundry-Project`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Parser)]
#[command(name = "foundry", about = "Foundry 软件工程控制平面 CLI")]
struct Cli {
    /// Control plane base URL.
    #[arg(long, env = "FOUNDRY_URL", default_value = "http://127.0.0.1:8080")]
    url: String,

    /// Login session token from `foundry login`.
    #[arg(long, env = "FOUNDRY_TOKEN")]
    token: Option<String>,

    /// Project UUID. Defaults to the first project visible to the account.
    #[arg(long, env = "FOUNDRY_PROJECT")]
    project: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Exchange a handle and password for a session token.
    Login { handle: String, password: String },
    /// List projects visible to the current account.
    Projects,
    /// List accounts (admin only).
    Accounts,
    /// Work with issues: the problem space.
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
    /// Work with workflow definitions.
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    /// Create a project rooted in a published workflow.
    CreateProject {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// Id of a published workflow to use for the root task.
        #[arg(long = "root-workflow", default_value = "project")]
        root_workflow: String,
    },
    /// Permanently delete a project and all data scoped to it.
    DeleteProject { id: String },
}

#[derive(Debug, Subcommand)]
enum WorkflowCommand {
    /// List workflow definitions visible to the account.
    List,
    /// Show one definition, or one published version with --version.
    Show {
        id: String,
        #[arg(long)]
        version: Option<u32>,
    },
    /// Publish the shared draft at the given revision.
    Publish {
        id: String,
        /// Current draft revision, as reported by `workflow show`.
        #[arg(long)]
        revision: String,
    },
}

#[derive(Debug, Subcommand)]
enum IssueCommand {
    /// List issues.
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Show one issue as JSON.
    Get { id: String },
    /// Create an issue from a JSON file.
    Create { file: String },
    /// Add a comment to an issue.
    Comment {
        id: String,
        body: String,
        #[arg(long)]
        internal: bool,
    },
    /// Request an issue state transition.
    Transition {
        id: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        resolution: Option<String>,
        #[arg(long)]
        duplicate_of: Option<String>,
        #[arg(long)]
        version: Option<u32>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        url,
        token,
        project,
        command,
    } = Cli::parse();
    let auth = Auth {
        token: token.as_deref(),
        project: project.as_deref(),
    };

    match command {
        Command::Login { handle, password } => {
            let body = json!({ "handle": handle, "password": password });
            print_response(
                &url,
                &Auth {
                    token: None,
                    project: None,
                },
                "POST",
                "/api/v1/auth/login",
                Some(body.to_string()),
            )
            .await?;
        }
        Command::Projects => {
            let auth = auth.require()?;
            print_response(&url, auth, "GET", "/api/v1/projects", None).await?;
        }
        Command::Accounts => {
            let auth = auth.require()?;
            print_response(&url, auth, "GET", "/api/v1/accounts", None).await?;
        }
        Command::Issue { command } => match command {
            IssueCommand::List {
                status,
                kind,
                assignee,
                text,
            } => {
                let auth = auth.require()?;
                let mut query = Vec::new();
                if let Some(status) = status {
                    query.push(format!("status={}", encode(&status)));
                }
                if let Some(kind) = kind {
                    query.push(format!("kind={}", encode(&kind)));
                }
                if let Some(assignee) = assignee {
                    query.push(format!("assignee={}", encode(&assignee)));
                }
                if let Some(text) = text {
                    query.push(format!("text={}", encode(&text)));
                }
                let path = if query.is_empty() {
                    "/api/v1/issues".to_string()
                } else {
                    format!("/api/v1/issues?{}", query.join("&"))
                };
                print_response(&url, auth, "GET", &path, None).await?;
            }
            IssueCommand::Get { id } => {
                let auth = auth.require()?;
                print_response(&url, auth, "GET", &format!("/api/v1/issues/{id}"), None).await?;
            }
            IssueCommand::Create { file } => {
                let auth = auth.require()?;
                let body = read_json(&file)?;
                print_response(&url, auth, "POST", "/api/v1/issues", Some(body)).await?;
            }
            IssueCommand::Comment { id, body, internal } => {
                let auth = auth.require()?;
                let payload = json!({ "body": body, "internal": internal });
                print_response(
                    &url,
                    auth,
                    "POST",
                    &format!("/api/v1/issues/{id}/comments"),
                    Some(payload.to_string()),
                )
                .await?;
            }
            IssueCommand::Transition {
                id,
                to,
                resolution,
                duplicate_of,
                version,
            } => {
                let auth = auth.require()?;
                let payload = json!({
                    "to": to,
                    "resolution": resolution,
                    "duplicate_of": duplicate_of,
                    "expected_version": version,
                });
                print_response(
                    &url,
                    auth,
                    "POST",
                    &format!("/api/v1/issues/{id}/transitions"),
                    Some(payload.to_string()),
                )
                .await?;
            }
        },
        Command::Workflow { command } => match command {
            WorkflowCommand::List => {
                let auth = auth.require()?;
                print_response(&url, auth, "GET", "/api/v1/workflows", None).await?;
            }
            WorkflowCommand::Show { id, version } => {
                let auth = auth.require()?;
                let path = match version {
                    Some(version) => format!("/api/v1/workflows/{id}/versions/{version}"),
                    None => format!("/api/v1/workflows/{id}"),
                };
                print_response(&url, auth, "GET", &path, None).await?;
            }
            WorkflowCommand::Publish { id, revision } => {
                let auth = auth.require()?;
                let if_match = format!("\"{revision}\"");
                print_response_with(
                    &url,
                    auth,
                    "POST",
                    &format!("/api/v1/workflows/{id}/publish"),
                    Some("{}".to_string()),
                    &[("If-Match", if_match.as_str())],
                )
                .await?;
            }
        },
        Command::CreateProject {
            name,
            description,
            root_workflow,
        } => {
            let auth = auth.require()?;
            let body = json!({
                "name": name,
                "description": description,
                "root_workflow_id": root_workflow,
            });
            print_response(
                &url,
                auth,
                "POST",
                "/api/v1/projects",
                Some(body.to_string()),
            )
            .await?;
        }
        Command::DeleteProject { id } => {
            let auth = auth.require()?;
            print_response(
                &url,
                auth,
                "DELETE",
                &format!("/api/v1/projects/{id}"),
                None,
            )
            .await?;
        }
    }
    Ok(())
}

struct Auth<'a> {
    token: Option<&'a str>,
    project: Option<&'a str>,
}

impl Auth<'_> {
    fn require(&self) -> Result<&Self> {
        if self.token.is_some() {
            Ok(self)
        } else {
            bail!("login required: run `foundry login <handle> <password>` and set FOUNDRY_TOKEN")
        }
    }
}

fn read_json(file: &str) -> Result<String> {
    let body = std::fs::read_to_string(file).with_context(|| format!("failed to read {file}"))?;
    let _: Value = serde_json::from_str(&body).context("invalid JSON request body")?;
    Ok(body)
}

async fn print_response(
    base: &str,
    auth: &Auth<'_>,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<()> {
    print_response_with(base, auth, method, path, body, &[]).await
}

async fn print_response_with(
    base: &str,
    auth: &Auth<'_>,
    method: &str,
    path: &str,
    body: Option<String>,
    headers: &[(&str, &str)],
) -> Result<()> {
    let (status, response) = request_with(base, auth, method, path, body, headers).await?;
    println!("{response}");
    if !(200..300).contains(&status) {
        bail!("request failed with HTTP {status}");
    }
    Ok(())
}

async fn request_with(
    base: &str,
    auth: &Auth<'_>,
    method: &str,
    path: &str,
    body: Option<String>,
    headers: &[(&str, &str)],
) -> Result<(u16, String)> {
    let (host, port) = parse_base(base)?;
    let mut stream = TcpStream::connect((host.as_str(), port))
        .await
        .with_context(|| format!("cannot connect to {base}"))?;

    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(token) = auth.token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(project) = auth.project {
        request.push_str(&format!("X-Foundry-Project: {project}\r\n"));
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = &body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    if let Some(body) = body {
        request.push_str(&body);
    }

    stream.write_all(request.as_bytes()).await?;
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    let text = String::from_utf8_lossy(&buffer).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    Ok((status, body.to_string()))
}

fn parse_base(base: &str) -> Result<(String, u16)> {
    let Some(rest) = base.strip_prefix("http://") else {
        bail!("only http:// URLs are supported (got {base:?})");
    };
    let rest = rest.trim_end_matches('/');
    match rest.split_once(':') {
        Some((host, port)) => Ok((host.to_string(), port.parse().context("invalid port")?)),
        None => Ok((rest.to_string(), 80)),
    }
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_base() {
        assert_eq!(parse_base("http://localhost:8080").unwrap().1, 8080);
        assert_eq!(parse_base("http://localhost").unwrap().1, 80);
        assert!(parse_base("https://localhost").is_err());
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(
            encode("部分 退款"),
            "%E9%83%A8%E5%88%86%20%E9%80%80%E6%AC%BE"
        );
    }
}
