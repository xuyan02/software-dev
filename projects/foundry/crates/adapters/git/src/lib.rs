//! Git adapter (planned).
//!
//! Foundry stores long-form content — requirements, design docs, ADRs — as
//! Markdown with YAML front matter in a project repository. Git remains the
//! source of truth for text; this adapter will:
//!
//! * read documents from a bare repository or working tree;
//! * write documents on a branch and open a change request;
//! * map commits to `Event` records so the console can show provenance.
//!
//! No implementation yet. See `docs/adr/0002-git-content-store.md`.
