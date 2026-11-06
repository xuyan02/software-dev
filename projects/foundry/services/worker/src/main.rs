//! Foundry worker.
//!
//! This process will claim tasks from the control plane, run them inside an
//! isolated sandbox, and report evidence back. It is deliberately a placeholder
//! in the first slice: no job queue exists yet, and printing a heartbeat is
//! enough to validate the workspace and deployment shape.
//!
//! The contract it will implement:
//!
//! 1. `claim(task_id)` with a lease and idempotency key;
//! 2. run the task in a disposable container with no ambient credentials;
//! 3. upload evidence and `complete(task_id, evidence)`;
//! 4. heartbeat the lease, and let the control plane requeue expired leases.

use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_tracing("foundry_worker=info");
    tracing::info!("foundry worker started (no job backend configured)");

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = interval.tick() => tracing::debug!("worker heartbeat"),
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("worker shutting down");
                break;
            }
        }
    }
    Ok(())
}
