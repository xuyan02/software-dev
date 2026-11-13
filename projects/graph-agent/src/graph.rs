use crate::{Context, LlmNode, LlmNodeAdapter, Node, NodeError, NodeId, TransitionIndex};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// A configured binding from `(node, local target index)` to a target node.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: NodeId,
    pub target: TransitionIndex,
    pub to: NodeId,
}

/// An assembled, validated agent graph.
pub struct Graph {
    id: String,
    entry: NodeId,
    exits: BTreeSet<NodeId>,
    nodes: BTreeMap<NodeId, Arc<dyn Node>>,
    edges: BTreeMap<(NodeId, TransitionIndex), NodeId>,
}

impl Graph {
    pub fn builder(id: impl Into<String>) -> GraphBuilder {
        GraphBuilder::new(id)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn entry(&self) -> &NodeId {
        &self.entry
    }

    /// Execute until an exit is reached.
    ///
    /// Exits are virtual: they are not executable nodes, and reaching one ends
    /// the run.
    pub async fn run(&self, ctx: &mut Context) -> Result<NodeId, GraphError> {
        let mut current = self.entry.clone();

        loop {
            let node = self
                .nodes
                .get(&current)
                .ok_or_else(|| GraphError::NodeNotFound(current.clone()))?;

            ctx.set_current_node(Some(current.clone()));
            node.run(ctx).await?;

            let index = node.to(ctx)?;
            let next = self.edges.get(&(current.clone(), index)).ok_or_else(|| {
                GraphError::MissingEdge {
                    node: current.clone(),
                    index,
                }
            })?;

            if self.exits.contains(next) {
                return Ok(next.clone());
            }

            current = next.clone();
        }
    }
}

pub struct GraphBuilder {
    id: String,
    entry: Option<NodeId>,
    exits: BTreeSet<NodeId>,
    nodes: Vec<(NodeId, Arc<dyn Node>)>,
    edges: Vec<Edge>,
}

impl GraphBuilder {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            entry: None,
            exits: BTreeSet::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn entry(mut self, id: impl Into<NodeId>) -> Self {
        self.entry = Some(id.into());
        self
    }

    pub fn exit(mut self, id: impl Into<NodeId>) -> Self {
        self.exits.insert(id.into());
        self
    }

    pub fn node(mut self, id: impl Into<NodeId>, node: impl Node + 'static) -> Self {
        self.nodes.push((id.into(), Arc::new(node)));
        self
    }

    /// Register an LLM node. The builder wraps it in the internal adapter.
    pub fn llm_node(mut self, id: impl Into<NodeId>, node: impl LlmNode + 'static) -> Self {
        let adapter = LlmNodeAdapter::new(node);
        self.nodes.push((id.into(), Arc::new(adapter)));
        self
    }

    pub fn edge(
        mut self,
        from: impl Into<NodeId>,
        target: impl Into<TransitionIndex>,
        to: impl Into<NodeId>,
    ) -> Self {
        self.edges.push(Edge {
            from: from.into(),
            target: target.into(),
            to: to.into(),
        });
        self
    }

