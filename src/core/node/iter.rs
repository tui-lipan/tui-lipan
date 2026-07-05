use super::id::NodeId;
use super::tree::{Node, NodeTree};

pub(crate) struct NodeDfsIter<'a> {
    pub(crate) tree: &'a NodeTree,
    pub(crate) stack: Vec<NodeId>,
}

impl<'a> Iterator for NodeDfsIter<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.stack.pop()?;
            if !self.tree.is_valid(id) {
                continue;
            }

            let node = self.tree.node(id);
            for &child in node.children.iter().rev() {
                if self.tree.is_valid(child) {
                    self.stack.push(child);
                }
            }
            return Some(node);
        }
    }
}
