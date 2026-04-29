//! Shared integer literal parsing bounds for lexer→checker→bytecode.
//! VM represents signed ints as [`i128`] and unsigned as [`u128`].

use crate::analyzer::Type;

/// Parses digits after semantic analysis enforced per-type bounds.
pub fn literal_u128(raw: &str) -> Result<u128, &'static str> {
    raw.parse::<u128>().map_err(|_| "invalid integer literal")
}

/// Maps declarator/synthesized integral type plus raw digits to bytecode constants.
///
/// Unary minus is not part of literals; negatives use unary [`crate::hir::HirExpr::Unary`].
pub fn bytecode_int_from_checked_literal(raw: &str, ty: &Type) -> Result<IntConst, &'static str> {
    let parsed = literal_u128(raw)?;
    match ty {
        Type::Int {
            signed: true, ..
        } => {
            if parsed > i128::MAX as u128 {
                return Err("literal out of signed range");
            }
            Ok(IntConst::Signed(parsed as i128))
        }
        Type::Int {
            signed: false, ..
        } => Ok(IntConst::Unsigned(parsed)),
        _ => Err("integer literal has non-int type"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntConst {
    Signed(i128),
    Unsigned(u128),
}
