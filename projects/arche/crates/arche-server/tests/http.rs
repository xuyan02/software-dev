use std::net::TcpListener;
use std::thread;

use arche_core::Repository;
use arche_server::{ServerOptions, serve_with_listener};
use tempfile::TempDir;

#[test]
fn clones_pushes_and_fetches_over_http() {
    let root = TempDir::new().unwrap();
    let server_repo = root.path().join("project");
    let repo =
        Repository::init_with_source(&server_repo, Some("git@example.test:app.git")).unwrap();
    repo.create_technical_system("app").unwrap();
    repo.add_all().unwrap();
    repo.commit_index("init", true).unwrap();
    drop(repo);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let options = ServerOptions::new(root.path()).protected(Vec::new());
    thread::spawn(move || {
        let _ = serve_with_listener(&options, listener);
    });
    let url = format!("http://{address}/project");

    let first = TempDir::new().unwrap();
    let clone = arche_client::clone_from(&url, first.path(), Some("main")).unwrap();
    assert_eq!(clone.source_url(), Some("git@example.test:app.git"));
    assert!(clone.find_by_qname("technical/app").is_ok());
    clone.create_technical_system("second").unwrap();
    clone.add_all().unwrap();
    clone.commit_index("add second", true).unwrap();
    let comment = clone.comment_add("technical/app", "补充错误码").unwrap();
    arche_client::push(&clone, false).unwrap();

    let pushed = Repository::open(&server_repo).unwrap();
    let server_head = pushed.ref_hash("refs/heads/main").unwrap().unwrap();
    assert_eq!(server_head, clone.head_commit().unwrap());
    assert_eq!(
        pushed.comment_list_on("main", None, false).unwrap()[0].body,
        "补充错误码"
    );

    let second = TempDir::new().unwrap();
    let fetched = arche_client::clone_from(&url, second.path(), Some("main")).unwrap();
    assert_eq!(fetched.source_url(), Some("git@example.test:app.git"));
    assert!(fetched.find_by_qname("technical/second").is_ok());
    fetched
        .comment_resolve(comment.id, "已补充", "reviewer-agent")
        .unwrap();
    arche_client::push(&fetched, false).unwrap();

    let refreshed = TempDir::new().unwrap();
    let fetched_again = arche_client::clone_from(&url, refreshed.path(), Some("main")).unwrap();
    let comments = fetched_again.comment_list(None, false).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].resolved);
    assert_eq!(comments[0].conclusion, "已补充");
}
