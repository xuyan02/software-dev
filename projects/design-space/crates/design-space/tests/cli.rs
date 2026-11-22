use std::fs;
use std::path::{Path, PathBuf};
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
fn creates_structs_and_enums_including_nested_ones() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "struct", "Point", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["create", "enum", "Color", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "create",
            "inner-struct",
            "Meta",
            "--parent",
            "namespaces/app/Point",
        ],
    );
    ok(
        root,
        &[
            "create",
            "inner-enum",
            "Kind",
            "--parent",
            "namespaces/app/Point",
        ],
    );

    let listed = ok(root, &["ls"]);
    assert!(listed.contains("namespaces/app/Point"));
    assert!(listed.contains("namespaces/app/Color"));
    assert!(listed.contains("namespaces/app/Point/Meta"));
    assert!(listed.contains("namespaces/app/Point/Kind"));
    assert!(ok(root, &["show", "namespaces/app/Point"]).contains("kind: Struct"));
    assert!(ok(root, &["show", "namespaces/app/Color"]).contains("kind: Enum"));
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
fn skeleton_documents_read_in_order() {
    let dir = tempdir().unwrap();
    ok(dir.path(), &["init", "--source", "file:///nonexistent"]);

    let shown = ok(dir.path(), &["doc", "show", "documents/design"]);
    let directory = shown.find("documents/design/directory").unwrap();
    let namespace = shown.find("documents/design/namespace").unwrap();
    let technical = shown.find("documents/design/technical").unwrap();
    let drafts = shown.find("documents/design/drafts").unwrap();
    assert!(directory < namespace);
    assert!(namespace < technical);
    assert!(technical < drafts);
}

#[test]
fn removes_a_document_subtree_recursively() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
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
            "documents/design/technical/section1/body",
            "--title",
            "定位",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/design/technical/section1/body1/body",
            "--text",
            "正文",
        ],
    );

    ok(
        root,
        &["rm", "--recursive", "documents/design/technical/section1"],
    );
    let listed = ok(root, &["ls", "--documents"]);
    assert!(!listed.contains("documents/design/technical/section1"));
    assert!(!listed.contains("documents/design/technical/section1/body1"));
}

#[test]
fn recursive_removal_keeps_external_references_unless_forced() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "Auth", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "Auth",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Auth",
            "--document",
            "documents/design/technical/section1",
        ],
    );

    let refused = ds(
        root,
        &["rm", "--recursive", "documents/design/technical/section1"],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("referenced by"));

    ok(
        root,
        &[
            "rm",
            "--recursive",
            "--force",
            "documents/design/technical/section1",
        ],
    );
    let auth = ok(root, &["show", "namespaces/app/Auth"]);
    assert!(auth.contains("document: <none>"));
}

#[test]
fn removes_nested_design_objects_recursively() {
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

    ok(root, &["rm", "--recursive", "namespaces/app/Base"]);
    let listed = ok(root, &["ls"]);
    assert!(!listed.contains("namespaces/app/Base"));
    assert!(!listed.contains("Base/foo"));
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
    // The server writes this file when it provisions the design space.
    fs::write(
        remote.join("repo.toml"),
        "[source]\nurl = \"file:///nonexistent\"\nobject_format = \"sha1\"\n",
    )
    .unwrap();

    ok(
        &clone,
        &["clone", remote.to_str().unwrap(), "--branch", "main"],
    );
    let repo_config = fs::read_to_string(clone.join(".ds/repo.toml")).unwrap();
    assert!(repo_config.contains("file:///nonexistent"));
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
    assert!(
        fs::read_to_string(remote.join("repo.toml"))
            .unwrap()
            .contains("file:///nonexistent"),
        "push must not modify repository metadata"
    );
}

#[test]
fn clones_repository_metadata_without_a_source_argument() {
    let dir = tempdir().unwrap();
    let repos = dir.path().join("repos");
    let collection = design_space::server::RepoCollection::open(&repos).unwrap();
    collection
        .create_with_source("app", "main", Some("https://example.com/source.git"))
        .unwrap();
    let clone = dir.path().join("clone");
    ok(
        &clone,
        &[
            "clone",
            repos.join("app").to_str().unwrap(),
            "--branch",
            "main",
        ],
    );
    let repo_config = fs::read_to_string(clone.join(".ds/repo.toml")).unwrap();
    assert!(repo_config.contains("https://example.com/source.git"));
    let local_config = fs::read_to_string(clone.join(".ds/config")).unwrap();
    assert!(!local_config.contains("source"));
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
fn source_fetch_follows_repo_toml_origin_changes() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    for (repo, file) in [(&first, "first.rs"), (&second, "second.rs")] {
        fs::create_dir_all(repo).unwrap();
        fs::write(repo.join(file), "fn f() {}\n").unwrap();
        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        git(repo, &["config", "user.name", "test"]);
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-qm", "initial"]);
        git(repo, &["branch", "-M", "master"]);
    }

    let ws = dir.path().join("ws");
    ok(
        &ws,
        &["init", "--source", &format!("file://{}", first.display())],
    );
    ok(&ws, &["source", "fetch"]);
    assert!(ok(&ws, &["source", "ls"]).contains("first.rs"));

    fs::write(
        ws.join(".ds/repo.toml"),
        format!(
            "[source]\nurl = \"file://{}\"\nobject_format = \"sha1\"\n",
            second.display()
        ),
    )
    .unwrap();
    ok(&ws, &["source", "fetch"]);
    let listed = ok(&ws, &["source", "ls"]);
    assert!(listed.contains("second.rs"));
    assert!(!listed.contains("first.rs"));
}

