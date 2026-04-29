use std::collections::HashMap;

use crate::{
    analyzer::Type,
    ast::{Ast, AstNode, BinaryOp, CompoundOp, UnaryOp},
    builtins::default_registry,
    hir::{BinOp, Hir, HirExpr, HirStmt, ScopeId, SymbolId, SymbolOrigin, SymbolTable, UnaryOp as HirUnaryOp},
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
                expr_spans: HashMap::new(),
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
                ty,
                value,
                is_const,
                span,
                ..
            } => {
                let value = self.lower_expr(*value);
                let symbol_ty = self.resolve_decl_type(*ty);
                let symbol = self.bind_symbol(*name, symbol_ty, *is_const);
                HirStmt::Let {
                    symbol,
                    value,
                    is_const: *is_const,
                    span: *span,
                }
            }
            AstNode::AssignStmt { target, value, span } => {
                let value = self.lower_expr(*value);
                let symbol = self.resolve_assignment_target(*target);
                HirStmt::Assign {
                    symbol,
                    value,
                    span: *span,
                }
            }
            AstNode::CompoundAssignStmt {
                target,
                op,
                value,
                span,
            } => {
                let symbol = self.resolve_assignment_target(*target);
                let rhs = self.lower_expr(*value);
                let lhs = self.insert_hir_expr(HirExpr::Var(symbol), self.node_span(*target));
                let binary = self.insert_hir_expr(
                    HirExpr::Binary {
                        op: map_compound_op(*op),
                        lhs,
                        rhs,
                    },
                    *span,
                );
                HirStmt::Assign {
                    symbol,
                    value: binary,
                    span: *span,
                }
            }
            AstNode::ExprStmt { expr, span } => {
                let expr = self.lower_expr(*expr);
                HirStmt::Expr { expr, span: *span }
            }
            AstNode::BlockStmt { statements, span } => {
                let stmts = self.lower_block_stmts(statements);
                HirStmt::Block { stmts, span: *span }
            }
            AstNode::IfStmt {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let condition = self.lower_expr(*condition);
                let then_branch = self.lower_if_branch(*then_branch);
                let else_branch = else_branch.map(|id| self.lower_if_branch(id));
                HirStmt::If {
                    condition,
                    then_branch,
                    else_branch,
                    span: *span,
                }
            }
            AstNode::WhileStmt {
                condition,
                body,
                span,
            } => {
                let condition = self.lower_expr(*condition);
                let body = self.lower_if_branch(*body);
                HirStmt::While {
                    condition,
                    body,
                    span: *span,
                }
            }
            AstNode::BreakStmt { span } => HirStmt::Break { span: *span },
            AstNode::ContinueStmt { span } => HirStmt::Continue { span: *span },
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

    fn bind_symbol(&mut self, id: ArenaId, ty: Type, is_const: bool) -> SymbolId {
        let Some(AstNode::Identifier { name, .. }) = self.ast.get(id) else {
            self.push_error("invalid declaration name", self.node_span(id));
            return self.symbols.define(
                self.current_scope,
                "<invalid>".to_string(),
                ty,
                SymbolOrigin::User,
                is_const,
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
            ty,
            SymbolOrigin::User,
            is_const,
        )
    }

    fn resolve_assignment_target(&mut self, id: ArenaId) -> SymbolId {
        let Some(AstNode::Identifier { name, span }) = self.ast.get(id) else {
            self.push_error("invalid assignment target", self.node_span(id));
            return self
                .symbols
                .define(self.current_scope, "<invalid_assign>".to_string(), Type::Unknown, SymbolOrigin::User, false);
        };
        match self.symbols.resolve(self.current_scope, name) {
            Some(symbol_id) => symbol_id,
            None => {
                self.push_error(format!("undefined symbol '{name}'"), *span);
                self.symbols
                    .define(self.current_scope, "<undefined_assign>".to_string(), Type::Unknown, SymbolOrigin::User, false)
            }
        }
    }

    fn lower_expr(&mut self, id: ArenaId) -> ArenaId {
        let Some(node) = self.ast.get(id) else {
            return self.insert_hir_expr(
                HirExpr::Invalid,
                Span::new_unchecked(self.ast.file_id, 0, 0),
            );
        };

        match node {
            AstNode::IntegerLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Int(value.clone()), self.node_span(id))
            }
            AstNode::FloatLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Float(value.clone()), self.node_span(id))
            }
            AstNode::StringLiteral { value, .. } => {
                let value = unquote_string_literal(value);
                self.insert_hir_expr(HirExpr::Str(value), self.node_span(id))
            }
            AstNode::CharLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Char(*value), self.node_span(id))
            }
            AstNode::BoolLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Bool(*value), self.node_span(id))
            }
            AstNode::UnaryExpr { op, operand, .. } => {
                let operand = self.lower_expr(*operand);
                let op = match op {
                    UnaryOp::Neg => HirUnaryOp::Neg,
                    UnaryOp::Not => HirUnaryOp::Not,
                    UnaryOp::BitNot => HirUnaryOp::BitNot,
                };
                self.insert_hir_expr(HirExpr::Unary { op, operand }, self.node_span(id))
            }
            AstNode::Identifier { name, span } => match self.symbols.resolve(self.current_scope, name) {
                Some(symbol_id) => self.insert_hir_expr(HirExpr::Var(symbol_id), self.node_span(id)),
                None => {
                    self.push_error(format!("undefined symbol '{name}'"), *span);
                    self.insert_hir_expr(HirExpr::Invalid, *span)
                }
            },
            AstNode::BinaryExpr {
                op, left, right, ..
            } => {
                let lhs = self.lower_expr(*left);
                let rhs = self.lower_expr(*right);
                self.insert_hir_expr(
                    HirExpr::Binary {
                        op: map_binary_op(*op),
                        lhs,
                        rhs,
                    },
                    self.node_span(id),
                )
            }
            AstNode::CallExpr { callee, args, span } => {
                let lowered_args = args.iter().map(|arg| self.lower_expr(*arg)).collect::<Vec<_>>();
                let callee_expr = self.lower_expr(*callee);
                let callee_symbol = match self.hir.exprs.get(callee_expr) {
                    Some(HirExpr::Var(symbol_id)) => Some(*symbol_id),
                    _ => None,
                };
                match callee_symbol {
                    Some(symbol_id) => self.insert_hir_expr(
                        HirExpr::Call {
                            callee: symbol_id,
                            args: lowered_args,
                        },
                        self.node_span(id),
                    ),
                    None => {
                        self.push_error("call target must be an identifier", *span);
                        self.insert_hir_expr(HirExpr::Invalid, *span)
                    }
                }
            }
            AstNode::TypeName { .. } => {
                self.push_error("type name is not an expression", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid, self.node_span(id))
            }
            AstNode::LetDecl { .. }
            | AstNode::AssignStmt { .. }
            | AstNode::CompoundAssignStmt { .. }
            | AstNode::IfStmt { .. }
            | AstNode::WhileStmt { .. }
            | AstNode::BreakStmt { .. }
            | AstNode::ContinueStmt { .. }
            | AstNode::ExprStmt { .. }
            | AstNode::BlockStmt { .. } => {
                self.push_error("statement used where expression expected", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid, self.node_span(id))
            }
            AstNode::Invalid { .. } => self.insert_hir_expr(HirExpr::Invalid, self.node_span(id)),
        }
    }

    fn insert_hir_expr(&mut self, expr: HirExpr, span: Span) -> ArenaId {
        let id = self
            .hir
            .exprs
            .insert(expr)
            .expect("hir arena insertion should not fail in normal conditions");
        self.hir.expr_spans.insert(id, span);
        id
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

    fn resolve_decl_type(&mut self, ty: Option<ArenaId>) -> Type {
        let Some(ty_id) = ty else {
            return Type::Unknown;
        };
        match self.ast.get(ty_id) {
            Some(AstNode::TypeName { name, span }) => match map_type_name(name) {
                Some(ty) => ty,
                None => {
                    self.push_error(format!("unknown type '{name}'"), *span);
                    Type::Unknown
                }
            },
            _ => Type::Unknown,
        }
    }

    fn lower_block_stmts(&mut self, statements: &[ArenaId]) -> Vec<HirStmt> {
        let parent_scope = self.current_scope;
        let block_scope = self.symbols.create_scope(Some(parent_scope));
        self.current_scope = block_scope;
        let lowered = statements
            .iter()
            .map(|stmt_id| self.lower_stmt(*stmt_id))
            .collect::<Vec<_>>();
        self.current_scope = parent_scope;
        lowered
    }

    fn lower_if_branch(&mut self, branch_id: ArenaId) -> Vec<HirStmt> {
        match self.ast.get(branch_id) {
            Some(AstNode::BlockStmt { statements, .. }) => self.lower_block_stmts(statements),
            Some(AstNode::IfStmt { .. }) => vec![self.lower_stmt(branch_id)],
            Some(AstNode::Invalid { span }) => {
                self.push_error("invalid if branch", *span);
                vec![HirStmt::Invalid { span: *span }]
            }
            _ => {
                self.push_error("invalid if branch", self.node_span(branch_id));
                vec![HirStmt::Invalid {
                    span: self.node_span(branch_id),
                }]
            }
        }
    }
}

