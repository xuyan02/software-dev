use android_capsule::AndroidCapsule;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "androidcapsule", about = "Try the android-capsule library")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create an emulator, screenshot, launch an app, screenshot, then close.
    Demo {
        #[arg(long, default_value = "com.android.settings")]
        package: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Demo { package } => {
            let started = Instant::now();
            let capsule = AndroidCapsule::create().await?;
            println!(
                "created in {:.2} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );

            let before = "/tmp/android-capsule-before.png";
            let shot = capsule.screenshot().await?;
            std::fs::write(before, &shot).context("write before screenshot")?;
            println!("before: {before} ({} bytes)", shot.len());

            capsule.launch(&package).await?;
            println!("launched {package}");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let after = "/tmp/android-capsule-after.png";
            let shot = capsule.screenshot().await?;
            std::fs::write(after, &shot).context("write after screenshot")?;
            println!("after:  {after} ({} bytes)", shot.len());

            capsule.close().await?;
            println!("android capsule closed and removed");
        }
    }
    Ok(())
}