#[test]
fn source_fetch_creates_a_root_git_worktree() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let ws = dir.path().join("ws");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/a.rs"), "fn a() {}\n").unwrap();
    git(&source, &["init", "-q"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    git(&source, &["config", "user.name", "test"]);
    git(&source, &["add", "-A"]);
    git(&source, &["commit", "-qm", "initial"]);
    git(&source, &["branch", "-M", "master"]);

    let url = format!("file://{}", source.display());
    ok(&ws, &["init", "--source", &url]);
    ok(&ws, &["source", "fetch"]);

    let gitfile = fs::read_to_string(ws.join(".git")).unwrap();
    assert!(gitfile.contains(".ds/source/worktrees/root"));
    assert_eq!(
        fs::read_to_string(ws.join("src/a.rs")).unwrap(),
        "fn a() {}\n"
    );

    let output = Command::new("git")
        .current_dir(&ws)
        .args(["status", "--short"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    fs::write(ws.join("src/a.rs"), "fn a() { /* changed */ }\n").unwrap();
    let output = Command::new("git")
        .current_dir(&ws)
        .args(["status", "--short"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("src/a.rs"));

    let output = Command::new("git")
        .current_dir(&ws)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .unwrap();
    assert_eq!(
        PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
            .canonicalize()
            .unwrap(),
        ws.canonicalize().unwrap()
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
fn imports_a_document_subtree_idempotently() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );

    let plan = root.join("plan.json");
    fs::write(
        &plan,
        r#"{
  "root": {"qname": "documents/specs/auth", "kind": "document", "title": "认证"},
  "sections": [
    {
      "title": "登录",
      "document": ["namespaces/app"],
      "body": [{"type": "text", "text": "第一段"}],
      "sections": [
        {
          "title": "验证码",
          "body": [
            {"type": "text", "text": "第二段"},
            {"type": "list", "items": ["第一项", "第二项"]},
            {"type": "block", "language": "rust", "text": "fn main() {}"},
            {"type": "mermaid", "text": "graph TD; A-->B;"},
            {"type": "html", "text": "<b>hi</b>"},
            {"type": "topic", "title": "示例", "body": [{"type": "text", "text": "主题段落"}]}
          ]
        }
      ]
    }
  ]
}"#,
    )
    .unwrap();
    let plan_arg = plan.to_str().unwrap();

    let first = ok(root, &["doc", "import", "--file", plan_arg]);
    assert!(first.contains("Imported 2 section(s)"), "{first}");
    ok(root, &["verify"]);

    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(shown.contains("section \"登录\""), "{shown}");
    assert!(shown.contains("section \"验证码\""), "{shown}");
    for text in [
        "第一段",
        "第二段",
        "第一项",
        "第二项",
        "fn main() {}",
        "graph TD; A-->B;",
        "<b>hi</b>",
        "主题段落",
    ] {
        assert!(shown.contains(text), "missing {text:?}:\n{shown}");
    }
    assert!(
        ok(root, &["show", "namespaces/app"]).contains("document: documents/specs/auth/section1")
    );

    // Running the same plan again must reuse the sections instead of
    // duplicating them or their body elements.
    let second = ok(root, &["doc", "import", "--file", plan_arg]);
    assert!(second.contains("Imported 0 section(s)"), "{second}");
    let shown = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert_eq!(shown.matches("section \"登录\"").count(), 1);
    assert_eq!(shown.matches("section \"验证码\"").count(), 1);
    assert_eq!(shown.matches("第一段").count(), 1);
    assert_eq!(shown.matches("第一项").count(), 1);
    ok(root, &["verify"]);
}

#[test]
fn document_json_markdown_and_round_trip() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/specs"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );

    let plan = root.join("plan.json");
    fs::write(
        &plan,
        r#"{
  "root": {"qname": "documents/specs/auth", "kind": "document", "title": "认证"},
  "sections": [
    {
      "title": "登录",
      "document": ["namespaces/app"],
      "body": [{"type": "text", "text": "第一段"}],
      "sections": [
        {
          "title": "验证码",
          "body": [
            {"type": "list", "items": ["第一项", "第二项"]},
            {"type": "block", "language": "rust", "text": "fn main() {}"},
            {"type": "mermaid", "text": "graph TD; A-->B;"},
            {"type": "html", "text": "<b>hi</b>"},
            {"type": "topic", "title": "示例", "body": [{"type": "text", "text": "主题段落"}]}
          ]
        }
      ]
    }
  ]
}"#,
    )
    .unwrap();
    ok(root, &["doc", "import", "--file", plan.to_str().unwrap()]);

    let json = ok(
        root,
        &["doc", "show", "documents/specs/auth", "--format", "json"],
    );
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["root"]["qname"], "documents/specs/auth");
    assert_eq!(value["root"]["kind"], "document");
    assert_eq!(value["root"]["title"], "认证");
    assert_eq!(value["sections"][0]["title"], "登录");
    assert_eq!(value["sections"][0]["document"][0], "namespaces/app");
    assert_eq!(
        value["sections"][0]["sections"][0]["body"][0]["items"][0]["text"],
        "第一项"
    );
    assert!(
        value["sections"][0]["sections"][0]["body"][0]["items"][0]["qname"]
            .as_str()
            .unwrap()
            .ends_with("/item1")
    );

    // The exported JSON is valid import input; re-importing is a no-op.
    let exported = root.join("exported.json");
    fs::write(&exported, &json).unwrap();
    let reimport = ok(
        root,
        &["doc", "import", "--file", exported.to_str().unwrap()],
    );
    assert!(reimport.contains("Imported 0 section(s)"), "{reimport}");

    // A qname in the exported JSON targets that exact object, even when the
    // text changes.
    let mut updated = value.clone();
    updated["sections"][0]["sections"][0]["body"][0]["items"][0]["text"] =
        serde_json::json!("第一项（修改后）");
    let updated_path = root.join("updated.json");
    fs::write(&updated_path, serde_json::to_string(&updated).unwrap()).unwrap();
    ok(
        root,
        &["doc", "import", "--file", updated_path.to_str().unwrap()],
    );
    let updated_json = ok(
        root,
        &["doc", "show", "documents/specs/auth", "--format", "json"],
    );
    let updated_value: serde_json::Value = serde_json::from_str(&updated_json).unwrap();
    let item = &updated_value["sections"][0]["sections"][0]["body"][0]["items"][0];
    assert_eq!(item["text"], "第一项（修改后）");
    assert!(item["qname"].as_str().unwrap().ends_with("/item1"));

    let md = ok(
        root,
        &["doc", "show", "documents/specs/auth", "--format", "md"],
    );
    for text in [
        "# 认证",
        "## 登录",
        "### 验证码",
        "第一段",
        "第一项",
        "```rust",
        "fn main() {}",
        "```mermaid",
        "graph TD; A-->B;",
        "```html",
        "<b>hi</b>",
        "示例",
        "主题段落",
        "<!-- document: namespaces/app -->",
    ] {
        assert!(md.contains(text), "missing {text:?}:\n{md}");
    }

    let object = ok(root, &["show", "namespaces/app", "--format", "json"]);
    let value: serde_json::Value = serde_json::from_str(&object).unwrap();
    assert_eq!(value["qname"], "namespaces/app");
    assert_eq!(value["kind"], "namespace");
    assert_eq!(value["document"], "documents/specs/auth/section1");

    let md = ok(root, &["show", "namespaces/app", "--format", "md"]);
    assert!(md.contains("# namespaces/app"), "{md}");
    assert!(md.contains("- kind: namespace"), "{md}");
    assert!(
        md.contains("- document: documents/specs/auth/section1"),
        "{md}"
    );
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
fn lists_open_comments_across_all_documents() {
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
    ok(
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
    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1",
            "--text",
            "章节级意见",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "第二个正文意见",
        ],
    );

    let open = ok(root, &["doc", "comment", "list", "--open"]);
    assert!(open.contains("documents/specs/auth\tcomment1\topen\t文档级意见"));
    assert!(!open.contains("补充错误码"));

    let all = ok(root, &["doc", "comment", "list"]);
    assert!(all.contains("documents/specs/auth\tcomment1\topen\t文档级意见"));
    assert!(all.contains("documents/specs/auth/section1/body1\tcomment1\tresolved\t补充错误码"));

    let filtered = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
            "--open",
        ],
    );
    assert!(filtered.contains("comment2\topen\t第二个正文意见"));
    assert!(!filtered.contains("补充错误码"));

    let exact = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1",
            "--open",
        ],
    );
    assert!(exact.contains("comment1\topen\t章节级意见"));
    assert!(!exact.contains("第二个正文意见"));

    let subtree = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1",
            "--open",
            "--subtree",
        ],
    );
    assert!(subtree.contains("documents/specs/auth/section1\tcomment1\topen\t章节级意见"));
    assert!(
        subtree.contains("documents/specs/auth/section1/body1\tcomment2\topen\t第二个正文意见")
    );
    assert!(!subtree.contains("补充错误码"));
    assert!(!subtree.contains("文档级意见"));
}