fn map_binary_op(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Subtract => BinOp::Subtract,
        BinaryOp::Multiply => BinOp::Multiply,
        BinaryOp::Divide => BinOp::Divide,
        BinaryOp::Modulo => BinOp::Modulo,
        BinaryOp::Power => BinOp::Power,
        BinaryOp::Equal => BinOp::Equal,
        BinaryOp::NotEqual => BinOp::NotEqual,
        BinaryOp::Less => BinOp::Less,
        BinaryOp::LessEqual => BinOp::LessEqual,
        BinaryOp::Greater => BinOp::Greater,
        BinaryOp::GreaterEqual => BinOp::GreaterEqual,
        BinaryOp::LogicalAnd => BinOp::LogicalAnd,
        BinaryOp::LogicalOr => BinOp::LogicalOr,
        BinaryOp::BitAnd => BinOp::BitAnd,
        BinaryOp::BitOr => BinOp::BitOr,
        BinaryOp::BitXor => BinOp::BitXor,
        BinaryOp::ShiftLeft => BinOp::ShiftLeft,
        BinaryOp::ShiftRight => BinOp::ShiftRight,
    }
}

fn map_compound_op(op: CompoundOp) -> BinOp {
    match op {
        CompoundOp::Add => BinOp::Add,
        CompoundOp::Subtract => BinOp::Subtract,
        CompoundOp::Multiply => BinOp::Multiply,
        CompoundOp::Divide => BinOp::Divide,
        CompoundOp::Modulo => BinOp::Modulo,
        CompoundOp::Power => BinOp::Power,
        CompoundOp::BitAnd => BinOp::BitAnd,
        CompoundOp::BitOr => BinOp::BitOr,
        CompoundOp::BitXor => BinOp::BitXor,
        CompoundOp::ShiftLeft => BinOp::ShiftLeft,
        CompoundOp::ShiftRight => BinOp::ShiftRight,
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
            true,
        );
    }
    scope_id
}

