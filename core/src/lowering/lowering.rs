use crate::{
    ast::{Ast, AstNode, BinaryOp},
    builtins::default_registry,
    hir::{BinOp, Hir, HirExpr, HirStmt, ScopeId, SymbolId, SymbolOrigin, SymbolTable, Type},
};
use foundation::{
    arena::Arena,
    diagnostics::{Diagnostic, Diagnostics, Severity},
    ids::ArenaId,
    span::Span,
};

pub fn lower(ast: &Ast) -> (Hir, SymbolTable, Diagnostics) {
    let mut lowering = Lowering::new(ast);
    lowering.lower_program();
    (lowering.hir, lowering.symbols, lowering.diagnostics)
}

struct Lowering<'a> {
    ast: &'a Ast,
    hir: Hir,
    symbols: SymbolTable,
    diagnostics: Diagnostics,
    current_scope: ScopeId,
}

impl<'a> Lowering<'a> {
    fn new(ast: &'a Ast) -> Self {
        let mut symbols = SymbolTable::new();
        let root_scope = init_global_scope(&mut symbols, &default_registry());
        Self {
            ast,
            hir: Hir {
                file_id: ast.file_id,
                exprs: Arena::new(),
                stmts: Vec::new(),
            },
            symbols,
            diagnostics: Diagnostics::new(),
            current_scope: root_scope,
        }
    }

    fn lower_program(&mut self) {
        for root in &self.ast.roots {
            let stmt = self.lower_stmt(*root);
            self.hir.stmts.push(stmt);
        }
    }

    fn lower_stmt(&mut self, id: ArenaId) -> HirStmt {
        let Some(node) = self.ast.get(id) else {
            return HirStmt::Invalid {
                span: Span::new_unchecked(self.ast.file_id, 0, 0),
            };
        };

        match node {
            AstNode::LetDecl {
                name,
                value,
                is_const,
                span,
                ..
            } => {
                let symbol = self.bind_symbol(*name);
                let value = self.lower_expr(*value);
                HirStmt::Let {
                    symbol,
                    value,
                    is_const: *is_const,
                    span: *span,
                }
            }
            AstNode::ExprStmt { expr, span } => {
                let expr = self.lower_expr(*expr);
                HirStmt::Expr { expr, span: *span }
            }
            AstNode::Invalid { span } => HirStmt::Invalid { span: *span },
            other => {
                let expr = self.lower_expr(id);
                HirStmt::Expr {
                    expr,
                    span: other.span(),
                }
            }
        }
    }

    fn bind_symbol(&mut self, id: ArenaId) -> SymbolId {
        let Some(AstNode::Identifier { name, .. }) = self.ast.get(id) else {
            self.push_error("invalid declaration name", self.node_span(id));
            return self.symbols.define(
                self.current_scope,
                "<invalid>".to_string(),
                Type::Unknown,
                SymbolOrigin::User,
            );
        };

        if self.symbols.resolve_in_scope(self.current_scope, name).is_some() {
            self.push_error(
                format!("symbol '{name}' already defined in scope"),
                self.node_span(id),
            );
        }

        self.symbols.define(
            self.current_scope,
            name.clone(),
            Type::Unknown,
            SymbolOrigin::User,
        )
    }