#[test]
fn manages_concepts_and_concept_references() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["concept", "mkdir", "concepts/agent"]);
    ok(
        root,
        &[
            "concept",
            "create",
            "concepts/agent/agent",
            "--name",
            "智能体",
            "--summary",
            "能自主执行任务的主体。",
            "--explanation",
            "智能体由身份、工具和运行状态组成。",
        ],
    );
    ok(
        root,
        &[
            "concept",
            "feature",
            "add",
            "concepts/agent/agent",
            "有独立身份。",
        ],
    );
    ok(
        root,
        &[
            "concept",
            "relation",
            "add",
            "concepts/agent/agent",
            "智能体执行任务，与任务是执行关系。",
        ],
    );
    ok(
        root,
        &["concept", "alias", "add", "concepts/agent/agent", "Agent"],
    );

    let shown = ok(root, &["concept", "show", "concepts/agent/agent"]);
    assert!(shown.contains("name: 智能体"));
    assert!(shown.contains("summary: 能自主执行任务的主体。"));
    assert!(shown.contains("characteristics: 有独立身份。"));
    assert!(shown.contains("relations: 智能体执行任务，与任务是执行关系。"));
    assert!(shown.contains("aliases: Agent"));

    let listed = ok(root, &["concept", "ls", "concepts/agent"]);
    assert!(listed.contains("concept\tconcepts/agent/agent\t智能体\t能自主执行任务的主体。"));

    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "概念",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/design/technical/section1/body",
            "--text",
            "[[concept:concepts/agent/agent|智能体]] 指能自主执行任务的主体。",
        ],
    );
    let checked = ok(root, &["concept", "check"]);
    assert!(checked.contains("concept references ok"));

    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/design/technical/section1/body",
            "--text",
            "[[concept:concepts/missing|缺失概念]]",
        ],
    );
    let check = ds(root, &["concept", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("concept not found: concepts/missing"));

    ok(
        root,
        &[
            "concept",
            "create",
            "concepts/agent/task",
            "--summary",
            "可被分派和执行的工作单元。",
        ],
    );
    ok(
        root,
        &[
            "concept",
            "mv",
            "concepts/agent/task",
            "concepts",
            "--name",
            "task",
        ],
    );
    let moved = ok(root, &["concept", "show", "concepts/task"]);
    assert!(moved.contains("summary: 可被分派和执行的工作单元。"));
    ok(root, &["concept", "rm", "concepts/task"]);
    let listed = ok(root, &["concept", "ls"]);
    assert!(listed.contains("concept-directory\tconcepts/agent"));
    assert!(!listed.contains("concepts/task"));
}

