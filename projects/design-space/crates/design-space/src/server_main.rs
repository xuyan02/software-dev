use std::path::PathBuf;

use clap::{Parser, Subcommand};
use design_space::error::{Error, Result};
use design_space::gc::{self, GcReport, RepackReport};
use design_space::project;
use design_space::remote;
use design_space::server::{self, ServerConfig};
use design_space::store::Store;

#[derive(Parser)]
#[command(name = "ds-server", version, about = "Standalone design-space server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve projects over HTTP (multi-project API + sync endpoints).
    Serve {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        listen: String,
        /// Host used in generated ds:// addresses.
        #[arg(long)]
        host: Option<String>,
        /// SSH user used in generated ds:// addresses.
        #[arg(long)]
        user: Option<String>,
        /// SSH port used in generated ds:// addresses.
        #[arg(long)]
        port: Option<u16>,
    },
    /// Speak the design-space protocol on stdin/stdout (for sshd).
    Stdio {
        /// Project repository directory (legacy).
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Project root; used with --project.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Project id from its ds:// address; resolved under the root.
        #[arg(long)]
        project: Option<String>,
    },
    /// Create, list or show projects.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Prune unreachable nodes in one project repository.
    Gc {
        #[arg(long)]
        repo: PathBuf,
    },
    /// Repack one project repository without pruning.
    Repack {
        #[arg(long)]
        repo: PathBuf,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    Create {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        name: String,
        /// Source git repository clients should clone (optional).
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    List {
        #[arg(long)]
        root: PathBuf,
    },
    Show {
        name: String,
        #[arg(long)]
        root: PathBuf,
    },
    /// Record the source repository advertised to `ds clone`.
    SetSource {
        name: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        root: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            root,
            listen,
            host,
            user,
            port,
        } => {
            let config = ServerConfig {
                root,
                host,
                user,
                port,
                token: std::env::var("DS_TOKEN")
                    .ok()
                    .filter(|token| !token.is_empty()),
            };
            eprintln!("serving projects on http {listen}");
            server::serve(config, &listen)
        }
        Command::Stdio {
            repo,
            root,
            project,
        } => {
            let project = project.or_else(|| {
                if repo.is_none() {
                    // sshd ForceCommand hides the client's command line but
                    // keeps it in SSH_ORIGINAL_COMMAND. Accept only a project
                    // id from it: paths and the root stay server-controlled.
                    original_stdio_project()
                } else {
                    None
                }
            });
            if let Some(repo) = repo {
                return remote::serve_stdio(&repo);
            }
            let Some(project_id) = project else {
                return Err(Error::Config(
                    "stdio requires --repo <path> or --project <id>".into(),
                ));
            };
            let root = match root {
                Some(root) => root,
                None => default_root()?,
            };
            remote::serve_project_stdio(&root, &project_id)
        }
        Command::Project { command } => project_command(command),
        Command::Gc { repo } => {
            let store = Store::new(&repo);
            let report = gc::gc(&store, &repo)?;
            print_gc(&report);
            Ok(())
        }
        Command::Repack { repo } => {
            let store = Store::new(&repo);
            let report = gc::repack(&store, &repo)?;
            print_repack(&report);
            Ok(())
        }
    }
}

fn default_root() -> Result<PathBuf> {
    match std::env::var("DESIGN_SPACE_ROOT") {
        Ok(root) if !root.trim().is_empty() => return Ok(PathBuf::from(root)),
        _ => {}
    }
    let home =
        std::env::var("HOME").map_err(|_| Error::Config("HOME is not set; pass --root".into()))?;
    Ok(PathBuf::from(home).join(".foundry").join("design-spaces"))
}

fn original_stdio_project() -> Option<String> {
    let command = std::env::var("SSH_ORIGINAL_COMMAND").ok()?;
    parse_project_arg(&command)
}

fn parse_project_arg(command: &str) -> Option<String> {
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "--project" {
            return tokens.next().map(|value| unquote(value).to_owned());
        }
    }
    None
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(value)
}

fn project_command(command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::Create {
            root,
            name,
            source,
            host,
            user,
            port,
        } => {
            let host = host
                .or_else(|| std::env::var("DS_HOST").ok())
                .or_else(|| std::env::var("HOSTNAME").ok())
                .filter(|host| !host.is_empty())
                .unwrap_or_else(|| "localhost".to_owned());
            let user = user.or_else(|| std::env::var("USER").ok());
            let project = project::create(
                &root,
                &name,
                source.as_deref(),
                &host,
                user.as_deref(),
                port,
            )?;
            println!("{}", server::describe(&project));
            Ok(())
        }
        ProjectCommand::List { root } => {
            for project in project::list(&root)? {
                println!("{}  {}", project.meta.name, project.meta.design_space);
            }
            Ok(())
        }
        ProjectCommand::Show { name, root } => {
            let project = project::open(&root, &name)?;
            println!("{}", server::describe(&project));
            Ok(())
        }
        ProjectCommand::SetSource { name, source, root } => {
            let project = project::set_source(&root, &name, &source)?;
            println!("{}", server::describe(&project));
            Ok(())
        }
    }
}

fn print_gc(report: &GcReport) {
    println!("Collected {} reachable node(s)", report.reachable);
    match &report.pack {
        Some(checksum) => println!(
            "Wrote pack {} ({} node(s))",
            short(checksum),
            report.pack_nodes
        ),
        None => println!("No reachable nodes; no pack written"),
    }
    println!(
        "Removed {} pack(s), {} loose node(s), {} stale cache file(s)",
        report.removed_packs, report.removed_loose, report.removed_cache
    );
}

fn print_repack(report: &RepackReport) {
    match &report.pack {
        Some(checksum) => println!("Packed {} node(s) into {}", report.nodes, short(checksum)),
        None => println!("No nodes to pack"),
    }
    println!(
        "Removed {} pack(s), {} loose node(s)",
        report.removed_packs, report.removed_loose
    );
}

fn short(text: &str) -> &str {
    &text[..text.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_from_ssh_original_command() {
        assert_eq!(
            parse_project_arg("ds-server stdio --project '01a1db53c0ffee42'"),
            Some("01a1db53c0ffee42".to_owned())
        );
        assert_eq!(
            parse_project_arg("ds-server stdio --root /srv --project abc --repo '/srv/x'"),
            Some("abc".to_owned())
        );
        // Paths in the client command are ignored; the server keeps its root.
        assert_eq!(parse_project_arg("ds-server stdio --repo /srv/x"), None);
        assert_eq!(parse_project_arg("something else"), None);
    }
}
