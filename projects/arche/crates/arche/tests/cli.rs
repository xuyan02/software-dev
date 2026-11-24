use std::process::Command;

use tempfile::TempDir;

#[test]
fn creates_technical_code_objects() {
    let temp = TempDir::new().unwrap();
    let init = arche().arg("init").arg(temp.path()).output().unwrap();
    assert!(init.status.success(), "{init:?}");

    let run = |args: &[&str]| {
        let output = arche()
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    };

    run(&["create", "technical-system", "app"]);
    run(&[
        "create",
        "technical-namespace",
        "--parent",
        "technical",
        "ns",
    ]);
    let class = run(&[
        "create",
        "technical-class",
        "--namespace",
        "technical/namespace/ns",
        "Foo",
    ]);
    assert!(class.contains("technical/namespace/ns/Foo"));
    let method = run(&[
        "create",
        "technical-method",
        "--parent",
        "technical/namespace/ns/Foo",
        "--prototype",
        "run()",
    ]);
    assert!(method.contains("run()"));
    run(&[
        "create",
        "technical-part",
        "body",
        "--parent",
        "technical/namespace/ns/Foo",
        "--source",
        "src/lib.rs:1-2",
    ]);
    run(&[
        "create",
        "technical-function",
        "--namespace",
        "technical/namespace/ns",
        "free",
    ]);

    let listed = run(&["ls"]);
    for kind in [
        "TechnicalClass",
        "TechnicalMethod",
        "TechnicalPart",
        "TechnicalFunction",
    ] {
        assert!(listed.contains(kind), "{kind} missing: {listed}");
    }

    let method_qname = method.lines().next().unwrap();
    let shown = run(&["show", method_qname]);
    assert!(shown.contains("arguments:"));
    assert!(shown.contains("return_value:"));
}

#[test]
fn imports_documents_and_applies_batch() {
    let temp = TempDir::new().unwrap();
    let init = arche().arg("init").arg(temp.path()).output().unwrap();
    assert!(init.status.success(), "{init:?}");
    let run = |args: &[&str]| {
        let output = arche()
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    run(&["create", "technical-system", "app"]);

    let document = temp.path().join("position.json");
    std::fs::write(
        &document,
        r#"{"fields":{"position":{"type":"DocumentText","content":[{"type":"text","text":"应用入口"}]}}}"#,
    )
    .unwrap();
    run(&["doc", "import", "technical/app", document.to_str().unwrap()]);
    let shown = run(&["show", "technical/app", "--format", "json"]);
    assert!(shown.contains("应用入口"), "{shown}");

    let plan = temp.path().join("plan.json");
    std::fs::write(
        &plan,
        r#"{
          "objects":[{"kind":"technical-module","name":"core","parent":"technical/app"}],
          "documents":[{"qname":"technical/app/core","fields":{"position":{"type":"DocumentText","content":[{"type":"text","text":"核心模块"}]}}}]
        }"#,
    )
    .unwrap();
    run(&["batch", plan.to_str().unwrap()]);
    let module = run(&["show", "technical/app/core"]);
    assert!(module.contains("核心模块"), "{module}");
}