#[test]
fn renames_and_moves_document_subtrees() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(root, &["doc", "mkdir", "documents/design/old"]);
    ok(
        root,
        &[
            "doc",
            "create",
            "documents/design/old/example",
            "--title",
            "示例草案",
        ],
    );
    ok(
        root,
        &["doc", "mv", "documents/design/old", "documents/design/new"],
    );
    let draft = ok(root, &["show", "documents/design/new"]);
    assert!(draft.contains("parent: documents/design"));
    assert!(!ds(root, &["show", "documents/design/old"]).status.success());

    ok(
        root,
        &[
            "doc",
            "mv",
            "documents/design/new/example",
            "documents/design/example",
        ],
    );
    let moved = ok(root, &["show", "documents/design/example"]);
    assert!(moved.contains("title: 示例草案"));
    assert!(moved.contains("parent: documents/design"));
    assert!(
        !ds(root, &["show", "documents/design/new/example"])
            .status
            .success()
    );
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
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "Auth", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "Session",
            "--namespace",
            "namespaces/app",
        ],
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
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Auth",
        ],
    );
    let unassigned = ok(root, &["structure", "unassigned"]);
    assert_eq!(unassigned.trim(), "namespaces/app/Session");

    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Session",
        ],
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
        &[
            "structure",
            "assign",
            "design/app/other",
            "namespaces/app/Auth",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already belongs to"));

    // A system may own top-level elements directly and also contain modules.
    ok(
        root,
        &["create", "class", "Direct", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["structure", "assign", "design/app", "namespaces/app/Direct"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "OtherThing",
            "--namespace",
            "namespaces/app",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/other",
            "namespaces/app/OtherThing",
        ],
    );
    let checked = ok(root, &["structure", "check"]);
    assert!(checked.contains("structure ok"));
    let shown = ok(root, &["structure", "show", "design/app"]);
    assert!(shown.contains("namespaces/app/Direct"));
    let output = ds(
        root,
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Direct",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already belongs to"));
}

