//! Workspace tasks. Run with `cargo xtask <command>`.

use std::process::Command;

fn main() {
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_string());
    match task.as_str() {
        "fmt" => run(&["fmt", "--all"]),
        "lint" => run(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
        "test" => run(&["test", "--workspace"]),
        "check" => {
            run(&["fmt", "--all", "--check"]);
            run(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ]);
            run(&["test", "--workspace"]);
        }
        other => {
            eprintln!("unknown task {other:?}; expected fmt, lint, test or check");
            std::process::exit(2);
        }
    }
}

fn run(args: &[&str]) {
    println!("> cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .status()
        .unwrap_or_else(|error| {
            eprintln!("failed to run cargo: {error}");
            std::process::exit(1);
        });
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
