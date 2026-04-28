use std::collections::HashMap;

use crate::analyzer::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub symbols: HashMap<String, SymbolId>,
    pub parent: Option<ScopeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolOrigin {
    User,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub ty: Type,
    pub origin: SymbolOrigin,
    pub scope: ScopeId,
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_scope(&mut self, parent: Option<ScopeId>) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope {
            symbols: HashMap::new(),
            parent,
        });
        id
    }

    pub fn define(&mut self, scope_id: ScopeId, name: String, ty: Type, origin: SymbolOrigin) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            id,
            name: name.clone(),
            ty,
            origin,
            scope: scope_id,
        });
        self.scopes[scope_id.0 as usize].symbols.insert(name, id);
        id
    }

    pub fn resolve(&self, mut scope_id: ScopeId, name: &str) -> Option<SymbolId> {
        loop {
            let scope = self.scopes.get(scope_id.0 as usize)?;
            if let Some(symbol_id) = scope.symbols.get(name) {
                return Some(*symbol_id);
            }
            match scope.parent {
                Some(parent) => scope_id = parent,
                None => return None,
            }
        }
    }

    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.0 as usize)
    }

    pub fn symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(id.0 as usize)
    }

    pub fn resolve_in_scope(&self, scope_id: ScopeId, name: &str) -> Option<SymbolId> {
        let scope = self.scopes.get(scope_id.0 as usize)?;
        scope.symbols.get(name).copied()
    }
}