#[test]
fn lays_out_technical_design_sections_by_structure() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "class", "Auth", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &[
            "create",
            "class",
            "Session",
            "--namespace",
            "namespaces/app",
        ],
    );
    ok(
        root,
        &[
            "create",
            "method",
            "login",
            "--parent",
            "namespaces/app/Auth",
            "--signature",
            "login()",
        ],
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
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Auth",
            "namespaces/app/Session",
        ],
    );
    for title in [
        "应用",
        "核心",
        "namespaces/app/Auth",
        "namespaces/app/Session",
    ] {
        ok(
            root,
            &[
                "doc",
                "section",
                "add",
                "documents/design/technical/section",
                "--title",
                title,
            ],
        );
    }
    ok(
        root,
        &[
            "edit",
            "design/app",
            "--document",
            "documents/design/technical/section1",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "design/app/core",
            "--document",
            "documents/design/technical/section2",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Auth",
            "--document",
            "documents/design/technical/section3",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Session",
            "--document",
            "documents/design/technical/section4",
        ],
    );
    ok(
        root,
        &[
            "create",
            "inner-class",
            "Inner",
            "--parent",
            "namespaces/app/Auth",
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
            "namespaces/app/Auth/Inner",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Auth/Inner",
            "--document",
            "documents/design/technical/section5",
        ],
    );
    ok(
        root,
        &["create", "class", "Entry", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["structure", "assign", "design/app", "namespaces/app/Entry"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "namespaces/app/Entry",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Entry",
            "--document",
            "documents/design/technical/section6",
        ],
    );
    let login = ok(root, &["ls"])
        .lines()
        .find(|line| line.starts_with("namespaces/app/Auth/login#"))
        .expect("method qname")
        .to_owned();
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            &login,
        ],
    );
    ok(
        root,
        &[
            "edit",
            &login,
            "--document",
            "documents/design/technical/section7",
        ],
    );

    let check = ds(root, &["doc", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("expected"));

    ok(root, &["doc", "layout"]);
    let checked = ok(root, &["doc", "check"]);
    assert!(checked.contains("document hierarchy ok"));

    let auth = ok(root, &["show", "namespaces/app/Auth"]);
    assert!(auth.contains("document: documents/design/technical/section1/section1/section1"));
    let inner = ok(root, &["show", "namespaces/app/Auth/Inner"]);
    assert!(
        inner.contains("document: documents/design/technical/section1/section1/section1/section1")
    );
    let system = ok(root, &["show", "design/app"]);
    assert!(system.contains("document: documents/design/technical/section1"));
    let entry = ok(root, &["show", "namespaces/app/Entry"]);
    assert!(entry.contains("document: documents/design/technical/section1/section2"));

    let login_doc = ok(root, &["show", &login]);
    assert!(
        login_doc
            .contains("document: documents/design/technical/section1/section1/section1/section2")
    );

    let titles = [
        ("documents/design/technical/section1", "应用"),
        ("documents/design/technical/section1/section1", "核心"),
        (
            "documents/design/technical/section1/section1/section1",
            "Auth",
        ),
        (
            "documents/design/technical/section1/section1/section1/section1",
            "Inner",
        ),
        (
            "documents/design/technical/section1/section1/section1/section2",
            "login",
        ),
        ("documents/design/technical/section1/section2", "Entry"),
    ];
    for (section, title) in titles {
        let shown = ok(root, &["show", section]);
        assert!(
            shown.contains(&format!("title: {title}")),
            "{section}: {shown}"
        );
    }
}

#[test]
fn namespace_section_titles_use_short_names() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    ok(
        root,
        &["create", "namespace", "auth", "--parent", "namespaces/app"],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/namespace/section",
            "--title",
            "namespaces/app",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/namespace/section1",
            "--title",
            "namespaces/app/auth",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app",
            "--document",
            "documents/design/namespace/section1",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/auth",
            "--document",
            "documents/design/namespace/section2",
        ],
    );

    ok(root, &["doc", "layout"]);
    let checked = ok(root, &["doc", "check"]);
    assert!(checked.contains("document hierarchy ok"));

    let app = ok(root, &["show", "documents/design/namespace/section1"]);
    assert!(app.contains("title: app"));
    let auth = ok(root, &["show", "documents/design/namespace/section2"]);
    assert!(auth.contains("title: auth"));
}