    pub fn build(self) -> Result<Graph, GraphError> {
        let entry = self.entry.ok_or(GraphError::MissingEntry)?;
        if self.exits.is_empty() {
            return Err(GraphError::NoExits);
        }

        let mut nodes = BTreeMap::new();
        for (id, node) in self.nodes {
            if nodes.insert(id.clone(), node).is_some() {
                return Err(GraphError::DuplicateNode(id));
            }
        }

        if !nodes.contains_key(&entry) {
            return Err(GraphError::EntryNotFound(entry));
        }
        if self.exits.contains(&entry) {
            return Err(GraphError::EntryIsExit(entry));
        }
        for exit in &self.exits {
            if nodes.contains_key(exit) {
                return Err(GraphError::NodeNameConflictsWithExit(exit.clone()));
            }
        }

        let mut edges = BTreeMap::new();
        for edge in self.edges {
            if self.exits.contains(&edge.from) {
                return Err(GraphError::ExitHasOutgoingEdge(edge.from));
            }

            let from_node = nodes
                .get(&edge.from)
                .ok_or_else(|| GraphError::UnknownEdgeFrom(edge.from.clone()))?;

            if !nodes.contains_key(&edge.to) && !self.exits.contains(&edge.to) {
                return Err(GraphError::UnknownEdgeTo(edge.to));
            }

            let count = from_node.transition_count();
            if edge.target.0 >= count {
                return Err(GraphError::TransitionOutOfRange {
                    node: edge.from,
                    index: edge.target,
                    count,
                });
            }

            if edges
                .insert((edge.from.clone(), edge.target), edge.to)
                .is_some()
            {
                return Err(GraphError::DuplicateEdge {
                    node: edge.from,
                    index: edge.target,
                });
            }
        }

        // Every node must have a binding for every local target.
        for (id, node) in &nodes {
            let count = node.transition_count();
            if count == 0 {
                return Err(GraphError::NodeHasNoTransitions(id.clone()));
            }
            for index in 0..count {
                let index = TransitionIndex(index);
                if !edges.contains_key(&(id.clone(), index)) {
                    return Err(GraphError::MissingEdge {
                        node: id.clone(),
                        index,
                    });
                }
            }
        }

        // Every node must be reachable from the entry node.
        let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for ((from, _), to) in &edges {
            adjacency.entry(from.clone()).or_default().push(to.clone());
        }

        let mut reachable = BTreeSet::new();
        let mut queue = VecDeque::from([entry.clone()]);
        while let Some(id) = queue.pop_front() {
            if !reachable.insert(id.clone()) {
                continue;
            }
            if let Some(targets) = adjacency.get(&id) {
                for target in targets {
                    if !reachable.contains(target) {
                        queue.push_back(target.clone());
                    }
                }
            }
        }
        for id in nodes.keys() {
            if !reachable.contains(id) {
                return Err(GraphError::UnreachableNode(id.clone()));
            }
        }
        for exit in &self.exits {
            if !reachable.contains(exit) {
                return Err(GraphError::UnreachableExit(exit.clone()));
            }
        }

        Ok(Graph {
            id: self.id,
            entry,
            exits: self.exits,
            nodes,
            edges,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph has no entry node")]
    MissingEntry,

    #[error("graph must declare at least one exit")]
    NoExits,

    #[error("duplicate node: {0}")]
    DuplicateNode(NodeId),

    #[error("entry node not found: {0}")]
    EntryNotFound(NodeId),

    #[error("node name conflicts with exit name: {0}")]
    NodeNameConflictsWithExit(NodeId),

    #[error("entry node cannot be an exit: {0}")]
    EntryIsExit(NodeId),

    #[error("edge from unknown node: {0}")]
    UnknownEdgeFrom(NodeId),

    #[error("edge to unknown node or exit: {0}")]
    UnknownEdgeTo(NodeId),

    #[error("exit {0} cannot have outgoing edges")]
    ExitHasOutgoingEdge(NodeId),

    #[error("transition index {index} out of range for node {node} (count {count})")]
    TransitionOutOfRange {
        node: NodeId,
        index: TransitionIndex,
        count: usize,
    },

    #[error("node {0} has no transitions but is not an exit")]
    NodeHasNoTransitions(NodeId),

    #[error("duplicate edge: {node}[{index}]")]
    DuplicateEdge {
        node: NodeId,
        index: TransitionIndex,
    },

    #[error("missing edge for node {node} target {index}")]
    MissingEdge {
        node: NodeId,
        index: TransitionIndex,
    },

    #[error("unreachable node: {0}")]
    UnreachableNode(NodeId),

    #[error("unreachable exit: {0}")]
    UnreachableExit(NodeId),

    #[error("current node not found: {0}")]
    NodeNotFound(NodeId),

    #[error(transparent)]
    Node(#[from] NodeError),
}