#[test]
fn repack_keeps_objects_readable() {
    let temp = TempDir::new().unwrap();
    let init = arche().arg("init").arg(temp.path()).output().unwrap();
    assert!(init.status.success(), "{init:?}");
    let run = |args: &[&str]| {
        let output = arche()
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    run(&["create", "technical-system", "app"]);
    run(&["repack"]);
    let shown = run(&["show", "technical/app"]);
    assert!(shown.contains("TechnicalSystem"), "{shown}");
    run(&["gc"]);
    let shown = run(&["show", "technical/app"]);
    assert!(shown.contains("TechnicalSystem"), "{shown}");
}

#[test]
fn built_in_help_covers_objects_and_fields() {
    let objects = arche().args(["help", "objects"]).output().unwrap();
    assert!(objects.status.success(), "{objects:?}");
    let text = String::from_utf8(objects.stdout).unwrap();
    assert!(text.contains("TechnicalMethod"));
    assert!(text.contains("RequirementStory"));

    let technical = arche().args(["help", "technical"]).output().unwrap();
    assert!(technical.status.success(), "{technical:?}");
    let text = String::from_utf8(technical.stdout).unwrap();
    assert!(text.contains("arche create technical-class"));
    assert!(text.contains("arche doc list item add"));

    let fields = arche().args(["help", "fields"]).output().unwrap();
    assert!(fields.status.success(), "{fields:?}");
    let text = String::from_utf8(fields.stdout).unwrap();
    assert!(text.contains("TechnicalClass"));
    assert!(text.contains("DocumentParagraphList"));
}

fn arche() -> Command {
    Command::new(env!("CARGO_BIN_EXE_arche"))
}

#[test]
fn init_create_show() {
    let temp = TempDir::new().unwrap();
    let init = arche()
        .arg("init")
        .arg(temp.path())
        .output()
        .expect("run arche init");
    assert!(init.status.success(), "{:?}", init);

    let create = arche()
        .current_dir(temp.path())
        .arg("create")
        .arg("technical-system")
        .arg("app")
        .output()
        .expect("run arche create");
    assert!(create.status.success(), "{:?}", create);
    let stdout = String::from_utf8(create.stdout).unwrap();
    assert!(stdout.contains("technical/app"));

    let show = arche()
        .current_dir(temp.path())
        .arg("show")
        .arg("technical/app")
        .output()
        .expect("run arche show");
    assert!(show.status.success(), "{:?}", show);
    let stdout = String::from_utf8(show.stdout).unwrap();
    assert!(stdout.contains("kind:  TechnicalSystem"));
    assert!(stdout.contains("parent: technical"));

    let module = arche()
        .current_dir(temp.path())
        .arg("create")
        .arg("technical-module")
        .arg("core")
        .arg("--parent")
        .arg("technical/app")
        .output()
        .expect("run arche create module");
    assert!(module.status.success(), "{:?}", module);

    let doc = arche()
        .current_dir(temp.path())
        .arg("doc")
        .arg("text")
        .arg("set")
        .arg("technical/app")
        .arg("position")
        .arg("应用入口。")
        .output()
        .expect("run arche doc text set");
    assert!(doc.status.success(), "{:?}", doc);

    let show = arche()
        .current_dir(temp.path())
        .arg("show")
        .arg("technical/app")
        .output()
        .expect("run arche show after doc");
    let stdout = String::from_utf8(show.stdout).unwrap();
    assert!(stdout.contains("position: 应用入口。"));
    assert!(stdout.contains("technical/app/module"));

    let add = arche()
        .current_dir(temp.path())
        .arg("add")
        .arg("-A")
        .output()
        .expect("run arche add");
    assert!(add.status.success(), "{:?}", add);

    let commit = arche()
        .current_dir(temp.path())
        .arg("commit")
        .arg("-m")
        .arg("add app")
        .output()
        .expect("run arche commit");
    assert!(commit.status.success(), "{:?}", commit);

    let log = arche()
        .current_dir(temp.path())
        .arg("log")
        .output()
        .expect("run arche log");
    let stdout = String::from_utf8(log.stdout).unwrap();
    assert!(stdout.contains("add app"));

    let comment = arche()
        .current_dir(temp.path())
        .arg("comment")
        .arg("add")
        .arg("technical/app")
        .arg("补充错误码")
        .output()
        .expect("run arche comment add");
    assert!(comment.status.success(), "{:?}", comment);

    let comments = arche()
        .current_dir(temp.path())
        .arg("comment")
        .arg("list")
        .output()
        .expect("run arche comment list");
    let stdout = String::from_utf8(comments.stdout).unwrap();
    assert!(stdout.contains("1\topen\ttechnical/app\t补充错误码"));
    assert!(stdout.contains("补充错误码"));

    let resolved = arche()
        .current_dir(temp.path())
        .arg("comment")
        .arg("resolve")
        .arg("1")
        .arg("--conclusion")
        .arg("已补充")
        .arg("--agent")
        .arg("reviewer-agent")
        .output()
        .expect("run arche comment resolve");
    assert!(resolved.status.success(), "{:?}", resolved);

    let comments = arche()
        .current_dir(temp.path())
        .arg("comment")
        .arg("list")
        .output()
        .expect("run arche comment list after resolve");
    let stdout = String::from_utf8(comments.stdout).unwrap();
    assert!(stdout.contains("1\tresolved\ttechnical/app\t补充错误码"));
    assert!(stdout.contains("conclusion (by reviewer-agent): 已补充"));

    let concept = arche()
        .current_dir(temp.path())
        .arg("concept")
        .arg("create")
        .arg("concept/domain/agent")
        .arg("--name")
        .arg("智能体")
        .arg("--explanation")
        .arg("现实中的智能机器")
        .output()
        .expect("run arche concept create");
    assert!(concept.status.success(), "{:?}", concept);

    let concepts = arche()
        .current_dir(temp.path())
        .arg("concept")
        .arg("list")
        .output()
        .expect("run arche concept list");
    let stdout = String::from_utf8(concepts.stdout).unwrap();
    assert!(stdout.contains("concept/domain/agent"));
    assert!(stdout.contains("智能体"));

    let system = arche()
        .current_dir(temp.path())
        .arg("requirement")
        .arg("create")
        .arg("system")
        .arg("仓库系统")
        .output()
        .expect("run arche requirement create system");
    assert!(system.status.success(), "{:?}", system);

    let module = arche()
        .current_dir(temp.path())
        .arg("requirement")
        .arg("create")
        .arg("module")
        .arg("--parent")
        .arg("requirement/system/仓库系统")
        .arg("订单管理")
        .output()
        .expect("run arche requirement create module");
    assert!(module.status.success(), "{:?}", module);

    let list = arche()
        .current_dir(temp.path())
        .arg("requirement")
        .arg("list")
        .output()
        .expect("run arche requirement list");
    let stdout = String::from_utf8(list.stdout).unwrap();
    assert!(stdout.contains("RequirementSystem"));
    assert!(stdout.contains("RequirementModule"));
}