#[test]
fn design_principles_section_must_be_first() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "其他",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1",
            "--title",
            "设计原则",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/design/technical/section2/body",
            "--title",
            "依赖方向",
        ],
    );

    let check = ds(root, &["doc", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("must be the first Section"));
}

#[test]
fn accepts_design_principles_section_at_the_top() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "设计原则",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1",
            "--title",
            "其他",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/design/technical/section1/body",
            "--title",
            "依赖方向",
        ],
    );
    let checked = ok(root, &["doc", "check"]);
    assert!(checked.contains("document hierarchy ok"));
}

#[test]
fn rejects_non_topic_blocks_in_design_principles_section() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "设计原则",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "text",
            "add",
            "documents/design/technical/section1/body",
            "--text",
            "遵循单一职责原则。",
        ],
    );

    let check = ds(root, &["doc", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("must contain principle topics only"));
}

#[test]
fn rejects_duplicate_design_principles_sections() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section",
            "--title",
            "设计原则",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section1",
            "--title",
            "其他",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "section",
            "add",
            "documents/design/technical/section2",
            "--title",
            "设计原则",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/design/technical/section1/body",
            "--title",
            "依赖方向",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/design/technical/section3/body",
            "--title",
            "并发模型",
        ],
    );

    let check = ds(root, &["doc", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("appears 2 times"));
}

#[test]
fn creates_layers_and_enforces_layer_rules() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    for name in ["Api", "Core", "Store"] {
        ok(
            root,
            &["create", "class", name, "--namespace", "namespaces/app"],
        );
    }

    ok(root, &["structure", "init"]);
    ok(
        root,
        &["structure", "system", "create", "app", "--title", "应用"],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "api",
            "--parent",
            "design/app",
            "--title",
            "接口层",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "core",
            "--parent",
            "design/app",
            "--title",
            "核心层",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "store",
            "--parent",
            "design/app",
            "--title",
            "存储层",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/api",
            "namespaces/app/Api",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Core",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/store",
            "namespaces/app/Store",
        ],
    );

    let shown = ok(root, &["structure", "show", "design/app/api"]);
    assert!(shown.contains("kind: layer"));
    assert!(shown.contains("title: 接口层"));
    assert!(shown.contains("order: 1"));
    assert!(shown.contains("namespaces/app/Api"));
    assert!(ok(root, &["structure", "show", "design/app/core"]).contains("order: 2"));
    let listed = ok(root, &["structure", "list"]);
    assert!(listed.contains("layer\tdesign/app/api\t接口层\tmembers=1"));
    assert!(ok(root, &["structure", "check"]).contains("structure ok"));

    // Sibling layers must use distinct orders.
    let duplicate = ds(
        root,
        &[
            "structure",
            "layer",
            "create",
            "duplicate",
            "--parent",
            "design/app",
            "--order",
            "1",
        ],
    );
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already used by a sibling layer"));

    // Layers may not nest.
    let nested = ds(
        root,
        &[
            "structure",
            "layer",
            "create",
            "inner",
            "--parent",
            "design/app/api",
        ],
    );
    assert!(!nested.status.success());
    assert!(
        String::from_utf8_lossy(&nested.stderr).contains("layer parent must be a system or module")
    );

    // Only adjacent layers may depend on each other.
    ok(
        root,
        &[
            "dep",
            "add",
            "namespaces/app/Api",
            "namespaces/app/Store",
            "--explicit",
        ],
    );
    let check = ds(root, &["structure", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("interact but are not adjacent"));
    ok(
        root,
        &[
            "dep",
            "rm",
            "namespaces/app/Api",
            "namespaces/app/Store",
            "--explicit",
        ],
    );
    ok(
        root,
        &[
            "dep",
            "add",
            "namespaces/app/Api",
            "namespaces/app/Core",
            "--explicit",
        ],
    );
    assert!(ok(root, &["structure", "check"]).contains("structure ok"));

    // Empty layers are reported and can be removed.
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "spare",
            "--parent",
            "design/app",
        ],
    );
    let check = ds(root, &["structure", "check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("layer is empty: design/app/spare"));
    ok(root, &["structure", "layer", "rm", "design/app/spare"]);

    // A container with layers cannot own direct members or modules.
    ok(
        root,
        &["create", "class", "Direct", "--namespace", "namespaces/app"],
    );
    ok(
        root,
        &["structure", "assign", "design/app", "namespaces/app/Direct"],
    );
    let check = ds(root, &["structure", "check"]);
    assert!(!check.status.success());
    assert!(
        String::from_utf8_lossy(&check.stdout).contains("layered container has direct members")
    );
    ok(
        root,
        &[
            "structure",
            "unassign",
            "design/app",
            "namespaces/app/Direct",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/api",
            "namespaces/app/Direct",
        ],
    );

    // Modules and their members may live inside a layer.
    ok(
        root,
        &[
            "create",
            "class",
            "Handler",
            "--namespace",
            "namespaces/app",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "module",
            "create",
            "handlers",
            "--parent",
            "design/app/api",
            "--title",
            "处理器",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/api/handlers",
            "namespaces/app/Handler",
        ],
    );
    assert!(ok(root, &["structure", "check"]).contains("structure ok"));

    let remove = ds(root, &["structure", "layer", "rm", "design/app/api"]);
    assert!(!remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stderr).contains("layer is not empty"));
}

