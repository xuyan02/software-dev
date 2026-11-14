//! Language adapters over tree-sitter.
//!
//! Each adapter lives in its own module and returns the same [`common::Entity`]
//! tree. Shared naming, flattening and root-prefix logic lives in
//! [`common`].
//!
//! Extraction is deliberately syntactic: preprocessor directives and macros
//! are kept as written, never expanded.

mod common;
mod cpp;
mod go;
mod java;
mod python;
mod rust;

use std::collections::BTreeSet;

use tree_sitter::Parser;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::source::SourceRepo;

/// Blobs larger than this are skipped by `scan_commit`.
pub const MAX_BLOB_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Cpp,
    Rust,
    Python,
    Go,
    Java,
}

pub fn detect(path: &str) -> Option<Language> {
    let extension = path.rsplit('.').next().unwrap_or("");
    match extension {
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h" | "h++" | "inl" => {
            Some(Language::Cpp)
        }
        "rs" => Some(Language::Rust),
        "py" | "pyi" => Some(Language::Python),
        "go" => Some(Language::Go),
        "java" => Some(Language::Java),
        _ => None,
    }
}

pub fn extract_file(
    language: Language,
    path: &str,
    commit_hex: &str,
    source: &str,
) -> Result<Vec<Element>> {
    let mut parser = Parser::new();
    let ts_language: tree_sitter::Language = match language {
        Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
    };
    parser
        .set_language(&ts_language)
        .map_err(|error| Error::Config(format!("tree-sitter: {error}")))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| Error::Config("tree-sitter returned no tree".into()))?;
    let root = tree.root_node();

    let entities = match language {
        Language::Cpp => cpp::collect(root, source),
        Language::Rust => rust::collect(root, source),
        Language::Python => python::collect(root, source),
        Language::Go => go::collect(root, source),
        Language::Java => java::collect(root, source),
    };

    let prefix = match language {
        Language::Cpp | Language::Java => None,
        Language::Rust => rust::prefix(path),
        Language::Python => python::prefix(path),
        Language::Go => go::prefix(root, source, path),
    };

    Ok(common::finish(
        entities,
        path,
        commit_hex,
        source,
        prefix.as_deref(),
    ))
}

/// Result of scanning one commit.
///
/// Only language-level elements enter the design space. Files without an
/// adapter, files the adapter finds nothing in, and blobs that are too large,
/// binary or not UTF-8 are skipped and reported here.
#[derive(Debug, Default)]
pub struct ScanReport {
    pub elements: Vec<Element>,
    /// Files that produced at least one element.
    pub extracted_files: usize,
    /// Source files whose extension has no adapter.
    pub unsupported: Vec<String>,
    /// Source files whose adapter produced no element.
    pub empty: Vec<String>,
    /// Blobs skipped as too large, non-UTF-8 or containing NUL.
    pub skipped_blobs: Vec<String>,
}

impl ScanReport {
    pub fn skipped_files(&self) -> usize {
        self.unsupported.len() + self.empty.len() + self.skipped_blobs.len()
    }
}

/// Read every blob in a commit and extract elements from it.
///
/// No work tree is created: blobs are read straight from the git object store,
/// so filters and smudge rules cannot change the described bytes.
pub fn scan_commit(source: &SourceRepo, commit_oid: &str) -> Result<ScanReport> {
    scan_commit_with(source, commit_oid, &mut |_, _| {})
}

/// Like [`scan_commit`], but reports each processed path and its element count.
pub fn scan_commit_with(
    source: &SourceRepo,
    commit_oid: &str,
    progress: &mut dyn FnMut(&str, usize),
) -> Result<ScanReport> {
    scan_commit_selected(source, commit_oid, None, progress)
}

/// Scan only the given paths in a commit.
pub fn scan_commit_paths(
    source: &SourceRepo,
    commit_oid: &str,
    paths: &BTreeSet<String>,
    progress: &mut dyn FnMut(&str, usize),
) -> Result<ScanReport> {
    scan_commit_selected(source, commit_oid, Some(paths), progress)
}

fn scan_commit_selected(
    source: &SourceRepo,
    commit_oid: &str,
    selected: Option<&BTreeSet<String>>,
    progress: &mut dyn FnMut(&str, usize),
) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    for entry in source.list_blobs(commit_oid)? {
        if let Some(paths) = selected
            && !paths.contains(&entry.path)
        {
            continue;
        }
        if source.blob_size(&entry.oid)? > MAX_BLOB_BYTES {
            report.skipped_blobs.push(entry.path);
            continue;
        }
        let Ok(bytes) = source.read_blob(&entry.oid) else {
            report.skipped_blobs.push(entry.path);
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            report.skipped_blobs.push(entry.path);
            continue;
        };
        if text.as_bytes().contains(&0) {
            report.skipped_blobs.push(entry.path);
            continue;
        }

        let Some(language) = detect(&entry.path) else {
            report.unsupported.push(entry.path);
            continue;
        };
        let extracted = extract_file(language, &entry.path, commit_oid, text).unwrap_or_default();
        if extracted.is_empty() {
            report.empty.push(entry.path);
            continue;
        }

        let count = extracted.len();
        report.extracted_files += 1;
        report.elements.extend(extracted);
        progress(&entry.path, count);
    }
    report.elements.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_languages_by_extension() {
        assert_eq!(detect("a.cpp"), Some(Language::Cpp));
        assert_eq!(detect("a.hpp"), Some(Language::Cpp));
        assert_eq!(detect("a.rs"), Some(Language::Rust));
        assert_eq!(detect("a.py"), Some(Language::Python));
        assert_eq!(detect("a.go"), Some(Language::Go));
        assert_eq!(detect("a.java"), Some(Language::Java));
        assert_eq!(detect("a.md"), None);
    }
}
