use std::process::Command;

use tempfile::TempDir;

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
    assert!(stdout.contains("technical/app/modules"));

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
    assert!(stdout.contains("comments.1"));
    assert!(stdout.contains("补充错误码"));

    let concept = arche()
        .current_dir(temp.path())
        .arg("concept")
        .arg("create")
        .arg("concepts/domain/agent")
        .arg("--name")
        .arg("智能体")
        .arg("--summary")
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
    assert!(stdout.contains("concepts/domain/agent"));
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
        .arg("requirements/systems/仓库系统")
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
