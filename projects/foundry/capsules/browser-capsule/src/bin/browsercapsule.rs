use anyhow::{Context, Result};
use browser_capsule::{BrowserCapsule, Input};
use clap::{Parser, Subcommand};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "browsercapsule", about = "Try the browser-capsule library")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a capsule, navigate, screenshot/input, then close.
    Demo {
        #[arg(long, default_value = "https://example.com")]
        url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Demo { url } => {
            let started = Instant::now();
            let capsule = BrowserCapsule::create().await?;
            println!(
                "created in {:.2} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );

            capsule.navigate(&url).await?;
            println!("navigated to {url}");

            let before = "/tmp/browser-capsule-before.png";
            let shot = capsule.screenshot().await?;
            std::fs::write(before, &shot).context("write before screenshot")?;
            println!("before: {before} ({} bytes)", shot.len());

            capsule
                .input(Input::Click {
                    x: 640,
                    y: 400,
                    button: 1,
                })
                .await?;
            capsule
                .input(Input::Text {
                    text: "hello from browser-capsule".into(),
                })
                .await?;
            capsule
                .input(Input::Key {
                    key: "Return".into(),
                })
                .await?;
            println!("sent click + text + Return");

            let after = "/tmp/browser-capsule-after.png";
            let shot = capsule.screenshot().await?;
            std::fs::write(after, &shot).context("write after screenshot")?;
            println!("after:  {after} ({} bytes)", shot.len());

            capsule.close().await?;
            println!("browser capsule closed and removed");
        }
    }
    Ok(())
}
