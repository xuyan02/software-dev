use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn ds(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ds"))
        .arg("--dir")
        .arg(root)
        .args(args)
        .output()
        .expect("run ds")
}

fn ok(root: &Path, args: &[&str]) -> String {
    let output = ds(root, args);
    assert!(
        output.status.success(),
        "ds {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

#[test]
fn staging_diff_reset_branches_and_pack() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "Base",
            "--namespace",
            "namespaces/app",
            "--design",
            "base",
        ],
    );
    ok(
        root,
        &[
            "create",
            "method",
            "foo",
            "--parent",
            "namespaces/app/Base",
            "--signature",
            "foo()",
        ],
    );
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-m", "initial"]);

    let listed = ok(root, &["ls"]);
    assert!(listed.contains("namespaces/app/Base"));
    assert!(listed.contains("Base/foo#"));

    ok(root, &["edit", "namespaces/app/Base", "--design", "base2"]);
    let diff = ok(root, &["diff"]);
    assert!(diff.contains("M namespaces/app/Base"));
    assert!(diff.contains("design:"));
    ok(root, &["add", "namespaces/app/Base"]);
    assert!(ok(root, &["diff", "--staged"]).contains("M namespaces/app/Base"));
    ok(root, &["reset", "namespaces/app/Base"]);
    assert!(ok(root, &["diff", "--staged"]).contains("no changes"));
    ok(root, &["reset", "--hard"]);
    assert!(ok(root, &["diff"]).contains("no changes"));

    ok(root, &["checkout", "-b", "feature"]);
    ok(
        root,
        &[
            "create",
            "class",
            "Feature",
            "--namespace",
            "namespaces/app",
        ],
    );
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-m", "feature"]);
    ok(root, &["checkout", "main"]);
    assert!(!ok(root, &["ls"]).contains("Feature"));
    ok(root, &["checkout", "feature"]);
    assert!(ok(root, &["ls"]).contains("Feature"));

    ok(root, &["repack"]);
    assert!(ok(root, &["show", "namespaces/app/Feature"]).contains("kind: Class"));
    ok(root, &["gc"]);
    assert!(ok(root, &["show", "namespaces/app/Feature"]).contains("kind: Class"));
}

#[test]
fn clones_a_remote_branch() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let clone = dir.path().join("clone");
    let remote = dir.path().join("remote");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&remote).unwrap();

    ok(&source, &["init", "--source", "file:///nonexistent"]);
    ok(
        &source,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        &source,
        &["create", "class", "Auth", "--namespace", "namespaces/app"],
    );
    ok(&source, &["add", "-A"]);
    ok(&source, &["commit", "-m", "initial"]);
    ok(&source, &["remote", "add", remote.to_str().unwrap()]);
    ok(&source, &["push"]);

    ok(
        &clone,
        &[
            "clone",
            remote.to_str().unwrap(),
            "--source",
            "file:///nonexistent",
            "--branch",
            "main",
        ],
    );
    let listed = ok(&clone, &["ls"]);
    assert!(listed.contains("namespaces/app/Auth"));
    assert!(ok(&clone, &["show", "namespaces/app/Auth"]).contains("kind: Class"));
}

#[test]
fn renames_and_reparents_children_and_references() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "namespace", "lib", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "Base", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "Derived",
            "--namespace",
            "namespaces/app",
            "--superclass",
            "namespaces/app/Base",
        ],
    );
    ok(
        root,
        &[
            "create",
            "method",
            "login",
            "--parent",
            "namespaces/app/Base",
            "--signature",
            "login()",
        ],
    );

    ok(
        root,
        &[
            "edit",
            "namespaces/app/Base",
            "--namespace",
            "namespaces/lib",
        ],
    );
    let listed = ok(root, &["ls"]);
    assert!(listed.contains("namespaces/lib/Base"));
    assert!(listed.contains("namespaces/lib/Base/login#"));
    assert!(!listed.contains("namespaces/app/Base/"));
    assert!(ok(root, &["show", "namespaces/app/Derived"]).contains("namespaces/lib/Base"));

    ok(root, &["edit", "namespaces/lib/Base", "--name", "Base2"]);
    let listed = ok(root, &["ls"]);
    assert!(listed.contains("namespaces/lib/Base2"));
    assert!(listed.contains("namespaces/lib/Base2/login#"));
    assert!(ok(root, &["show", "namespaces/app/Derived"]).contains("namespaces/lib/Base2"));
}

