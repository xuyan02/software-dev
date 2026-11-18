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

fn status_has(text: &str, name: &str) -> bool {
    text.lines().any(|line| {
        line.split_once('\t')
            .is_some_and(|(_, qname)| qname == name)
    })
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
    ok(root, &["doc", "mkdir", "documents/design"]);
    ok(
        root,
        &[
            "doc",
            "create",
            "documents/design/technical",
            "--title",
            "技术设计",
        ],
    );
    ok(
        root,
        &["create", "class", "Base", "--namespace", "namespaces/app"],
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

    ok(
        root,
        &[
            "edit",
            "namespaces/app/Base",
            "--document",
            "documents/design/technical",
        ],
    );
    let diff = ok(root, &["diff"]);
    assert!(diff.contains("M namespaces/app/Base"));
    assert!(diff.contains("document:"));
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
    assert_eq!(
        ok(&clone, &["remote", "show"]).trim(),
        remote.to_str().unwrap()
    );
    ok(
        &clone,
        &[
            "create",
            "class",
            "Session",
            "--namespace",
            "namespaces/app",
        ],
    );
    ok(&clone, &["add", "-A"]);
    ok(&clone, &["commit", "-m", "clone push"]);
    ok(&clone, &["push"]);
    ok(&clone, &["fetch"]);
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
    ok(&ws, &["doc", "mkdir", "documents/design"]);
    ok(
        &ws,
        &[
            "doc",
            "create",
            "documents/design/directory",
            "--title",
            "目录设计",
        ],
    );
    ok(&ws, &["source", "fetch"]);
    ok(&ws, &["source", "checkout", "master"]);
    ok(&ws, &["source", "sync"]);

    let files = ok(&ws, &["ls", "--files"]);
    assert!(files.contains("files/src/a.rs"));
    assert!(files.contains("files/docs"));
    assert!(files.contains("files/docs/x.md"));

    ok(
        &ws,
        &[
            "edit",
            "files/src/a.rs",
            "--document",
            "documents/design/directory",
        ],
    );

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
    assert!(ok(&ws, &["show", "files/src/b.rs"]).contains("document: documents/design/directory"));
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
    assert!(String::from_utf8_lossy(&mixed.stderr).contains("has no parent"));

    let wrong_parent = ds(root, &["create", "class", "Bad", "--parent", "files"]);
    assert!(!wrong_parent.status.success());

    ok(
        root,
        &[
            "create",
            "inner-class",
            "Inner",
            "--parent",
            "namespaces/app/Base",
        ],
    );
    ok(
        root,
        &[
            "create",
            "inner-interface",
            "InnerApi",
            "--parent",
            "namespaces/app/Base",
        ],
    );
    let listed = ok(root, &["ls"]);
    assert!(listed.contains("namespaces/app/Base/Inner"));
    assert!(listed.contains("namespaces/app/Base/InnerApi"));
}

#[test]
fn stages_and_unstages_objects_associated_with_a_source_file() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let ws = dir.path().join("ws");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(
        source.join("src/a.rs"),
        "// one\n// two\n// three\n// four\n// five\n",
    )
    .unwrap();
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
    ok(
        &ws,
        &[
            "create",
            "namespace",
            "app",
            "--parent",
            "namespaces",
            "--source",
            "src/a.rs:1-5",
        ],
    );
    ok(
        &ws,
        &[
            "create",
            "class",
            "App",
            "--namespace",
            "namespaces/app",
            "--file",
            "files/src/a.rs",
            "--source",
            "src/a.rs:1-2",
        ],
    );
    ok(&ws, &["doc", "mkdir", "documents/design"]);
    ok(
        &ws,
        &[
            "doc",
            "create",
            "documents/design/technical",
            "--title",
            "技术设计",
        ],
    );
    ok(
        &ws,
        &[
            "doc",
            "create",
            "documents/design/directory",
            "--title",
            "目录设计",
        ],
    );
    ok(
        &ws,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "未分类实体",
        ],
    );
    ok(
        &ws,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1/section",
            "--title",
            "namespaces/app",
        ],
    );
    ok(
        &ws,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1/section1",
            "--title",
            "namespaces/app/App",
        ],
    );
    ok(
        &ws,
        &[
            "doc",
            "section",
            "add",
            "documents/design/directory/section",
            "--title",
            "src/a.rs",
        ],
    );
    ok(
        &ws,
        &[
            "edit",
            "namespaces/app",
            "--document",
            "documents/design/technical/section1/section1",
        ],
    );
    ok(
        &ws,
        &[
            "edit",
            "namespaces/app/App",
            "--document",
            "documents/design/technical/section1/section2",
        ],
    );
    ok(
        &ws,
        &[
            "edit",
            "files/src/a.rs",
            "--document",
            "documents/design/directory/section1",
        ],
    );

    let before = ok(&ws, &["status", "--porcelain"]);
    assert!(!before.contains("\tnamespace\t"));
    assert!(!before.contains("\tsrc/a.rs\t"));
    let detailed = ok(&ws, &["status", "--porcelain", "--detail"]);
    assert!(detailed.contains("namespace\tsrc/a.rs\tnamespaces/app"));
    assert!(detailed.contains("class\tsrc/a.rs\tnamespaces/app/App"));
    assert!(status_has(&before, "namespaces/app"));
    assert!(status_has(&before, "namespaces/app/App"));
    assert!(status_has(
        &before,
        "documents/design/technical/section1/section1"
    ));
    assert!(status_has(&before, "documents/design/directory/section1"));

    ok(&ws, &["add", "--file", "src/a.rs"]);
    let staged = ok(&ws, &["status", "--porcelain"]);
    assert!(!status_has(&staged, "namespaces/app/App"));
    assert!(!status_has(&staged, "namespaces/app"));
    assert!(!status_has(
        &staged,
        "documents/design/technical/section1/section1"
    ));
    assert!(!status_has(&staged, "documents/design/directory/section1"));

    ok(&ws, &["unstage", "--file", "src/a.rs"]);
    let unstaged = ok(&ws, &["status", "--porcelain"]);
    assert!(status_has(&unstaged, "namespaces/app/App"));
    assert!(status_has(
        &unstaged,
        "documents/design/technical/section1/section1"
    ));
    assert!(status_has(&unstaged, "documents/design/directory/section1"));
}