    fn lower_expr(&mut self, id: ArenaId) -> ArenaId {
        let Some(node) = self.ast.get(id) else {
            return self.insert_hir_expr(HirExpr::Invalid);
        };

        match node {
            AstNode::IntegerLiteral { value, span } => match value.parse::<i64>() {
                Ok(value) => self.insert_hir_expr(HirExpr::Int(value)),
                Err(_) => {
                    self.push_error("integer literal out of range for i64", *span);
                    self.insert_hir_expr(HirExpr::Invalid)
                }
            },
            AstNode::Identifier { name, span } => match self.symbols.resolve(self.current_scope, name) {
                Some(symbol_id) => self.insert_hir_expr(HirExpr::Var(symbol_id)),
                None => {
                    self.push_error(format!("undefined symbol '{name}'"), *span);
                    self.insert_hir_expr(HirExpr::Invalid)
                }
            },
            AstNode::BinaryExpr {
                op, left, right, ..
            } => {
                let lhs = self.lower_expr(*left);
                let rhs = self.lower_expr(*right);
                self.insert_hir_expr(HirExpr::Binary {
                    op: map_binary_op(*op),
                    lhs,
                    rhs,
                })
            }
            AstNode::FloatLiteral { span, .. }
            | AstNode::StringLiteral { span, .. }
            | AstNode::BoolLiteral { span, .. } => {
                self.push_error("literal not supported in HIR yet", *span);
                self.insert_hir_expr(HirExpr::Invalid)
            }
            AstNode::TypeName { .. } => {
                self.push_error("type name is not an expression", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid)
            }
            AstNode::LetDecl { .. } | AstNode::ExprStmt { .. } => {
                self.push_error("statement used where expression expected", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid)
            }
            AstNode::Invalid { .. } => self.insert_hir_expr(HirExpr::Invalid),
        }
    }

    fn insert_hir_expr(&mut self, expr: HirExpr) -> ArenaId {
        self.hir
            .exprs
            .insert(expr)
            .expect("hir arena insertion should not fail in normal conditions")
    }

    fn node_span(&self, id: ArenaId) -> Span {
        self.ast
            .get(id)
            .map(AstNode::span)
            .unwrap_or_else(|| Span::new_unchecked(self.ast.file_id, 0, 0))
    }

    fn push_error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(message, span, Severity::Error));
    }
}

fn map_binary_op(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Subtract => BinOp::Subtract,
        BinaryOp::Multiply => BinOp::Multiply,
        BinaryOp::Divide => BinOp::Divide,
    }
}

fn init_global_scope(symbols: &mut SymbolTable, registry: &crate::builtins::BuiltinRegistry) -> ScopeId {
    let scope_id = symbols.create_scope(None);
    for builtin in &registry.items {
        symbols.define(
            scope_id,
            builtin.name.to_string(),
            builtin.ty.clone(),
            SymbolOrigin::Builtin,
        );
    }
    scope_id
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{
        hir::{HirExpr, ScopeId, SymbolOrigin},
        lexer::lex,
        parser::parse,
    };

    use super::lower;

    #[test]
    fn lowers_identifiers_to_symbols() {
        let src = "a := 1; b := a + 2";
        let lex_output = lex(FileId::from_u32(1), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(1), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());

        let (hir, symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert_eq!(hir.stmts.len(), 2);
        let b_symbol = symbols.resolve(ScopeId(0), "b");
        assert!(b_symbol.is_some());
    }

    #[test]
    fn reports_undefined_symbol() {
        let src = "a := b + 1";
        let lex_output = lex(FileId::from_u32(2), src);
        let (ast, _) = parse(FileId::from_u32(2), src.len() as u32, lex_output.tokens);

        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(diagnostics.has_errors());
        let mut has_invalid = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Invalid)) {
                has_invalid = true;
                break;
            }
        }
        assert!(has_invalid);
        assert!(diagnostics.iter().any(|d| d.message.contains("undefined symbol")));
    }

    #[test]
    fn hir_var_uses_symbol_id_not_string() {
        let src = "a := 1; b := a";
        let lex_output = lex(FileId::from_u32(3), src);
        let (ast, _) = parse(FileId::from_u32(3), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());

        let mut has_var = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Var(_))) {
                has_var = true;
                break;
            }
        }
        assert!(has_var);
    }

    #[test]
    fn global_scope_includes_print_builtin() {
        let src = "a := 1";
        let lex_output = lex(FileId::from_u32(4), src);
        let (ast, _) = parse(FileId::from_u32(4), src.len() as u32, lex_output.tokens);
        let (_hir, symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let print_id = symbols.resolve(ScopeId(0), "print").expect("builtin print");
        let print_symbol = symbols.symbol(print_id).expect("symbol exists");
        assert_eq!(print_symbol.origin, SymbolOrigin::Builtin);
    }
}
