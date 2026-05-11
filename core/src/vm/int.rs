#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntTag {
    I8,
    I16,
    I32,
    I64,
    I128,
    U1,
    U8,
    U16,
    U32,
    U64,
    U128,
}

impl IntTag {
    #[must_use]
    pub fn bits(self) -> u16 {
        match self {
            IntTag::I8 | IntTag::U8 => 8,
            IntTag::I16 | IntTag::U16 => 16,
            IntTag::I32 | IntTag::U32 => 32,
            IntTag::I64 | IntTag::U64 => 64,
            IntTag::I128 | IntTag::U128 => 128,
            IntTag::U1 => 1,
        }
    }

    #[must_use]
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            IntTag::I8 | IntTag::I16 | IntTag::I32 | IntTag::I64 | IntTag::I128
        )
    }

    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            IntTag::I8 => "i8",
            IntTag::I16 => "i16",
            IntTag::I32 => "i32",
            IntTag::I64 => "i64",
            IntTag::I128 => "i128",
            IntTag::U1 => "u1",
            IntTag::U8 => "u8",
            IntTag::U16 => "u16",
            IntTag::U32 => "u32",
            IntTag::U64 => "u64",
            IntTag::U128 => "u128",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntPayload {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedInt {
    tag: IntTag,
    payload: IntPayload,
}

impl TypedInt {
    pub fn try_new(tag: IntTag, payload: IntPayload) -> Result<Self, &'static str> {
        let value = Self { tag, payload };
        if value.is_in_range() {
            Ok(value)
        } else {
            Err("integer value out of range for target type")
        }
    }

    #[must_use]
    pub fn tag(self) -> IntTag {
        self.tag
    }

    #[must_use]
    pub fn payload(self) -> IntPayload {
        self.payload
    }

    #[must_use]
    pub fn type_name(self) -> &'static str {
        self.tag.type_name()
    }

    #[must_use]
    pub fn display_value(self) -> String {
        match self.payload {
            IntPayload::Signed(v) => v.to_string(),
            IntPayload::Unsigned(v) => v.to_string(),
        }
    }

    pub fn try_from_signed(tag: IntTag, value: i128) -> Result<Self, &'static str> {
        Self::try_new(tag, IntPayload::Signed(value))
    }

    pub fn try_from_unsigned(tag: IntTag, value: u128) -> Result<Self, &'static str> {
        Self::try_new(tag, IntPayload::Unsigned(value))
    }

    #[must_use]
    pub fn as_signed(self) -> Option<i128> {
        match self.payload {
            IntPayload::Signed(v) => Some(v),
            IntPayload::Unsigned(_) => None,
        }
    }

    #[must_use]
    pub fn as_unsigned(self) -> Option<u128> {
        match self.payload {
            IntPayload::Unsigned(v) => Some(v),
            IntPayload::Signed(_) => None,
        }
    }

    pub fn checked_neg(self) -> Result<Self, &'static str> {
        match self.payload {
            IntPayload::Signed(v) => {
                let out = v
                    .checked_neg()
                    .ok_or("integer overflow in unary negation")?;
                Self::try_from_signed(self.tag, out)
            }
            IntPayload::Unsigned(_) => Err("invalid operand for unary '-' (unsigned)"),
        }
    }

    pub fn bit_not(self) -> Result<Self, &'static str> {
        match self.payload {
            IntPayload::Signed(v) => Self::try_from_signed(self.tag, !v),
            IntPayload::Unsigned(v) => Self::try_from_unsigned(self.tag, !v),
        }
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| a.checked_add(b), |a, b| a.checked_add(b))
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| a.checked_sub(b), |a, b| a.checked_sub(b))
    }

    pub fn checked_mul(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| a.checked_mul(b), |a, b| a.checked_mul(b))
    }

    pub fn checked_div(self, rhs: Self) -> Result<Self, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(a), IntPayload::Signed(b)) => {
                if b == 0 {
                    return Err("division by zero");
                }
                let out = a.checked_div(b).ok_or("integer division overflow")?;
                Self::try_from_signed(self.tag, out)
            }
            (IntPayload::Unsigned(a), IntPayload::Unsigned(b)) => {
                if b == 0 {
                    return Err("division by zero");
                }
                let out = a / b;
                Self::try_from_unsigned(self.tag, out)
            }
            _ => Err("integer payload mismatch"),
        }
    }

    pub fn checked_mod(self, rhs: Self) -> Result<Self, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(a), IntPayload::Signed(b)) => {
                if b == 0 {
                    return Err("modulo by zero");
                }
                let out = a % b;
                Self::try_from_signed(self.tag, out)
            }
            (IntPayload::Unsigned(a), IntPayload::Unsigned(b)) => {
                if b == 0 {
                    return Err("modulo by zero");
                }
                let out = a % b;
                Self::try_from_unsigned(self.tag, out)
            }
            _ => Err("integer payload mismatch"),
        }
    }

    pub fn checked_pow(self, rhs: Self) -> Result<Self, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(base), IntPayload::Signed(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| "integer exponent out of range")?;
                let out = base
                    .checked_pow(exp_u32)
                    .ok_or("integer overflow in pow")?;
                Self::try_from_signed(self.tag, out)
            }
            (IntPayload::Unsigned(base), IntPayload::Unsigned(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| "integer exponent out of range")?;
                let out = base
                    .checked_pow(exp_u32)
                    .ok_or("integer overflow in pow")?;
                Self::try_from_unsigned(self.tag, out)
            }
            _ => Err("integer payload mismatch"),
        }
    }

    pub fn checked_bitwise_and(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| Some(a & b), |a, b| Some(a & b))
    }

    pub fn checked_bitwise_or(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| Some(a | b), |a, b| Some(a | b))
    }

    pub fn checked_bitwise_xor(self, rhs: Self) -> Result<Self, &'static str> {
        self.checked_bin(rhs, |a, b| Some(a ^ b), |a, b| Some(a ^ b))
    }

    pub fn checked_shift(self, rhs: Self, is_left: bool) -> Result<Self, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(a), IntPayload::Signed(b)) => {
                let shift = u32::try_from(b).map_err(|_| "shift amount must be non-negative")?;
                let out = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or("shift amount out of range")?;
                Self::try_from_signed(self.tag, out)
            }
            (IntPayload::Unsigned(a), IntPayload::Unsigned(b)) => {
                let shift = u32::try_from(b).map_err(|_| "shift amount out of range")?;
                let out = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or("shift amount out of range")?;
                Self::try_from_unsigned(self.tag, out)
            }
            _ => Err("integer payload mismatch"),
        }
    }

    pub fn cmp_same_type(self, rhs: Self) -> Result<std::cmp::Ordering, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(a), IntPayload::Signed(b)) => Ok(a.cmp(&b)),
            (IntPayload::Unsigned(a), IntPayload::Unsigned(b)) => Ok(a.cmp(&b)),
            _ => Err("integer payload mismatch"),
        }
    }

    fn is_in_range(self) -> bool {
        match (self.tag, self.payload) {
            (IntTag::I8, IntPayload::Signed(v)) => i8::try_from(v).is_ok(),
            (IntTag::I16, IntPayload::Signed(v)) => i16::try_from(v).is_ok(),
            (IntTag::I32, IntPayload::Signed(v)) => i32::try_from(v).is_ok(),
            (IntTag::I64, IntPayload::Signed(v)) => i64::try_from(v).is_ok(),
            (IntTag::I128, IntPayload::Signed(_)) => true,
            (IntTag::U1, IntPayload::Unsigned(v)) => v <= 1,
            (IntTag::U8, IntPayload::Unsigned(v)) => u8::try_from(v).is_ok(),
            (IntTag::U16, IntPayload::Unsigned(v)) => u16::try_from(v).is_ok(),
            (IntTag::U32, IntPayload::Unsigned(v)) => u32::try_from(v).is_ok(),
            (IntTag::U64, IntPayload::Unsigned(v)) => u64::try_from(v).is_ok(),
            (IntTag::U128, IntPayload::Unsigned(_)) => true,
            _ => false,
        }
    }

    fn checked_bin(
        self,
        rhs: Self,
        signed_op: fn(i128, i128) -> Option<i128>,
        unsigned_op: fn(u128, u128) -> Option<u128>,
    ) -> Result<Self, &'static str> {
        if self.tag != rhs.tag {
            return Err("integer type mismatch");
        }
        match (self.payload, rhs.payload) {
            (IntPayload::Signed(a), IntPayload::Signed(b)) => {
                let out = signed_op(a, b).ok_or("integer overflow or invalid operation")?;
                Self::try_from_signed(self.tag, out)
            }
            (IntPayload::Unsigned(a), IntPayload::Unsigned(b)) => {
                let out = unsigned_op(a, b).ok_or("integer overflow or invalid operation")?;
                Self::try_from_unsigned(self.tag, out)
            }
            _ => Err("integer payload mismatch"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IntPayload, IntTag, TypedInt};

    #[test]
    fn accepts_in_range_signed_values() {
        let v = TypedInt::try_new(IntTag::I32, IntPayload::Signed(2147483647)).expect("i32 max");
        assert_eq!(v.type_name(), "i32");
    }

    #[test]
    fn rejects_out_of_range_signed_values() {
        let err = TypedInt::try_new(IntTag::I32, IntPayload::Signed(2147483648));
        assert!(err.is_err());
    }

    #[test]
    fn accepts_u1_values_only_zero_or_one() {
        assert!(TypedInt::try_new(IntTag::U1, IntPayload::Unsigned(0)).is_ok());
        assert!(TypedInt::try_new(IntTag::U1, IntPayload::Unsigned(1)).is_ok());
        assert!(TypedInt::try_new(IntTag::U1, IntPayload::Unsigned(2)).is_err());
    }

    #[test]
    fn rejects_payload_sign_mismatch() {
        assert!(TypedInt::try_new(IntTag::I16, IntPayload::Unsigned(1)).is_err());
        assert!(TypedInt::try_new(IntTag::U16, IntPayload::Signed(1)).is_err());
    }

    #[test]
    fn exposes_typed_accessors() {
        let i = TypedInt::try_from_signed(IntTag::I64, 7).expect("i64");
        assert_eq!(i.as_signed(), Some(7));
        assert_eq!(i.as_unsigned(), None);

        let u = TypedInt::try_from_unsigned(IntTag::U64, 7).expect("u64");
        assert_eq!(u.as_unsigned(), Some(7));
        assert_eq!(u.as_signed(), None);
    }
}