#[test]
fn creates_and_orders_document_fragments() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);

    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["doc", "create", "documents/specs/auth", "--title", "认证"],
    );

    // Insert the first section at the head; the document is empty.
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/section",
            "--title",
            "登录",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "第一段",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "第三段",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "第零段",
        ],
    );
    ok(
        root,
        &["doc", "list", "add", "documents/specs/auth/section1/body1"],
    );
    ok(
        root,
        &[
            "doc",
            "list",
            "item",
            "add",
            "documents/specs/auth/section1/body4/item",
            "--text",
            "第一项",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "list",
            "item",
            "add",
            "documents/specs/auth/section1/body4/item1",
            "--text",
            "第三项",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "list",
            "item",
            "add",
            "documents/specs/auth/section1/body4/item",
            "--text",
            "第零项",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/section1/section",
            "--title",
            "验证码",
        ],
    );

    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("document \"认证\""));
    assert!(shown.contains("section \"登录\""));
    assert!(shown.contains("text \"第零段\""));
    assert!(shown.contains("text \"第一段\""));
    assert!(shown.contains("list"));
    assert!(shown.contains("text \"第零项\""));
    assert!(shown.contains("text \"第一项\""));
    assert!(shown.contains("text \"第三项\""));
    assert!(shown.contains("text \"第三段\""));
    assert!(shown.contains("section \"验证码\""));
    let zero = shown.find("第零段").unwrap();
    let one = shown.find("第一段").unwrap();
    let list = shown.find("list\n").unwrap();
    let three = shown.find("第三段").unwrap();
    assert!(zero < one && one < list && list < three);

    let status = ok(root, &["status", "--porcelain"]);
    assert!(status_has(&status, "documents/specs"));
    assert!(status_has(&status, "documents/specs/auth"));
    assert!(status_has(&status, "documents/specs/auth/section1"));
    assert!(status_has(&status, "documents/specs/auth/section1/body1"));
    assert!(status_has(&status, "documents/specs/auth/section1/body4"));
    assert!(status_has(
        &status,
        "documents/specs/auth/section1/body4/item1"
    ));

    ok(root, &["add", "-A"]);
    ok(root, &["verify"]);
    ok(root, &["commit", "-m", "documents"]);
}

