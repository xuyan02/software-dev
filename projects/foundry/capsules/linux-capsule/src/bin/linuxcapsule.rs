use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use linux_capsule::{Input, LinuxCapsule};
use std::path::Path;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "linuxcapsule", about = "Try the linux-capsule library")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Desktop,
    Headless,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a capsule and exercise UI, exec and file transfer.
    Demo {
        #[arg(long, value_enum, default_value = "desktop")]
        mode: Mode,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Demo { mode } => {
            let started = Instant::now();
            let capsule = match mode {
                Mode::Desktop => LinuxCapsule::desktop().await?,
                Mode::Headless => LinuxCapsule::headless().await?,
            };
            println!(
                "created in {:.2} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );

            if matches!(mode, Mode::Desktop) {
                let before = "/tmp/linux-capsule-before.png";
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
                        text: "hello from linux-capsule".into(),
                    })
                    .await?;
                capsule
                    .input(Input::Key {
                        key: "Return".into(),
                    })
                    .await?;
                println!("sent click + text + Return");

                let after = "/tmp/linux-capsule-after.png";
                let shot = capsule.screenshot().await?;
                std::fs::write(after, &shot).context("write after screenshot")?;
                println!("after:  {after} ({} bytes)", shot.len());
            } else {
                println!("headless mode: skipping screenshot/input");
            }

            std::fs::write("/tmp/linux-capsule-upload.txt", "hello from the host\n")
                .context("write local upload file")?;
            capsule
                .upload(
                    Path::new("/tmp/linux-capsule-upload.txt"),
                    "/tmp/linux-capsule-upload.txt",
                )
                .await?;
            let output = capsule.exec("cat /tmp/linux-capsule-upload.txt").await?;
            println!("uploaded file contents: {}", output.stdout_string().trim());

            capsule
                .exec("echo written-in-capsule > /tmp/linux-capsule-download.txt")
                .await?;
            capsule
                .download(
                    "/tmp/linux-capsule-download.txt",
                    Path::new("/tmp/linux-capsule-download.txt"),
                )
                .await?;
            println!(
                "downloaded file contents: {}",
                std::fs::read_to_string("/tmp/linux-capsule-download.txt")
                    .context("read downloaded file")?
                    .trim()
            );

            let output = capsule.exec("uname -a").await?;
            println!("exec uname -a: {}", output.stdout_string().trim());

            capsule.close().await?;
            println!("capsule closed and removed");
        }
    }
    Ok(())
}