#[test]
fn lays_out_layer_sections_in_order() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ok(root, &["init", "--source", "file:///nonexistent"]);
    ok(
        root,
        &["create", "namespace", "app", "--parent", "namespaces"],
    );
    for name in ["Api", "Core"] {
        ok(
            root,
            &["create", "class", name, "--namespace", "namespaces/app"],
        );
    }
    ok(root, &["structure", "init"]);
    ok(
        root,
        &["structure", "system", "create", "app", "--title", "应用"],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "api",
            "--parent",
            "design/app",
            "--title",
            "接口层",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "create",
            "core",
            "--parent",
            "design/app",
            "--title",
            "核心层",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/api",
            "namespaces/app/Api",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "assign",
            "design/app/core",
            "namespaces/app/Core",
        ],
    );

    // Create the sections out of order; layout must move them under the
    // matching layer in layer order.
    for title in [
        "核心层",
        "namespaces/app/Core",
        "接口层",
        "namespaces/app/Api",
        "应用",
    ] {
        ok(
            root,
            &[
                "doc",
                "section",
                "add",
                "documents/design/technical/section",
                "--title",
                title,
            ],
        );
    }
    ok(
        root,
        &[
            "edit",
            "design/app",
            "--document",
            "documents/design/technical/section5",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "set",
            "design/app/api",
            "--document",
            "documents/design/technical/section3",
        ],
    );
    ok(
        root,
        &[
            "structure",
            "layer",
            "set",
            "design/app/core",
            "--document",
            "documents/design/technical/section1",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Api",
            "--document",
            "documents/design/technical/section4",
        ],
    );
    ok(
        root,
        &[
            "edit",
            "namespaces/app/Core",
            "--document",
            "documents/design/technical/section2",
        ],
    );

    let check = ds(root, &["doc", "check"]);
    assert!(!check.status.success());

    ok(root, &["doc", "layout"]);
    assert!(ok(root, &["doc", "check"]).contains("document hierarchy ok"));

    let titles = [
        ("documents/design/technical/section5", "应用"),
        ("documents/design/technical/section5/section1", "接口层"),
        (
            "documents/design/technical/section5/section1/section1",
            "Api",
        ),
        ("documents/design/technical/section5/section2", "核心层"),
        (
            "documents/design/technical/section5/section2/section1",
            "Core",
        ),
    ];
    for (section, title) in titles {
        let shown = ok(root, &["show", section]);
        assert!(
            shown.contains(&format!("title: {title}")),
            "{section}: {shown}"
        );
    }
}

#[test]
fn adds_and_edits_fenced_code_blocks() {
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
            "使用代码块说明协议。",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "block",
            "add",
            "documents/specs/auth/section1/body1",
            "--language",
            "rust",
            "--text",
            "let token = login();\nnext_step();",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "block",
            "add",
            "documents/specs/auth/section1/body2",
            "--text",
            "[[concept:concepts/missing|代码里的引用不参与检查]]",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "topic",
            "add",
            "documents/specs/auth/section1/body3",
            "--title",
            "流程",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "block",
            "add",
            "documents/specs/auth/section1/body4/body",
            "--language",
            "mermaid",
            "--text",
            "flowchart TD",
        ],
    );

    let rendered = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(rendered.contains("body2  block \"rust\""));
    assert!(rendered.contains("let token = login();"));
    assert!(rendered.contains("next_step();"));
    assert!(rendered.contains("body3  block \"\""));
    assert!(rendered.contains("body4  topic \"流程\""));
    assert!(rendered.contains("body1  block \"mermaid\""));

    let shown = ok(root, &["show", "documents/specs/auth/section1/body2"]);
    assert!(shown.contains("kind: Block"));
    assert!(shown.contains("language: rust"));
    assert!(shown.contains("text: let token = login();"));
    let listed = ok(root, &["ls", "--documents"]);
    assert!(listed.contains("documents/specs/auth/section1/body2"));
    assert!(ok(root, &["concept", "check"]).contains("concept references ok"));

    ok(
        root,
        &[
            "doc",
            "block",
            "set",
            "documents/specs/auth/section1/body2",
            "--language",
            "text",
            "--text",
            "changed code",
        ],
    );
    let shown = ok(root, &["show", "documents/specs/auth/section1/body2"]);
    assert!(shown.contains("language: text"));
    assert!(shown.contains("text: changed code"));

    let unchanged = ds(
        root,
        &["doc", "block", "set", "documents/specs/auth/section1/body2"],
    );
    assert!(!unchanged.status.success());
    assert!(String::from_utf8_lossy(&unchanged.stderr).contains("nothing to update"));

    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1/body2",
            "--text",
            "示例需要补充错误码",
        ],
    );
    let comments = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body2",
        ],
    );
    assert!(comments.contains("comment1\topen\t示例需要补充错误码"));

    // Blocks are body elements, not list items.
    ok(
        root,
        &["doc", "list", "add", "documents/specs/auth/section1/body"],
    );
    let in_list = ds(
        root,
        &[
            "doc",
            "block",
            "add",
            "documents/specs/auth/section1/body5/item",
            "--text",
            "x",
        ],
    );
    assert!(!in_list.status.success());
}