#[test]
fn rejects_invalid_document_targets() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["doc", "create", "documents/specs/auth", "--title", "认证"],
    );

    let output = ds(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/body",
            "--text",
            "x",
        ],
    );
    assert!(!output.status.success());

    let output = ds(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/nope",
            "--title",
            "x",
        ],
    );
    assert!(!output.status.success());

    let output = ds(
        root,
        &["doc", "create", "documents/specs/Auth", "--title", "x"],
    );
    assert!(!output.status.success());
}

#[test]
fn removes_document_fragments_from_parent_order() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["doc", "create", "documents/specs/auth", "--title", "认证"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/section",
            "--title",
            "登录",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "第一段",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "第二段",
        ],
    );
    ok(root, &["rm", "documents/specs/auth/section1/body2"]);
    ok(root, &["verify"]);
    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("第一段"));
    assert!(!shown.contains("第二段"));

    let output = ds(root, &["rm", "documents/specs/auth/section1"]);
    assert!(!output.status.success());
}

#[test]
fn links_code_objects_to_document_sections() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/design"]);
    ok(
        root,
        &[
            "doc",
            "create",
            "documents/design/technical",
            "--title",
            "技术设计",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "未分类实体",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1/section",
            "--title",
            "namespaces/app/Auth",
        ],
    );
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "Auth",
            "--namespace",
            "namespaces/app",
            "--document",
            "documents/design/technical/section1/section1",
        ],
    );
    ok(root, &["add", "namespaces/app/Auth"]);
    let status = ok(root, &["status", "--porcelain"]);
    assert!(!status.contains("namespaces/app/Auth"));
    assert!(!status.contains("documents/design/technical/section1/section1"));
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/design/technical/section1/section1/body",
            "--text",
            "负责认证边界。",
        ],
    );
    ok(root, &["verify"]);
    let shown = ok(root, &["show", "namespaces/app/Auth"]);
    assert!(shown.contains("document: documents/design/technical/section1/section1"));

    ok(root, &["edit", "namespaces/app/Auth", "--clear-document"]);
    ok(root, &["verify"]);
    let shown = ok(root, &["show", "namespaces/app/Auth"]);
    assert!(shown.contains("document: <none>"));
}

#[test]
fn rejects_document_links_to_non_document_objects() {
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
            "Auth",
            "--namespace",
            "namespaces/app",
            "--document",
            "namespaces/app",
        ],
    );
    let output = ds(root, &["verify"]);
    assert!(!output.status.success());
}

#[test]
fn adds_resolves_and_lists_document_comments() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["doc", "create", "documents/specs/auth", "--title", "认证"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/section",
            "--title",
            "登录",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "登录使用 OAuth。",
        ],
    );

    let added = ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "补充错误码",
        ],
    );
    assert!(added.contains("comment1"));

    let listed = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
        ],
    );
    assert!(listed.contains("comment1\topen\t补充错误码"));

    ok(
        root,
        &[
            "doc",
            "comment",
            "resolve",
            "documents/specs/auth/section1/body1",
            "1",
        ],
    );
    let listed = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
        ],
    );
    assert!(listed.contains("comment1\tresolved\t补充错误码"));

    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("comment1 [resolved] 补充错误码"));
    let shown = ok(root, &["show", "documents/specs/auth/section1/body1"]);
    assert!(shown.contains("comments: comment1 [resolved] 补充错误码"));

    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth",
            "--text",
            "文档级意见",
        ],
    );
    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("comment1 [open] 文档级意见"));

    let cleared = ok(root, &["doc", "comment", "clear"]);
    assert!(cleared.contains("Cleared 2 comment(s) from 2 object(s)"));
    let listed = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
        ],
    );
    assert!(listed.contains("no comments"));
    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(!shown.contains("comment1"));
    assert!(ok(root, &["doc", "comment", "clear"]).contains("no comments"));
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-m", "clear comments"]);
    assert!(ok(root, &["status"]).contains("working tree clean"));

    let output = ds(
        root,
        &[
            "doc",
            "comment",
            "resolve",
            "documents/specs/auth/section1/body1",
            "99",
        ],
    );
    assert!(!output.status.success());
}

