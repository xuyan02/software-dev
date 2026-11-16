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
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cycle"));

    let method = ok(root, &["ls"])
        .lines()
        .find(|line| line.contains("A/foo#"))
        .unwrap()
        .to_owned();
    let output = ds(
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
