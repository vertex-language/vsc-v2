vsc_index::newtype_index! {
    /// Identifies an AST node. Nodes start out with `DUMMY_NODE_ID`; real ids
    /// are assigned once expansion has built the crate's final tree.
    pub struct NodeId {}
}

/// The id of a node that hasn't been numbered yet.
pub const DUMMY_NODE_ID: NodeId = NodeId::MAX;

/// The id of the crate root.
pub const CRATE_NODE_ID: NodeId = NodeId::ZERO;