#[test]
fn creates_titled_topics_in_section_body() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["doc", "create", "documents/specs/auth", "--title", "认证"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/specs/auth/section",
            "--title",
            "登录",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/specs/auth/section1/body",
            "--title",
            "认证流程",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section1/body1/body",
            "--text",
            "先校验凭据。",
        ],
    );

    ok(
        root,
        &[
            "doc",
            "list",
            "add",
            "documents/specs/auth/section1/body1/body1",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "list",
            "item",
            "add",
            "documents/specs/auth/section1/body1/body2/item",
            "--text",
            "第二段。",
        ],
    );

    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("topic \"认证流程\""));
    assert!(shown.contains("text \"先校验凭据。\""));
    assert!(shown.contains("text \"第二段。\""));
    let topic = shown.find("topic \"认证流程\"").unwrap();
    let text = shown.find("text \"先校验凭据。\"").unwrap();
    assert!(topic < text);

    let status = ok(root, &["status", "--porcelain"]);
    assert!(status_has(&status, "documents/specs/auth/section1/body1"));
    assert!(status_has(
        &status,
        "documents/specs/auth/section1/body1/body1"
    ));
    assert!(status_has(
        &status,
        "documents/specs/auth/section1/body1/body2"
    ));
    assert!(status_has(
        &status,
        "documents/specs/auth/section1/body1/body2/item1"
    ));

    // Topics can only be added directly to a section body.
    let output = ds(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/specs/auth/section1/body1/body",
            "--title",
            "嵌套",
        ],
    );
    assert!(!output.status.success());

    let output = ds(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/specs/auth/section/body",
            "--text",
            "x",
        ],
    );
    assert!(!output.status.success());
}

#[test]
fn unstages_a_directory_with_staged_children() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["create", "directory", "src", "--parent", "files"]);
    ok(root, &["create", "file", "a.rs", "--parent", "files/src"]);
    ok(root, &["add", "--directory", "src"]);
    ok(root, &["add", "--file", "src/a.rs"]);
    ok(root, &["unstage", "--directory", "src"]);
    let status = ok(root, &["status", "--porcelain"]);
    assert!(status_has(&status, "files/src"));
    assert!(status_has(&status, "files/src/a.rs"));
    ok(root, &["verify"]);
}

#[test]
fn creates_systems_modules_and_assigns_members_once() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/design"]);
    ok(
        root,
        &["doc", "create", "documents/design/technical", "--title", "技术设计"],
    );
    ok(
        root,
        &[
            "create",
            "namespace",
            "app",
            "--parent",
            "namespaces",
        ],
    );
    ok(
        root,
        &["create", "class", "Auth", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["create", "class", "Session", "--namespace", "namespaces/app"],
    );

    ok(root, &["structure", "init"]);
    ok(
        root,
        &[
            "structure",
            "system",
            "create",
            "app",
            "--title",
            "应用",
            "--document",
            "documents/design/technical",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "module",
            "create",
            "core",
            "--parent",
            "design/app",
            "--title",
            "核心",
            "--document",
            "documents/design/technical",
        ],
    );
    ok(
        root,
        &["structure", "assign", "design/app/core", "namespaces/app/Auth"],
    );
    let unassigned = ok(root, &["structure", "unassigned"]);
    assert_eq!(unassigned.trim(), "namespaces/app/Session");

    ok(
        root,
        &["structure", "assign", "design/app/core", "namespaces/app/Session"],
    );
    let checked = ok(root, &["structure", "check"]);
    assert!(checked.contains("structure ok"));
    let shown = ok(root, &["structure", "show", "design/app/core"]);
    assert!(shown.contains("kind: module"));
    assert!(shown.contains("namespaces/app/Auth"));
    assert!(shown.contains("namespaces/app/Session"));

    ok(
        root,
        &[
            "structure",
            "module",
            "create",
            "other",
            "--parent",
            "design/app",
            "--title",
            "其他",
            "--document",
            "documents/design/technical",
        ],
    );
    let output = ds(
        root,
        &["structure", "assign", "design/app/other", "namespaces/app/Auth"],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already belongs to module")
    );
}