#[test]
fn rejects_invalid_relations_and_unsafe_removals() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "A", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["create", "class", "B", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "create",
            "method",
            "foo",
            "--parent",
            "namespaces/app/A",
            "--signature",
            "foo()",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/A",
            "--superclass",
            "namespaces/app/B",
        ],
    );
    let output = ds(
        root,
        &[
            "edit",
            "namespaces/app/B",
            "--superclass",
            "namespaces/app/A",
        ],
    );
    assert!(output.status.success());
    let output = ds(root, &["verify"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cycle"));

    ok(root, &["edit", "namespaces/app/B", "--superclass", ""]);

    let method = ok(root, &["ls"])
        .lines()
        .find(|line| line.contains("A/foo#"))
        .unwrap()
        .to_owned();
    ok(
        root,
        &[
            "create",
            "method",
            "bar",
            "--parent",
            "namespaces/app/B",
            "--signature",
            "foo()",
            "--overrides",
            &method,
        ],
    );
    let output = ds(root, &["verify"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("closure"));

    let output = ds(
        root,
        &[
            "dep",
            "add",
            "namespaces/app/A",
            "namespaces/app/B",
            "--explicit",
        ],
    );
    assert!(!output.status.success());

    let output = ds(root, &["rm", "namespaces/app/A"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("children"));
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn syncs_files_and_directories_with_the_source_tree() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let ws = dir.path().join("ws");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(source.join("docs")).unwrap();
    fs::write(source.join("src/a.rs"), "fn a() {}\n").unwrap();
    fs::write(source.join("docs/x.md"), "x\n").unwrap();
    git(&source, &["init", "-q"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    git(&source, &["config", "user.name", "test"]);
    git(&source, &["add", "-A"]);
    git(&source, &["commit", "-qm", "initial"]);
    git(&source, &["branch", "-M", "master"]);

    let url = format!("file://{}", source.display());
    ok(&ws, &["init", "--source", &url]);
    ok(&ws, &["source", "fetch"]);
    ok(&ws, &["source", "checkout", "master"]);
    ok(&ws, &["source", "sync"]);

    let files = ok(&ws, &["ls", "--files"]);
    assert!(files.contains("files/src/a.rs"));
    assert!(files.contains("files/docs"));
    assert!(files.contains("files/docs/x.md"));

    ok(&ws, &["edit", "files/src/a.rs", "--design", "a rules"]);

    git(&source, &["mv", "src/a.rs", "src/b.rs"]);
    fs::write(source.join("src/c.rs"), "fn c() {}\n").unwrap();
    fs::remove_file(source.join("docs/x.md")).unwrap();
    git(&source, &["add", "-A"]);
    git(&source, &["commit", "-qm", "change"]);

    ok(&ws, &["source", "fetch"]);
    ok(&ws, &["source", "sync"]);

    let files = ok(&ws, &["ls", "--files"]);
    assert!(files.contains("files/src/b.rs"));
    assert!(!files.contains("files/src/a.rs"));
    assert!(files.contains("files/src/c.rs"));
    assert!(!files.contains("files/docs/x.md"));
    assert!(ok(&ws, &["show", "files/src/b.rs"]).contains("a rules"));
    assert!(!ds(&ws, &["show", "files/src/a.rs"]).status.success());
}

#[test]
fn top_level_and_nested_types_use_different_parent_forms() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "Base", "--namespace", "namespaces/app"],
    );

    let mixed = ds(
        root,
        &[
            "create",
            "class",
            "Mixed",
            "--parent",
            "namespaces/app/Base",
            "--namespace",
            "namespaces/app",
        ],
    );
    assert!(!mixed.status.success());
    assert!(
        String::from_utf8_lossy(&mixed.stderr).contains("nested type uses --parent")
    );

    let wrong_parent = ds(root, &["create", "class", "Bad", "--parent", "files"]);
    assert!(!wrong_parent.status.success());

    ok(
        root,
        &[
            "create",
            "class",
            "Inner",
            "--parent",
            "namespaces/app/Base",
        ],
    );
    assert!(ok(root, &["ls"]).contains("namespaces/app/Base/Inner"));
}
