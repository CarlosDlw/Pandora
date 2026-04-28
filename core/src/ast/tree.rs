use foundation::{arena::Arena, ids::ArenaId, ids::FileId};

use super::node::AstNode;

#[derive(Debug)]
pub struct Ast {
    pub file_id: FileId,
    pub roots: Vec<ArenaId>,
    pub arena: Arena<AstNode>,
}

impl Ast {
    pub fn get(&self, id: ArenaId) -> Option<&AstNode> {
        self.arena.get(id)
    }
}