/// Lexer includes surrounding `"`; runtime `print` expects the decoded content (no delimiter).
fn unquote_string_literal(raw: &str) -> String {
    let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return raw.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn map_type_name(name: &str) -> Option<Type> {
    match name {
        "i1" => Some(Type::Int { signed: true, bits: 1 }),
        "i8" => Some(Type::Int { signed: true, bits: 8 }),
        "i16" => Some(Type::Int { signed: true, bits: 16 }),
        "i32" => Some(Type::Int { signed: true, bits: 32 }),
        "i64" => Some(Type::Int { signed: true, bits: 64 }),
        "i128" => Some(Type::Int { signed: true, bits: 128 }),
        "u1" => Some(Type::Int { signed: false, bits: 1 }),
        "u8" => Some(Type::Int { signed: false, bits: 8 }),
        "u16" => Some(Type::Int { signed: false, bits: 16 }),
        "u32" => Some(Type::Int { signed: false, bits: 32 }),
        "u64" => Some(Type::Int { signed: false, bits: 64 }),
        "u128" => Some(Type::Int { signed: false, bits: 128 }),
        "f32" => Some(Type::Float { bits: 32 }),
        "f64" => Some(Type::Float { bits: 64 }),
        "str" => Some(Type::Str),
        "bool" => Some(Type::Bool),
        "char" => Some(Type::Char),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{
        hir::{BinOp, HirExpr, HirStmt, ScopeId, SymbolOrigin},
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

    #[test]
    fn lowers_builtin_call_into_hir_call() {
        let src = "print(1, 2)";
        let lex_output = lex(FileId::from_u32(5), src);
        let (ast, _) = parse(FileId::from_u32(5), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());

        let mut has_call = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Call { .. })) {
                has_call = true;
                break;
            }
        }
        assert!(has_call);
    }

    #[test]
    fn block_scope_allows_shadowing_without_leaking() {
        let src = "x := 1; { x := 2; y := x }; z := x";
        let lex_output = lex(FileId::from_u32(6), src);
        let (ast, _) = parse(FileId::from_u32(6), src.len() as u32, lex_output.tokens);
        let (_hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn block_local_is_not_visible_outside() {
        let src = "{ local := 1 }; print(local)";
        let lex_output = lex(FileId::from_u32(7), src);
        let (ast, _) = parse(FileId::from_u32(7), src.len() as u32, lex_output.tokens);
        let (_hir, _symbols, diagnostics) = lower(&ast);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("undefined symbol 'local'")));
    }

    #[test]
    fn lowers_new_binary_ops_into_hir() {
        let src = "x := (1 % 2) == 1 && true || false";
        let lex_output = lex(FileId::from_u32(8), src);
        let (ast, _) = parse(FileId::from_u32(8), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let mut has_mod = false;
        let mut has_eq = false;
        let mut has_and = false;
        let mut has_or = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if let Some(HirExpr::Binary { op, .. }) = hir.exprs.get(id) {
                has_mod |= matches!(op, BinOp::Modulo);
                has_eq |= matches!(op, BinOp::Equal);
                has_and |= matches!(op, BinOp::LogicalAnd);
                has_or |= matches!(op, BinOp::LogicalOr);
            }
        }
        assert!(has_mod && has_eq && has_and && has_or);
    }

    #[test]
    fn lowers_new_unary_ops_into_hir() {
        let src = "x := !false; y := ~1";
        let lex_output = lex(FileId::from_u32(9), src);
        let (ast, _) = parse(FileId::from_u32(9), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let mut has_not = false;
        let mut has_bit_not = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if let Some(HirExpr::Unary { op, .. }) = hir.exprs.get(id) {
                has_not |= matches!(op, crate::hir::UnaryOp::Not);
                has_bit_not |= matches!(op, crate::hir::UnaryOp::BitNot);
            }
        }
        assert!(has_not && has_bit_not);
    }

    #[test]
    fn lowers_if_else_stmt_into_hir_if() {
        let src = "if true { x := 1 } else { x := 2 }";
        let lex_output = lex(FileId::from_u32(10), src);
        let (ast, _) = parse(FileId::from_u32(10), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert!(matches!(hir.stmts.first(), Some(HirStmt::If { .. })));
    }

    #[test]
    fn lowers_while_break_continue_into_hir() {
        let src = "while 1 { continue; break }";
        let lex_output = lex(FileId::from_u32(11), src);
        let (ast, _) = parse(FileId::from_u32(11), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let Some(HirStmt::While { body, .. }) = hir.stmts.first() else {
            panic!("expected while");
        };
        assert!(body.iter().any(|s| matches!(s, HirStmt::Continue { .. })));
        assert!(body.iter().any(|s| matches!(s, HirStmt::Break { .. })));
    }

    #[test]
    fn lowers_compound_assign_to_binary_assign() {
        let src = "x := 1; x += 2";
        let lex_output = lex(FileId::from_u32(12), src);
        let (ast, _) = parse(FileId::from_u32(12), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let Some(HirStmt::Assign { value, .. }) = hir.stmts.get(1) else {
            panic!("expected assign stmt");
        };
        assert!(matches!(
            hir.exprs.get(*value),
            Some(HirExpr::Binary { op: BinOp::Add, .. })
        ));
    }
}
