//! Shared integer literal parsing bounds for lexer→checker→bytecode.
//! VM represents signed ints as [`i128`] and unsigned as [`u128`].

use crate::analyzer::Type;

/// Parses digits after semantic analysis enforced per-type bounds.
pub fn literal_u128(raw: &str) -> Result<u128, &'static str> {
    let (digits, radix) = normalize_integer_literal(raw)?;
    u128::from_str_radix(&digits, radix).map_err(|_| "invalid integer literal")
}

pub fn literal_f64(raw: &str) -> Result<f64, &'static str> {
    let normalized = normalize_float_literal(raw)?;
    normalized
        .parse::<f64>()
        .map_err(|_| "invalid float literal")
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

fn normalize_integer_literal(raw: &str) -> Result<(String, u32), &'static str> {
    let (digits_raw, radix) = if let Some(rest) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        (rest, 16)
    } else if let Some(rest) = raw.strip_prefix("0o").or_else(|| raw.strip_prefix("0O")) {
        (rest, 8)
    } else if let Some(rest) = raw.strip_prefix("0b").or_else(|| raw.strip_prefix("0B")) {
        (rest, 2)
    } else {
        (raw, 10)
    };

    let mut normalized = String::with_capacity(digits_raw.len());
    let mut saw_digit = false;
    let mut prev_is_underscore = false;
    for ch in digits_raw.chars() {
        if ch == '_' {
            if !saw_digit || prev_is_underscore {
                return Err("invalid integer literal");
            }
            prev_is_underscore = true;
            continue;
        }
        if !is_digit_for_radix(ch, radix) {
            return Err("invalid integer literal");
        }
        normalized.push(ch);
        saw_digit = true;
        prev_is_underscore = false;
    }

    if !saw_digit || prev_is_underscore {
        return Err("invalid integer literal");
    }
    Ok((normalized, radix))
}

fn normalize_float_literal(raw: &str) -> Result<String, &'static str> {
    if raw.starts_with('_') || raw.ends_with('_') {
        return Err("invalid float literal");
    }
    let mut normalized = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if *ch != '_' {
            normalized.push(*ch);
            continue;
        }
        let prev = idx.checked_sub(1).and_then(|i| chars.get(i));
        let next = chars.get(idx + 1);
        if !prev.is_some_and(|c| c.is_ascii_digit()) || !next.is_some_and(|c| c.is_ascii_digit()) {
            return Err("invalid float literal");
        }
    }
    Ok(normalized)
}

fn is_digit_for_radix(ch: char, radix: u32) -> bool {
    match radix {
        2 => matches!(ch, '0' | '1'),
        8 => matches!(ch, '0'..='7'),
        10 => ch.is_ascii_digit(),
        16 => ch.is_ascii_hexdigit(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{literal_f64, literal_u128};

    #[test]
    fn parses_integer_bases_and_separators() {
        assert_eq!(literal_u128("0xFF").expect("hex"), 255);
        assert_eq!(literal_u128("0o755").expect("oct"), 493);
        assert_eq!(literal_u128("0b1010").expect("bin"), 10);
        assert_eq!(literal_u128("1_000_000").expect("sep"), 1_000_000);
    }

    #[test]
    fn rejects_invalid_integer_literals() {
        assert!(literal_u128("0x").is_err());
        assert!(literal_u128("0b102").is_err());
        assert!(literal_u128("1__0").is_err());
        assert!(literal_u128("1_").is_err());
    }

    #[test]
    fn parses_scientific_float_with_separator() {
        let v = literal_f64("6.02e23").expect("sci");
        assert!(v > 1e23);
        let v = literal_f64("1_000.5").expect("sep");
        assert_eq!(v, 1000.5);
    }
}