#[test]
fn adds_and_edits_mermaid_diagrams() {
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
            "mermaid",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "flowchart TD\n  A[请求] --> B[校验]",
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
            "图前后要有说明。",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "mermaid",
            "add",
            "documents/specs/auth/section1/body2",
            "--text",
            "sequenceDiagram\n  A->>B: 校验凭据",
        ],
    );

    let rendered = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(rendered.contains("body1  mermaid"));
    assert!(rendered.contains("flowchart TD"));
    assert!(rendered.contains("body2  text"));
    assert!(rendered.contains("body3  mermaid"));
    assert!(rendered.contains("sequenceDiagram"));

    let shown = ok(root, &["show", "documents/specs/auth/section1/body1"]);
    assert!(shown.contains("kind: MermaidBlock"));
    assert!(shown.contains("text: flowchart TD"));

    ok(
        root,
        &[
            "doc",
            "mermaid",
            "set",
            "documents/specs/auth/section1/body1",
            "--text",
            "flowchart LR\n  A --> B",
        ],
    );
    let shown = ok(root, &["show", "documents/specs/auth/section1/body1"]);
    assert!(shown.contains("text: flowchart LR"));
    assert!(!shown.contains("flowchart TD"));

    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "图里缺少错误分支",
        ],
    );
    let comments = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
        ],
    );
    assert!(comments.contains("comment1\topen\t图里缺少错误分支"));

    // A text paragraph cannot be set as a Mermaid diagram.
    let wrong = ds(
        root,
        &[
            "doc",
            "mermaid",
            "set",
            "documents/specs/auth/section1/body2",
            "--text",
            "flowchart TD",
        ],
    );
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("not a mermaid block"));
}

#[test]
fn adds_and_edits_html_previews() {
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
            "登录页",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "html",
            "add",
            "documents/specs/auth/section1/body",
            "--text",
            "<div style=\"padding:12px\">UI 效果图</div>",
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
            "效果图说明。",
        ],
    );
    ok(
        root,
        &[
            "doc",
            "html",
            "add",
            "documents/specs/auth/section1/body2",
            "--text",
            "<svg width=\"40\" height=\"20\"><rect width=\"40\" height=\"20\"/></svg>",
        ],
    );

    let rendered = ok(root, &["doc", "show", "documents/specs/auth"]);
    assert!(rendered.contains("body1  html"));
    assert!(rendered.contains("UI 效果图"));
    assert!(rendered.contains("body2  text"));
    assert!(rendered.contains("body3  html"));
    assert!(rendered.contains("<svg"));

    let shown = ok(root, &["show", "documents/specs/auth/section1/body1"]);
    assert!(shown.contains("kind: HtmlBlock"));
    assert!(shown.contains("text: <div style=\"padding:12px\">UI 效果图</div>"));

    ok(
        root,
        &[
            "doc",
            "html",
            "set",
            "documents/specs/auth/section1/body1",
            "--text",
            "<div>新的效果图</div>",
        ],
    );
    let shown = ok(root, &["show", "documents/specs/auth/section1/body1"]);
    assert!(shown.contains("text: <div>新的效果图</div>"));
    assert!(!shown.contains("UI 效果图"));

    ok(
        root,
        &[
            "doc",
            "comment",
            "add",
            "documents/specs/auth/section1/body1",
            "--text",
            "需要补充空状态",
        ],
    );
    let comments = ok(
        root,
        &[
            "doc",
            "comment",
            "list",
            "documents/specs/auth/section1/body1",
        ],
    );
    assert!(comments.contains("comment1\topen\t需要补充空状态"));

    let wrong = ds(
        root,
        &[
            "doc",
            "html",
            "set",
            "documents/specs/auth/section1/body2",
            "--text",
            "<div>x</div>",
        ],
    );
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("not an html block"));
}
