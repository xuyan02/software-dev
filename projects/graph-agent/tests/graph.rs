use async_trait::async_trait;
use graph_agent::{Context, Graph, GraphBuilder, GraphError, Node, NodeError, TransitionIndex};

struct FixedNode {
    count: usize,
    next: TransitionIndex,
    output: &'static str,
}

#[async_trait]
impl Node for FixedNode {
    async fn run(&self, ctx: &mut Context) -> Result<(), NodeError> {
        ctx.set_output(self.output)?;
        Ok(())
    }

    fn to(&self, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(self.next)
    }

    fn transition_count(&self) -> usize {
        self.count
    }
}

struct BranchNode;

#[async_trait]
impl Node for BranchNode {
    async fn run(&self, _ctx: &mut Context) -> Result<(), NodeError> {
        Ok(())
    }

    fn to(&self, ctx: &Context) -> Result<TransitionIndex, NodeError> {
        if ctx.var::<bool>("go")? {
            Ok(TransitionIndex(0))
        } else {
            Ok(TransitionIndex(1))
        }
    }

    fn transition_count(&self) -> usize {
        2
    }
}

fn build_error(builder: GraphBuilder) -> GraphError {
    builder.build().err().expect("graph should fail to build")
}

fn fixed(count: usize, output: &'static str) -> FixedNode {
    FixedNode {
        count,
        next: TransitionIndex::ZERO,
        output,
    }
}

fn simple_graph() -> Graph {
    Graph::builder("simple")
        .entry("load")
        .exit("done")
        .node("load", fixed(1, "loaded"))
        .edge("load", 0, "done")
        .build()
        .expect("valid graph")
}

#[tokio::test]
async fn builds_and_runs_complete_graph() {
    let graph = simple_graph();
    let mut ctx = Context::new();

    let exit = graph.run(&mut ctx).await.expect("run");

    assert_eq!(exit.as_str(), "done");
    assert_eq!(ctx.node_output::<String>("load").unwrap(), "loaded");
}

#[tokio::test]
async fn follows_the_selected_transition_index() {
    let graph = Graph::builder("branch")
        .entry("branch")
        .exit("left")
        .exit("right")
        .node("branch", BranchNode)
        .edge("branch", 0, "left")
        .edge("branch", 1, "right")
        .build()
        .expect("valid graph");

    let mut ctx = Context::new();
    ctx.set_var("go", true).unwrap();
    assert_eq!(graph.run(&mut ctx).await.unwrap().as_str(), "left");

    let mut ctx = Context::new();
    ctx.set_var("go", false).unwrap();
    assert_eq!(graph.run(&mut ctx).await.unwrap().as_str(), "right");
}

#[test]
fn rejects_missing_edge() {
    let error = build_error(
        Graph::builder("missing")
            .entry("validate")
            .exit("approve")
            .node("validate", fixed(2, "validate"))
            .edge("validate", 0, "approve"),
    );

    assert!(matches!(
        error,
        GraphError::MissingEdge { ref node, index } if node.as_str() == "validate" && index.0 == 1
    ));
}

#[test]
fn rejects_out_of_range_transition() {
    let error = build_error(
        Graph::builder("range")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .edge("load", 1, "done"),
    );

    assert!(matches!(
        error,
        GraphError::TransitionOutOfRange { index, count, .. }
            if index.0 == 1 && count == 1
    ));
}

#[test]
fn rejects_exit_with_outgoing_edge() {
    let error = build_error(
        Graph::builder("exit-edge")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .edge("load", 0, "done")
            .edge("done", 0, "load"),
    );

    assert!(matches!(
        error,
        GraphError::ExitHasOutgoingEdge(ref node) if node.as_str() == "done"
    ));
}

#[test]
fn rejects_unreachable_node() {
    let error = build_error(
        Graph::builder("unreachable")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .node("orphan", fixed(1, "orphan"))
            .edge("load", 0, "done")
            .edge("orphan", 0, "done"),
    );

    assert!(matches!(
        error,
        GraphError::UnreachableNode(ref node) if node.as_str() == "orphan"
    ));
}

#[test]
fn rejects_unreachable_exit() {
    let error = build_error(
        Graph::builder("unreachable-exit")
            .entry("load")
            .exit("done")
            .exit("failed")
            .node("load", fixed(1, "load"))
            .edge("load", 0, "done"),
    );

    assert!(matches!(
        error,
        GraphError::UnreachableExit(ref exit) if exit.as_str() == "failed"
    ));
}

#[test]
fn rejects_duplicate_node() {
    let error = build_error(
        Graph::builder("duplicate")
            .entry("load")
            .exit("done")
            .node("load", fixed(0, "load"))
            .node("load", fixed(0, "load")),
    );

    assert!(matches!(
        error,
        GraphError::DuplicateNode(ref node) if node.as_str() == "load"
    ));
}

#[test]
fn rejects_missing_entry() {
    let error = build_error(
        Graph::builder("no-entry")
            .exit("done")
            .node("load", fixed(0, "load")),
    );

    assert!(matches!(error, GraphError::MissingEntry));
}

#[test]
fn rejects_missing_exit() {
    let error = build_error(
        Graph::builder("no-exit")
            .entry("load")
            .node("load", fixed(0, "load")),
    );

    assert!(matches!(error, GraphError::NoExits));
}

#[test]
fn rejects_non_exit_without_transitions() {
    let error = build_error(
        Graph::builder("no-transitions")
            .entry("load")
            .exit("done")
            .node("load", fixed(0, "load")),
    );

    assert!(matches!(
        error,
        GraphError::NodeHasNoTransitions(ref node) if node.as_str() == "load"
    ));
}

#[test]
fn rejects_duplicate_edge() {
    let error = build_error(
        Graph::builder("duplicate-edge")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .edge("load", 0, "done")
            .edge("load", 0, "done"),
    );

    assert!(matches!(
        error,
        GraphError::DuplicateEdge { ref node, index }
            if node.as_str() == "load" && index.0 == 0
    ));
}

#[test]
fn rejects_node_named_like_exit() {
    let error = build_error(
        Graph::builder("conflict")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .node("done", fixed(0, "done"))
            .edge("load", 0, "done"),
    );

    assert!(matches!(
        error,
        GraphError::NodeNameConflictsWithExit(ref node) if node.as_str() == "done"
    ));
}

#[test]
fn rejects_unknown_edge_target() {
    let error = build_error(
        Graph::builder("unknown-target")
            .entry("load")
            .exit("done")
            .node("load", fixed(1, "load"))
            .edge("load", 0, "missing"),
    );

    assert!(matches!(
        error,
        GraphError::UnknownEdgeTo(ref node) if node.as_str() == "missing"
    ));
}
