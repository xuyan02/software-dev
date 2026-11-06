//! Object storage adapter (planned).
//!
//! Stores large evidence: test reports, screenshots, traces, coverage
//! bundles, SBOMs, benchmark output. Metadata lives in Postgres; bytes live in
//! S3-compatible storage. Evidence is content-addressed by SHA-256 so it can be
//! referenced from a `Decision`, a transition, or a release record without
//! copying it into the event log.
