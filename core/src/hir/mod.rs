#![allow(clippy::module_inception)]

pub mod hir;
pub mod symbols;

pub use hir::{
    BinOp, Hir, HirArrayItem, HirExpr, HirId, HirStmt, IncDecOp, IncDecPosition, UnaryOp,
};
pub use symbols::{Scope, ScopeId, Symbol, SymbolId, SymbolOrigin, SymbolTable};
