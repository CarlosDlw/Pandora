//! Linear VM execution: intrinsics use [`SymbolOrigin::Intrinsic`];
//! user-defined callees are rejected until closures/functions exist.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use csv::{ReaderBuilder as CsvReaderBuilder, WriterBuilder as CsvWriterBuilder};
use quick_xml::Reader as XmlReader;
use quick_xml::events::Event as XmlEvent;
use regex::Regex;
use rustc_hash::FxHashMap as HashMap;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

use crate::hir::symbols::{SymbolId, SymbolOrigin, SymbolTable};

use super::{bytecode::Op, chunk::Chunk, value::Value};

#[derive(Debug, Clone)]
struct CallFrame {
    chunk: Arc<Chunk>,
    ip: usize,
    locals: Vec<(SymbolId, Value)>,
    scope_frames: Vec<Vec<SymbolId>>,
    current_function: Option<FunctionContext>,
}

#[derive(Debug, Clone)]
struct FunctionContext {
    function: Arc<crate::vm::chunk::FunctionChunk>,
    self_symbol: Option<SymbolId>,
}

#[derive(Debug, Clone)]
struct TryContext {
    call_depth: usize,
    handler_ip: usize,
    stack_len: usize,
    scope_depth: usize,
}

pub struct Vm<'a> {
    chunk: Arc<Chunk>,
    ip: usize,
    stack: Vec<Value>,
    locals: Vec<(SymbolId, Value)>,
    globals: RefCell<HashMap<SymbolId, Value>>,
    scope_frames: Vec<Vec<SymbolId>>,
    current_function: Option<FunctionContext>,
    symbols: &'a SymbolTable,
    intrinsic_ids: HashMap<SymbolId, IntrinsicId>,
    call_stack: Vec<CallFrame>,
    try_stack: Vec<TryContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntrinsicId {
    Known,
}

impl<'a> Vm<'a> {
    pub fn new(
        chunk: &'a Chunk,
        symbols: &'a SymbolTable,
        initial_env: HashMap<SymbolId, Value>,
    ) -> Self {
        let mut globals = initial_env;
        let mut intrinsic_ids = HashMap::default();
        for idx in 0..u32::MAX {
            let id = SymbolId(idx);
            let Some(symbol) = symbols.symbol(id) else {
                break;
            };
            if symbol.origin == SymbolOrigin::Intrinsic {
                intrinsic_ids.insert(id, IntrinsicId::Known);
                globals.entry(id).or_insert(Value::Builtin(id));
            }
        }
        Self {
            chunk: Arc::new(chunk.clone()),
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            globals: RefCell::new(globals),
            scope_frames: vec![Vec::new()],
            current_function: None,
            symbols,
            intrinsic_ids,
            call_stack: Vec::new(),
            try_stack: Vec::new(),
        }
    }

    /// Executes until [`Op::Return`] or end of chunk. On failure returns accumulated diagnostics (no panics).
    pub fn run(&mut self) -> Result<(), Diagnostics> {
        let mut diagnostics = Diagnostics::new();

        loop {
            let active_chunk = Arc::clone(&self.chunk);

            while self.ip < active_chunk.code.len() {
                let op = &active_chunk.code[self.ip];
                let span = active_chunk.spans[self.ip];
                self.ip += 1;

                if let Err(d) = self.step_op(op, span) {
                    if self.recoverable_panic(&d) && self.recover_from_panic(&d) {
                        break;
                    }
                    diagnostics.push(d);
                    return Err(diagnostics);
                }

                if !Arc::ptr_eq(&active_chunk, &self.chunk) {
                    break;
                }
            }

            if !Arc::ptr_eq(&active_chunk, &self.chunk) {
                continue;
            }

            if self.ip >= self.chunk.code.len() {
                match self.return_from_call_frame() {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(d) => {
                        diagnostics.push(d);
                        return Err(diagnostics);
                    }
                }
            }
        }

        if !self.stack.is_empty() {
            diagnostics.push(Diagnostic::new(
                format!(
                    "internal error: stack not empty after execution ({} value(s) left)",
                    self.stack.len()
                ),
                self.last_chunk_span(),
                Severity::Error,
            ));
            return Err(diagnostics);
        }
        Ok(())
    }

    fn locals_get(&self, sym: SymbolId) -> Option<Value> {
        self.locals
            .iter()
            .rev()
            .find(|(local_sym, _)| *local_sym == sym)
            .map(|(_, value)| value.clone())
    }

    fn locals_get_mut(&mut self, sym: SymbolId) -> Option<&mut Value> {
        let index = self
            .locals
            .iter()
            .rposition(|(local_sym, _)| *local_sym == sym)?;
        Some(&mut self.locals[index].1)
    }

    fn locals_insert(&mut self, sym: SymbolId, value: Value) {
        if let Some(index) = self
            .locals
            .iter()
            .rposition(|(local_sym, _)| *local_sym == sym)
        {
            self.locals[index].1 = value;
        } else {
            self.locals.push((sym, value));
        }
    }

    fn locals_remove(&mut self, sym: SymbolId) {
        if let Some(index) = self
            .locals
            .iter()
            .rposition(|(local_sym, _)| *local_sym == sym)
        {
            self.locals.remove(index);
        }
    }

    fn snapshot_locals(&self) -> HashMap<SymbolId, Value> {
        self.locals.iter().cloned().collect()
    }

    fn last_chunk_span(&self) -> Span {
        self.chunk
            .spans
            .last()
            .copied()
            .unwrap_or_else(|| Span::new_unchecked(foundation::ids::FileId::from_u32(0), 0, 0))
    }

    fn step_op(&mut self, op: &Op, span: Span) -> Result<(), Diagnostic> {
        match op {
            Op::ConstI128(n) => {
                let v = super::int::TypedInt::try_from_signed(super::int::IntTag::I128, *n)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                self.stack.push(Value::Int(v));
            }
            Op::ConstU128(n) => {
                let v = super::int::TypedInt::try_from_unsigned(super::int::IntTag::U128, *n)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                self.stack.push(Value::Int(v));
            }
            Op::ConstInt(n) => self.stack.push(Value::Int(*n)),
            Op::ConstBool(b) => self.stack.push(Value::Bool(*b)),
            Op::ConstStr(s) => self.stack.push(Value::Str(s.clone())),
            Op::ConstFloat(f) => self.stack.push(Value::Float(*f)),
            Op::ConstChar(c) => self.stack.push(Value::Char(*c)),
            Op::ConstUnit => self.stack.push(Value::Unit),
            Op::ConstNull => self.stack.push(Value::Null),
            Op::MakeTuple(count) => {
                let count = *count as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.pop_one(span)?);
                }
                items.reverse();
                self.stack.push(Value::Tuple(items));
            }
            Op::MakeArray(count) => {
                let count = *count as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.pop_one(span)?);
                }
                items.reverse();
                self.stack.push(Value::Array(items));
            }
            Op::MakeMap(count) => {
                let count = *count as usize;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let value = self.pop_one(span)?;
                    let key = self.pop_one(span)?;
                    entries.push((key, value));
                }
                entries.reverse();
                self.stack.push(Value::Map(entries));
            }
            Op::MakeSet(count) => {
                let count = *count as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let value = self.pop_one(span)?;
                    if !items.contains(&value) {
                        items.push(value);
                    }
                }
                items.reverse();
                self.stack.push(Value::Set(items));
            }
            Op::MakeRange(inclusive) => {
                let end = self.pop_one(span)?;
                let start = self.pop_one(span)?;
                let mut items = Vec::new();
                match (start, end) {
                    (Value::Int(s), Value::Int(e)) => {
                        if s.tag() != e.tag() {
                            return Err(Diagnostic::new(
                                "range bounds must be same integer type",
                                span,
                                Severity::Error,
                            ));
                        }
                        match (s.payload(), e.payload()) {
                            (
                                super::int::IntPayload::Signed(ss),
                                super::int::IntPayload::Signed(ee),
                            ) => {
                                let end_bound = if *inclusive { ee.saturating_add(1) } else { ee };
                                if ss < end_bound {
                                    let mut i = ss;
                                    while i < end_bound {
                                        let v = super::int::TypedInt::try_from_signed(s.tag(), i)
                                            .map_err(|msg| {
                                            Diagnostic::new(msg, span, Severity::Error)
                                        })?;
                                        items.push(Value::Int(v));
                                        i = i.saturating_add(1);
                                    }
                                }
                            }
                            (
                                super::int::IntPayload::Unsigned(ss),
                                super::int::IntPayload::Unsigned(ee),
                            ) => {
                                let end_bound = if *inclusive { ee.saturating_add(1) } else { ee };
                                if ss < end_bound {
                                    let mut i = ss;
                                    while i < end_bound {
                                        let v = super::int::TypedInt::try_from_unsigned(s.tag(), i)
                                            .map_err(|msg| {
                                                Diagnostic::new(msg, span, Severity::Error)
                                            })?;
                                        items.push(Value::Int(v));
                                        i = i.saturating_add(1);
                                    }
                                }
                            }
                            _ => {
                                return Err(Diagnostic::new(
                                    "range bounds must be same integer signedness",
                                    span,
                                    Severity::Error,
                                ));
                            }
                        }
                    }
                    (Value::Int128(s), Value::Int128(e)) => {
                        let end_bound = if *inclusive { e.saturating_add(1) } else { e };
                        if s < end_bound {
                            let mut i = s;
                            while i < end_bound {
                                items.push(Value::Int128(i));
                                i = i.saturating_add(1);
                            }
                        }
                    }
                    (Value::UInt128(s), Value::UInt128(e)) => {
                        let end_bound = if *inclusive { e.saturating_add(1) } else { e };
                        if s < end_bound {
                            let mut i = s;
                            while i < end_bound {
                                items.push(Value::UInt128(i));
                                i = i.saturating_add(1);
                            }
                        }
                    }
                    (a, b) => {
                        return Err(Diagnostic::new(
                            format!(
                                "range bounds must be same integer type, got {:?} and {:?}",
                                a, b
                            ),
                            span,
                            Severity::Error,
                        ));
                    }
                }
                self.stack.push(Value::Array(items));
            }
            Op::ArrayExtend => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (lhs, rhs) {
                    (Value::Array(mut base), Value::Array(extra)) => {
                        base.extend(extra);
                        self.stack.push(Value::Array(base));
                    }
                    (a, b) => {
                        return Err(Diagnostic::new(
                            format!("array spread expects arrays, got {:?} and {:?}", a, b),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::TupleGet(index) => {
                let tuple = self.pop_one(span)?;
                match tuple {
                    Value::Tuple(items) => {
                        let value = items.get(*index).cloned().ok_or_else(|| {
                            Diagnostic::new("tuple index out of range", span, Severity::Error)
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("tuple access on non-tuple value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::ArrayGet => {
                let index_value = self.pop_one(span)?;
                let array_value = self.pop_one(span)?;
                let index = to_usize_index(&index_value, span)?;
                match array_value {
                    Value::Array(items) => {
                        let len = items.len();
                        let value = items.get(index).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("index out of bounds: index={}, len={}", index, len),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    Value::Tuple(items) => {
                        let len = items.len();
                        let value = items.get(index).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("tuple index out of bounds: index={}, len={}", index, len),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("index access on non-array value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::ArrayLen => {
                let array_value = self.pop_one(span)?;
                match array_value {
                    Value::Array(items) => {
                        let n = i128::try_from(items.len()).map_err(|_| {
                            Diagnostic::new(
                                "array length exceeds i128 range",
                                span,
                                Severity::Error,
                            )
                        })?;
                        let v = super::int::TypedInt::try_from_signed(super::int::IntTag::I128, n)
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                        self.stack.push(Value::Int(v));
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("array length on non-array value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::MakeStruct(type_name, field_count) => {
                let field_count = *field_count as usize;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    fields.push(self.pop_one(span)?);
                }
                fields.reverse();
                self.stack.push(Value::StructInstance {
                    type_name: type_name.clone(),
                    fields,
                });
            }
            Op::StructLoadSlot(sym, slot) => {
                if let Some((_, value)) = self
                    .locals
                    .iter()
                    .rev()
                    .find(|(local_sym, _)| *local_sym == *sym)
                {
                    match value {
                        Value::StructInstance { fields, .. } => {
                            let value = fields.get(*slot).cloned().ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field slot '{}'", slot),
                                    span,
                                    Severity::Error,
                                )
                            })?;
                            self.stack.push(value);
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("field access on non-struct value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else {
                    let globals = self.globals.borrow();
                    let Some(value) = globals.get(sym) else {
                        return Err(Diagnostic::new(
                            "load of uninitialized or missing symbol",
                            span,
                            Severity::Error,
                        ));
                    };
                    match value {
                        Value::StructInstance { fields, .. } => {
                            let value = fields.get(*slot).cloned().ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field slot '{}'", slot),
                                    span,
                                    Severity::Error,
                                )
                            })?;
                            self.stack.push(value);
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("field access on non-struct value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                }
            }
            Op::StructGetSlot(slot) => {
                let base = self.pop_one(span)?;
                match base {
                    Value::StructInstance { fields, .. } => {
                        let value = fields.get(*slot).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("unknown struct field slot '{}'", slot),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("field access on non-struct value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::StructGet(field) => {
                let base = self.pop_one(span)?;
                match base {
                    Value::Err {
                        message,
                        code,
                        origin,
                        cause,
                    } => {
                        let value = match field.as_str() {
                            "message" => Value::Str(message),
                            "code" => {
                                let v = super::int::TypedInt::try_from_signed(
                                    super::int::IntTag::I32,
                                    i128::from(code),
                                )
                                .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                                Value::Int(v)
                            }
                            "origin" => Value::Str(origin),
                            "cause" => cause.map(|c| *c).unwrap_or(Value::Null),
                            _ => {
                                return Err(Diagnostic::new(
                                    format!("unknown err field '{field}'"),
                                    span,
                                    Severity::Error,
                                ));
                            }
                        };
                        self.stack.push(value);
                    }
                    Value::StructInstance { type_name, fields } => {
                        let slot = self
                            .chunk
                            .struct_field_slots
                            .get(type_name.as_str())
                            .and_then(|slots| slots.get(field.as_str()))
                            .copied()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field '{field}'"),
                                    span,
                                    Severity::Error,
                                )
                            })?;
                        let value = fields.get(slot).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("unknown struct field slot '{}'", slot),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("field access on non-struct value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::StructSetSlot(slot) => {
                let value = self.pop_one(span)?;
                let base = self.pop_one(span)?;
                match base {
                    Value::StructInstance {
                        type_name,
                        mut fields,
                    } => {
                        let Some(target) = fields.get_mut(*slot) else {
                            return Err(Diagnostic::new(
                                format!("unknown struct field slot '{}'", slot),
                                span,
                                Severity::Error,
                            ));
                        };
                        *target = value;
                        self.stack.push(Value::StructInstance { type_name, fields });
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("field assignment on non-struct value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::StructSet(field) => {
                let value = self.pop_one(span)?;
                let base = self.pop_one(span)?;
                match base {
                    Value::StructInstance {
                        type_name,
                        mut fields,
                    } => {
                        let slot = self
                            .chunk
                            .struct_field_slots
                            .get(type_name.as_str())
                            .and_then(|slots| slots.get(field.as_str()))
                            .copied()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field '{field}'"),
                                    span,
                                    Severity::Error,
                                )
                            })?;
                        let Some(target) = fields.get_mut(slot) else {
                            return Err(Diagnostic::new(
                                format!("unknown struct field slot '{}'", slot),
                                span,
                                Severity::Error,
                            ));
                        };
                        *target = value;
                        self.stack.push(Value::StructInstance { type_name, fields });
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("field assignment on non-struct value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }

            Op::Load(sym) => {
                let v = if let Some(v) = self.locals_get(*sym) {
                    v
                } else if let Some(v) = self.globals.borrow().get(sym).cloned() {
                    v
                } else if let Some(current_function) = self.current_function.as_ref() {
                    if current_function.self_symbol == Some(*sym) {
                        Value::Function {
                            function: Arc::clone(&current_function.function),
                            captured: Arc::new(self.snapshot_locals()),
                            self_symbol: current_function.self_symbol,
                        }
                    } else {
                        return Err(Diagnostic::new(
                            "load of uninitialized or missing symbol",
                            span,
                            Severity::Error,
                        ));
                    }
                } else {
                    return Err(Diagnostic::new(
                        "load of uninitialized or missing symbol",
                        span,
                        Severity::Error,
                    ));
                };
                self.stack.push(v);
            }

            Op::Bind(sym) => {
                let v = self.pop_one(span)?;
                let is_global_scope = self.call_stack.is_empty() && self.scope_frames.len() == 1;
                if is_global_scope {
                    self.globals.borrow_mut().insert(*sym, v.clone());
                }
                self.locals_insert(*sym, v);
                if let Some(frame) = self.scope_frames.last_mut() {
                    frame.push(*sym);
                }
            }
            Op::MakeClosure(sym) => {
                let function = self.chunk.functions.get(sym).cloned().ok_or_else(|| {
                    Diagnostic::new("function body not found", span, Severity::Error)
                })?;
                self.stack.push(Value::Function {
                    function: Arc::new(function),
                    captured: Arc::new(self.snapshot_locals()),
                    self_symbol: Some(*sym),
                });
            }

            Op::Assign(sym) => {
                let sym_info = self.symbols.symbol(*sym).ok_or_else(|| {
                    Diagnostic::new("unknown symbol id in assignment", span, Severity::Error)
                })?;
                if sym_info.origin == SymbolOrigin::Intrinsic {
                    return Err(Diagnostic::new(
                        "cannot assign to a builtin",
                        span,
                        Severity::Error,
                    ));
                }
                if sym_info.is_const {
                    return Err(Diagnostic::new(
                        "cannot assign to a constant declared with '::'",
                        span,
                        Severity::Error,
                    ));
                }
                let v = self.pop_one(span)?;
                let is_global_scope = self.call_stack.is_empty() && self.scope_frames.len() == 1;
                if let Some(slot) = self.locals_get_mut(*sym) {
                    *slot = v;
                } else if is_global_scope || self.globals.borrow().contains_key(sym) {
                    self.globals.borrow_mut().insert(*sym, v);
                } else {
                    self.locals_insert(*sym, v);
                }
            }
            Op::ArrayAssign(sym) => {
                let value = self.pop_one(span)?;
                let index_value = self.pop_one(span)?;
                let index = to_usize_index(&index_value, span)?;
                let is_global_scope = self.call_stack.is_empty() && self.scope_frames.len() == 1;
                if let Some(slot) = self.locals_get_mut(*sym) {
                    match slot {
                        Value::Array(items) => {
                            if index >= items.len() {
                                return Err(Diagnostic::new(
                                    format!(
                                        "index out of bounds: index={}, len={}",
                                        index,
                                        items.len()
                                    ),
                                    span,
                                    Severity::Error,
                                ));
                            }
                            items[index] = value;
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("index assignment on non-array value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else if is_global_scope || self.globals.borrow().contains_key(sym) {
                    let mut globals = self.globals.borrow_mut();
                    let Some(slot) = globals.get_mut(sym) else {
                        return Err(Diagnostic::new(
                            "array assignment target not found",
                            span,
                            Severity::Error,
                        ));
                    };
                    match slot {
                        Value::Array(items) => {
                            if index >= items.len() {
                                return Err(Diagnostic::new(
                                    format!(
                                        "index out of bounds: index={}, len={}",
                                        index,
                                        items.len()
                                    ),
                                    span,
                                    Severity::Error,
                                ));
                            }
                            items[index] = value;
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("index assignment on non-array value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else {
                    return Err(Diagnostic::new(
                        "array assignment target not found",
                        span,
                        Severity::Error,
                    ));
                }
            }
            Op::StructAssignSlot(sym, slot) => {
                let value = self.pop_one(span)?;
                let is_global_scope = self.call_stack.is_empty() && self.scope_frames.len() == 1;
                if let Some(target) = self.locals_get_mut(*sym) {
                    match target {
                        Value::StructInstance { fields, .. } => {
                            let Some(field_target) = fields.get_mut(*slot) else {
                                return Err(Diagnostic::new(
                                    format!("unknown struct field slot '{}'", slot),
                                    span,
                                    Severity::Error,
                                ));
                            };
                            *field_target = value;
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("field assignment on non-struct value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else if is_global_scope || self.globals.borrow().contains_key(sym) {
                    let mut globals = self.globals.borrow_mut();
                    let Some(target) = globals.get_mut(sym) else {
                        return Err(Diagnostic::new(
                            "field assignment target not found",
                            span,
                            Severity::Error,
                        ));
                    };
                    match target {
                        Value::StructInstance { fields, .. } => {
                            let Some(field_target) = fields.get_mut(*slot) else {
                                return Err(Diagnostic::new(
                                    format!("unknown struct field slot '{}'", slot),
                                    span,
                                    Severity::Error,
                                ));
                            };
                            *field_target = value;
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("field assignment on non-struct value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else {
                    return Err(Diagnostic::new(
                        "field assignment target not found",
                        span,
                        Severity::Error,
                    ));
                }
            }
            Op::StructAssign(sym, field) => {
                let value = self.pop_one(span)?;
                let is_global_scope = self.call_stack.is_empty() && self.scope_frames.len() == 1;
                if let Some(local_index) = self
                    .locals
                    .iter()
                    .rposition(|(local_sym, _)| *local_sym == *sym)
                {
                    let slot = {
                        let current = &self.locals[local_index].1;
                        let Value::StructInstance { type_name, .. } = current else {
                            return Err(Diagnostic::new(
                                format!("field assignment on non-struct value: {:?}", current),
                                span,
                                Severity::Error,
                            ));
                        };
                        self.chunk
                            .struct_field_slots
                            .get(type_name.as_str())
                            .and_then(|slots| slots.get(field.as_str()))
                            .copied()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field '{}'", field),
                                    span,
                                    Severity::Error,
                                )
                            })?
                    };

                    let target = &mut self.locals[local_index].1;
                    let Value::StructInstance { fields, .. } = target else {
                        return Err(Diagnostic::new(
                            format!("field assignment on non-struct value: {:?}", target),
                            span,
                            Severity::Error,
                        ));
                    };
                    let Some(field_target) = fields.get_mut(slot) else {
                        return Err(Diagnostic::new(
                            format!("unknown struct field slot '{}'", slot),
                            span,
                            Severity::Error,
                        ));
                    };
                    *field_target = value;
                } else if is_global_scope || self.globals.borrow().contains_key(sym) {
                    let slot = {
                        let globals = self.globals.borrow();
                        let Some(current) = globals.get(sym) else {
                            return Err(Diagnostic::new(
                                "field assignment target not found",
                                span,
                                Severity::Error,
                            ));
                        };
                        let Value::StructInstance { type_name, .. } = current else {
                            return Err(Diagnostic::new(
                                format!("field assignment on non-struct value: {:?}", current),
                                span,
                                Severity::Error,
                            ));
                        };
                        self.chunk
                            .struct_field_slots
                            .get(type_name.as_str())
                            .and_then(|slots| slots.get(field.as_str()))
                            .copied()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    format!("unknown struct field '{}'", field),
                                    span,
                                    Severity::Error,
                                )
                            })?
                    };

                    let mut globals = self.globals.borrow_mut();
                    let Some(target) = globals.get_mut(sym) else {
                        return Err(Diagnostic::new(
                            "field assignment target not found",
                            span,
                            Severity::Error,
                        ));
                    };
                    match target {
                        Value::StructInstance { fields, .. } => {
                            let Some(field_target) = fields.get_mut(slot) else {
                                return Err(Diagnostic::new(
                                    format!("unknown struct field slot '{}'", slot),
                                    span,
                                    Severity::Error,
                                ));
                            };
                            *field_target = value;
                        }
                        other => {
                            return Err(Diagnostic::new(
                                format!("field assignment on non-struct value: {:?}", other),
                                span,
                                Severity::Error,
                            ));
                        }
                    }
                } else {
                    return Err(Diagnostic::new(
                        "field assignment target not found",
                        span,
                        Severity::Error,
                    ));
                }
            }

            Op::Neg => self.apply_neg(span)?,
            Op::Not => self.apply_not(span)?,
            Op::BitNot => self.apply_bit_not(span)?,

            Op::Add => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (&lhs, &rhs) {
                    (Value::Str(_), _) | (_, Value::Str(_)) => {
                        let result =
                            format!("{}{}", lhs.display_for_print(), rhs.display_for_print());
                        self.stack.push(Value::Str(result));
                    }
                    _ => {
                        self.stack.push(lhs);
                        self.stack.push(rhs);
                        self.apply_bin_checked(
                            span,
                            |a, b| a.checked_add(b),
                            |a, b| a.checked_add(b),
                            |a, b| Some(a + b),
                        )?;
                    }
                }
            }
            Op::AddInt => self.apply_bin_checked(
                span,
                |a, b| a.checked_add(b),
                |a, b| a.checked_add(b),
                |_, _| None,
            )?,
            Op::AddFloat => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (lhs, rhs) {
                    (Value::Float(a), Value::Float(b)) => self.stack.push(Value::Float(a + b)),
                    (l, r) => {
                        return Err(Diagnostic::new(
                            format!("invalid operands for AddFloat: {:?} and {:?}", l, r),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::StrConcat => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (lhs, rhs) {
                    (Value::Str(a), Value::Str(b)) => {
                        self.stack.push(Value::Str(format!("{a}{b}")))
                    }
                    (l, r) => {
                        return Err(Diagnostic::new(
                            format!("invalid operands for StrConcat: {:?} and {:?}", l, r),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::Sub => self.apply_bin_checked(
                span,
                |a, b| a.checked_sub(b),
                |a, b| a.checked_sub(b),
                |a, b| Some(a - b),
            )?,
            Op::SubInt => self.apply_bin_checked(
                span,
                |a, b| a.checked_sub(b),
                |a, b| a.checked_sub(b),
                |_, _| None,
            )?,
            Op::Mul => self.apply_bin_checked(
                span,
                |a, b| a.checked_mul(b),
                |a, b| a.checked_mul(b),
                |a, b| Some(a * b),
            )?,
            Op::MulInt => self.apply_bin_checked(
                span,
                |a, b| a.checked_mul(b),
                |a, b| a.checked_mul(b),
                |_, _| None,
            )?,
            Op::Div => self.apply_div(span)?,
            Op::DivInt => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                let out = match (lhs, rhs) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(
                        a.checked_div(b)
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                    ),
                    (Value::Int128(a), Value::Int128(b)) => {
                        if b == 0 {
                            return Err(Diagnostic::new("division by zero", span, Severity::Error));
                        }
                        Value::Int128(a.checked_div(b).ok_or_else(|| {
                            Diagnostic::new("integer division overflow", span, Severity::Error)
                        })?)
                    }
                    (Value::UInt128(a), Value::UInt128(b)) => {
                        if b == 0 {
                            return Err(Diagnostic::new("division by zero", span, Severity::Error));
                        }
                        Value::UInt128(a / b)
                    }
                    (l, r) => {
                        return Err(Diagnostic::new(
                            format!("invalid operands for DivInt: {:?} and {:?}", l, r),
                            span,
                            Severity::Error,
                        ));
                    }
                };
                self.stack.push(out);
            }
            Op::Mod => self.apply_mod(span)?,
            Op::ModInt => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                let out = match (lhs, rhs) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(
                        a.checked_mod(b)
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                    ),
                    (Value::Int128(a), Value::Int128(b)) => {
                        if b == 0 {
                            return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                        }
                        Value::Int128(a % b)
                    }
                    (Value::UInt128(a), Value::UInt128(b)) => {
                        if b == 0 {
                            return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                        }
                        Value::UInt128(a % b)
                    }
                    (l, r) => {
                        return Err(Diagnostic::new(
                            format!("invalid operands for ModInt: {:?} and {:?}", l, r),
                            span,
                            Severity::Error,
                        ));
                    }
                };
                self.stack.push(out);
            }
            Op::Pow => self.apply_pow(span)?,
            Op::Eq => self.apply_eq(span)?,
            Op::Ne => self.apply_ne(span)?,
            Op::Lt => self.apply_cmp(span, |ord| ord == std::cmp::Ordering::Less)?,
            Op::Le => self.apply_cmp(span, |ord| ord != std::cmp::Ordering::Greater)?,
            Op::Gt => self.apply_cmp(span, |ord| ord == std::cmp::Ordering::Greater)?,
            Op::Ge => self.apply_cmp(span, |ord| ord != std::cmp::Ordering::Less)?,
            Op::LogicalAnd => self.apply_logical(span, |a, b| a && b)?,
            Op::LogicalOr => self.apply_logical(span, |a, b| a || b)?,
            Op::BitAnd => self.apply_bitwise(span, |a, b| a & b, |a, b| a & b)?,
            Op::BitOr => self.apply_bitwise(span, |a, b| a | b, |a, b| a | b)?,
            Op::BitXor => self.apply_bitwise(span, |a, b| a ^ b, |a, b| a ^ b)?,
            Op::Shl => self.apply_shift(span, true)?,
            Op::Shr => self.apply_shift(span, false)?,
            Op::JumpIfFalse(target) => {
                if *target > self.chunk.code.len() {
                    return Err(Diagnostic::new(
                        "invalid jump target",
                        span,
                        Severity::Error,
                    ));
                }
                let value = self.pop_one(span)?;
                if !is_truthy(&value) {
                    self.ip = *target;
                }
            }
            Op::Jump(target) => {
                if *target > self.chunk.code.len() {
                    return Err(Diagnostic::new(
                        "invalid jump target",
                        span,
                        Severity::Error,
                    ));
                }
                self.ip = *target;
            }
            Op::TryStart(handler) => {
                self.try_stack.push(TryContext {
                    call_depth: self.call_stack.len(),
                    handler_ip: *handler,
                    stack_len: self.stack.len(),
                    scope_depth: self.scope_frames.len(),
                });
            }
            Op::TryEnd => {
                if let Some(last) = self.try_stack.last()
                    && last.call_depth == self.call_stack.len()
                {
                    let _ = self.try_stack.pop();
                }
            }

            Op::Call(sym, argc) => {
                let argc = *argc as usize;
                let args = match argc {
                    0 => Vec::new(),
                    1 => {
                        let arg0 = self.pop_one(span)?;
                        vec![arg0]
                    }
                    2 => {
                        let arg1 = self.pop_one(span)?;
                        let arg0 = self.pop_one(span)?;
                        vec![arg0, arg1]
                    }
                    _ => {
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop_one(span)?);
                        }
                        args.reverse();
                        args
                    }
                };

                let symbol = self.symbols.symbol(*sym).ok_or_else(|| {
                    Diagnostic::new("unknown symbol id in call", span, Severity::Error)
                })?;

                if symbol.origin != SymbolOrigin::Intrinsic {
                    return Err(Diagnostic::new(
                        "calls to user-defined functions are not implemented yet",
                        span,
                        Severity::Error,
                    ));
                }

                let ret = self.dispatch_intrinsic(*sym, &args, span)?;
                self.stack.push(ret);
            }
            Op::CallDirect(sym, argc) => {
                let argc = *argc as usize;
                let args = match argc {
                    0 => Vec::new(),
                    1 => {
                        let arg0 = self.pop_one(span)?;
                        vec![arg0]
                    }
                    2 => {
                        let arg1 = self.pop_one(span)?;
                        let arg0 = self.pop_one(span)?;
                        vec![arg0, arg1]
                    }
                    _ => {
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop_one(span)?);
                        }
                        args.reverse();
                        args
                    }
                };
                let callee = self.resolve_symbol_for_call(*sym, span)?;
                self.invoke_callee(callee, args, argc, span)?;
            }
            Op::CallValue(argc) => {
                let argc = *argc as usize;
                let args = match argc {
                    0 => Vec::new(),
                    1 => {
                        let arg0 = self.pop_one(span)?;
                        vec![arg0]
                    }
                    2 => {
                        let arg1 = self.pop_one(span)?;
                        let arg0 = self.pop_one(span)?;
                        vec![arg0, arg1]
                    }
                    _ => {
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop_one(span)?);
                        }
                        args.reverse();
                        args
                    }
                };
                let callee = self.pop_one(span)?;
                self.invoke_callee(callee, args, argc, span)?;
            }

            Op::Pop => {
                let _ = self.pop_one(span)?;
            }
            Op::Dup => {
                let value = self.stack.last().cloned().ok_or_else(|| {
                    Diagnostic::new("stack underflow (dup)", span, Severity::Error)
                })?;
                self.stack.push(value);
            }
            Op::Swap => {
                if self.stack.len() < 2 {
                    return Err(Diagnostic::new(
                        "stack underflow (swap)",
                        span,
                        Severity::Error,
                    ));
                }
                let top = self.stack.len() - 1;
                self.stack.swap(top, top - 1);
            }
            Op::WrapErr => {
                let message = self.pop_one(span)?;
                let err = self.pop_one(span)?;
                let Value::Str(message) = message else {
                    return Err(Diagnostic::new(
                        "WrapErr requires message string",
                        span,
                        Severity::Error,
                    ));
                };
                let wrapped = wrap_err_value(
                    err,
                    message,
                    None,
                    "wrap".to_string(),
                    span,
                    &self.chunk.struct_field_slots,
                )?;
                self.stack.push(wrapped);
            }

            Op::EnterScope => {
                self.scope_frames.push(Vec::new());
            }

            Op::ExitScope => {
                if self.scope_frames.len() <= 1 {
                    return Err(Diagnostic::new(
                        "scope stack underflow (internal bytecode error)",
                        span,
                        Severity::Error,
                    ));
                }
                let frame = self.scope_frames.pop().expect("checked len");
                for sym in frame.into_iter().rev() {
                    self.locals_remove(sym);
                }
            }

            Op::Return => {
                self.return_from_call_frame()?;
            }
        }
        Ok(())
    }

    fn return_from_call_frame(&mut self) -> Result<bool, Diagnostic> {
        if let Some(frame) = self.call_stack.pop() {
            let ret = self.stack.pop().unwrap_or(Value::Unit);
            self.chunk = frame.chunk;
            self.ip = frame.ip;
            self.locals = frame.locals;
            self.scope_frames = frame.scope_frames;
            self.current_function = frame.current_function;
            self.stack.push(ret);
            self.try_stack
                .retain(|ctx| ctx.call_depth <= self.call_stack.len());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn recoverable_panic(&self, diagnostic: &Diagnostic) -> bool {
        diagnostic.message.starts_with("panic:") && !self.try_stack.is_empty()
    }

    fn recover_from_panic(&mut self, diagnostic: &Diagnostic) -> bool {
        let current_depth = self.call_stack.len();
        let Some(index) = self
            .try_stack
            .iter()
            .rposition(|ctx| ctx.call_depth <= current_depth)
        else {
            return false;
        };
        let ctx = self.try_stack[index].clone();
        self.try_stack.truncate(index);

        while self.call_stack.len() > ctx.call_depth {
            let Some(frame) = self.call_stack.pop() else {
                break;
            };
            self.chunk = frame.chunk;
            self.ip = frame.ip;
            self.locals = frame.locals;
            self.scope_frames = frame.scope_frames;
            self.current_function = frame.current_function;
        }

        self.stack.truncate(ctx.stack_len);
        while self.scope_frames.len() > ctx.scope_depth {
            let Some(frame) = self.scope_frames.pop() else {
                break;
            };
            for sym in frame {
                self.locals_remove(sym);
            }
        }
        self.ip = ctx.handler_ip;
        self.stack.push(err_value_from_panic(diagnostic));
        true
    }

    fn apply_neg(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        let out = match v {
            Value::Int(i) => Value::Int(
                i.checked_neg()
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            Value::Int128(i) => Value::Int128(i.checked_neg().ok_or_else(|| {
                Diagnostic::new("integer overflow in unary negation", span, Severity::Error)
            })?),
            Value::Float(f) => Value::Float(-f),
            Value::UInt128(_) => {
                return Err(Diagnostic::new(
                    "invalid operand for unary '-' (unsigned)",
                    span,
                    Severity::Error,
                ));
            }
            other => {
                return Err(Diagnostic::new(
                    format!(
                        "invalid operand for unary '-' (got {})",
                        builtin_type_name(&other)
                    ),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_not(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        match v {
            Value::Bool(b) => {
                self.stack.push(Value::Bool(!b));
                Ok(())
            }
            other => Err(Diagnostic::new(
                format!(
                    "invalid operand for logical '!': {}",
                    builtin_type_name(&other)
                ),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bit_not(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        match v {
            Value::Int(i) => {
                let out = i
                    .bit_not()
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                self.stack.push(Value::Int(out));
                Ok(())
            }
            Value::Int128(i) => {
                self.stack.push(Value::Int128(!i));
                Ok(())
            }
            Value::UInt128(u) => {
                self.stack.push(Value::UInt128(!u));
                Ok(())
            }
            other => Err(Diagnostic::new(
                format!(
                    "invalid operand for bitwise '~': {}",
                    builtin_type_name(&other)
                ),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bin_checked(
        &mut self,
        span: Span,
        signed_int: fn(i128, i128) -> Option<i128>,
        unsigned_int: fn(u128, u128) -> Option<u128>,
        float_op: fn(f64, f64) -> Option<f64>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if a.tag() != b.tag() {
                    return Err(Diagnostic::new(
                        "integer type mismatch",
                        span,
                        Severity::Error,
                    ));
                }
                let out = match (a.payload(), b.payload()) {
                    (super::int::IntPayload::Signed(x), super::int::IntPayload::Signed(y)) => {
                        let v = signed_int(x, y).ok_or_else(|| {
                            Diagnostic::new(
                                "integer overflow or invalid operation",
                                span,
                                Severity::Error,
                            )
                        })?;
                        super::int::TypedInt::try_from_signed(a.tag(), v)
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    }
                    (super::int::IntPayload::Unsigned(x), super::int::IntPayload::Unsigned(y)) => {
                        let v = unsigned_int(x, y).ok_or_else(|| {
                            Diagnostic::new(
                                "integer overflow or invalid operation",
                                span,
                                Severity::Error,
                            )
                        })?;
                        super::int::TypedInt::try_from_unsigned(a.tag(), v)
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            "integer payload mismatch",
                            span,
                            Severity::Error,
                        ));
                    }
                };
                Value::Int(out)
            }
            (Value::Int128(a), Value::Int128(b)) => {
                let r = signed_int(a, b).ok_or_else(|| {
                    Diagnostic::new(
                        "integer overflow or invalid operation",
                        span,
                        Severity::Error,
                    )
                })?;
                Value::Int128(r)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                let r = unsigned_int(a, b).ok_or_else(|| {
                    Diagnostic::new(
                        "integer overflow or invalid operation",
                        span,
                        Severity::Error,
                    )
                })?;
                Value::UInt128(r)
            }
            (Value::Float(a), Value::Float(b)) => {
                let r = float_op(a, b).ok_or_else(|| {
                    Diagnostic::new("floating-point operation invalid", span, Severity::Error)
                })?;
                Value::Float(r)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for arithmetic: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_div(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Int(
                a.checked_div(b)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            (Value::Int128(a), Value::Int128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                let r = a.checked_div(b).ok_or_else(|| {
                    Diagnostic::new("integer division overflow", span, Severity::Error)
                })?;
                Value::Int128(r)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                Value::UInt128(a / b)
            }
            (Value::Float(a), Value::Float(b)) => {
                if b == 0.0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                Value::Float(a / b)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for division: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_mod(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Int(
                a.checked_mod(b)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            (Value::Int128(a), Value::Int128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                }
                Value::Int128(a % b)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                }
                Value::UInt128(a % b)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for modulo: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_pow(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int(base), Value::Int(exp)) => Value::Int(
                base.checked_pow(exp)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            (Value::Int128(base), Value::Int128(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| {
                    Diagnostic::new("integer exponent out of range", span, Severity::Error)
                })?;
                Value::Int128(base.checked_pow(exp_u32).ok_or_else(|| {
                    Diagnostic::new("integer overflow in pow", span, Severity::Error)
                })?)
            }
            (Value::UInt128(base), Value::UInt128(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| {
                    Diagnostic::new("integer exponent out of range", span, Severity::Error)
                })?;
                Value::UInt128(base.checked_pow(exp_u32).ok_or_else(|| {
                    Diagnostic::new("integer overflow in pow", span, Severity::Error)
                })?)
            }
            (Value::Float(base), Value::Float(exp)) => Value::Float(base.powf(exp)),
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for power: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_cmp(
        &mut self,
        span: Span,
        predicate: fn(std::cmp::Ordering) -> bool,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        if let Some(ord) = compare_integer_values(&lhs, &rhs) {
            let ord = ord.map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
            self.stack.push(Value::Bool(predicate(ord)));
            return Ok(());
        }
        let ord = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => a
                .cmp_same_type(b)
                .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            (Value::Int128(a), Value::Int128(b)) => a.cmp(&b),
            (Value::UInt128(a), Value::UInt128(b)) => a.cmp(&b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(&b).ok_or_else(|| {
                Diagnostic::new("float comparison is invalid", span, Severity::Error)
            })?,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(&b),
            (Value::Char(a), Value::Char(b)) => a.cmp(&b),
            (Value::Str(a), Value::Str(b)) => a.cmp(&b),
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for comparison: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(Value::Bool(predicate(ord)));
        Ok(())
    }

    fn apply_eq(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        if let Some(ord) = compare_integer_values(&lhs, &rhs) {
            let ord = ord.map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
            self.stack
                .push(Value::Bool(ord == std::cmp::Ordering::Equal));
            return Ok(());
        }
        self.stack.push(Value::Bool(lhs == rhs));
        Ok(())
    }

    fn apply_ne(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        if let Some(ord) = compare_integer_values(&lhs, &rhs) {
            let ord = ord.map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
            self.stack
                .push(Value::Bool(ord != std::cmp::Ordering::Equal));
            return Ok(());
        }
        self.stack.push(Value::Bool(lhs != rhs));
        Ok(())
    }

    fn apply_logical(&mut self, span: Span, op: fn(bool, bool) -> bool) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Bool(a), Value::Bool(b)) => {
                self.stack.push(Value::Bool(op(a, b)));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for logical op: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bitwise(
        &mut self,
        span: Span,
        signed_op: fn(i128, i128) -> i128,
        unsigned_op: fn(u128, u128) -> u128,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if a.tag() != b.tag() {
                    return Err(Diagnostic::new(
                        "integer type mismatch",
                        span,
                        Severity::Error,
                    ));
                }
                let out = match (a.payload(), b.payload()) {
                    (super::int::IntPayload::Signed(x), super::int::IntPayload::Signed(y)) => {
                        super::int::TypedInt::try_from_signed(a.tag(), signed_op(x, y))
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    }
                    (super::int::IntPayload::Unsigned(x), super::int::IntPayload::Unsigned(y)) => {
                        super::int::TypedInt::try_from_unsigned(a.tag(), unsigned_op(x, y))
                            .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            "integer payload mismatch",
                            span,
                            Severity::Error,
                        ));
                    }
                };
                self.stack.push(Value::Int(out));
                Ok(())
            }
            (Value::Int128(a), Value::Int128(b)) => {
                self.stack.push(Value::Int128(signed_op(a, b)));
                Ok(())
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                self.stack.push(Value::UInt128(unsigned_op(a, b)));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for bitwise op: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_shift(&mut self, span: Span, is_left: bool) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                let out = a
                    .checked_shift(b, is_left)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                self.stack.push(Value::Int(out));
                Ok(())
            }
            (Value::Int128(a), Value::Int128(b)) => {
                let shift = u32::try_from(b).map_err(|_| {
                    Diagnostic::new("shift amount must be non-negative", span, Severity::Error)
                })?;
                let result = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or_else(|| {
                    Diagnostic::new("shift amount out of range", span, Severity::Error)
                })?;
                self.stack.push(Value::Int128(result));
                Ok(())
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                let shift = u32::try_from(b).map_err(|_| {
                    Diagnostic::new("shift amount out of range", span, Severity::Error)
                })?;
                let result = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or_else(|| {
                    Diagnostic::new("shift amount out of range", span, Severity::Error)
                })?;
                self.stack.push(Value::UInt128(result));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for shift: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn pop_one(&mut self, span: Span) -> Result<Value, Diagnostic> {
        self.stack.pop().ok_or_else(|| {
            Diagnostic::new(
                "stack underflow (internal bytecode error)",
                span,
                Severity::Error,
            )
        })
    }

    fn resolve_symbol_for_call(&mut self, sym: SymbolId, span: Span) -> Result<Value, Diagnostic> {
        if let Some(v) = self.locals_get(sym) {
            return Ok(v);
        }
        if let Some(v) = self.globals.borrow().get(&sym).cloned() {
            return Ok(v);
        }
        if let Some(current_function) = self.current_function.as_ref()
            && current_function.self_symbol == Some(sym)
        {
            return Ok(Value::Function {
                function: Arc::clone(&current_function.function),
                captured: Arc::new(self.snapshot_locals()),
                self_symbol: current_function.self_symbol,
            });
        }
        Err(Diagnostic::new(
            "load of uninitialized or missing symbol",
            span,
            Severity::Error,
        ))
    }

    fn invoke_callee(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        argc: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match callee {
            Value::Builtin(sym) => {
                let _symbol = self.symbols.symbol(sym).ok_or_else(|| {
                    Diagnostic::new("unknown builtin symbol", span, Severity::Error)
                })?;
                let ret = self.dispatch_intrinsic(sym, &args, span)?;
                self.stack.push(ret);
                Ok(())
            }
            Value::Function {
                function,
                captured,
                self_symbol,
            } => {
                let fn_chunk = Arc::clone(&function);
                if fn_chunk.params.len() != argc {
                    return Err(Diagnostic::new(
                        format!(
                            "invalid argument count: expected {}, got {}",
                            fn_chunk.params.len(),
                            argc
                        ),
                        span,
                        Severity::Error,
                    ));
                }

                let next_locals = if captured.is_empty() {
                    let mut locals = Vec::with_capacity(argc);
                    for (param, arg) in fn_chunk.params.iter().zip(args) {
                        locals.push((*param, arg));
                    }
                    locals
                } else {
                    let mut locals = Vec::with_capacity(captured.len() + argc);
                    for (sym, value) in captured.iter() {
                        locals.push((*sym, value.clone()));
                    }
                    for (param, arg) in fn_chunk.params.iter().zip(args) {
                        if let Some(idx) = locals
                            .iter()
                            .rposition(|(local_sym, _)| *local_sym == *param)
                        {
                            locals[idx].1 = arg;
                        } else {
                            locals.push((*param, arg));
                        }
                    }
                    locals
                };

                let current_function = self_symbol.map(|sym| FunctionContext {
                    function: Arc::clone(&function),
                    self_symbol: Some(sym),
                });
                let previous = CallFrame {
                    chunk: self.chunk.clone(),
                    ip: self.ip,
                    locals: std::mem::take(&mut self.locals),
                    scope_frames: std::mem::take(&mut self.scope_frames),
                    current_function: self.current_function.take(),
                };
                self.call_stack.push(previous);
                self.chunk = Arc::clone(&fn_chunk.chunk);
                self.ip = 0;
                self.locals = next_locals;
                self.scope_frames = vec![Vec::new()];
                self.current_function = current_function;
                Ok(())
            }
            other => Err(Diagnostic::new(
                format!("attempted call on non-function value: {:?}", other),
                span,
                Severity::Error,
            )),
        }
    }

    fn dispatch_intrinsic(
        &mut self,
        symbol_id: SymbolId,
        args: &[Value],
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let Some(_intrinsic) = self.intrinsic_ids.get(&symbol_id).copied() else {
            return Err(Diagnostic::new(
                "symbol is not registered as intrinsic",
                span,
                Severity::Error,
            ));
        };
        let symbol = self
            .symbols
            .symbol(symbol_id)
            .ok_or_else(|| Diagnostic::new("unknown intrinsic symbol", span, Severity::Error))?;
        dispatch_builtin(self, symbol.name.as_str(), args, span)
    }
}

fn dispatch_builtin(
    vm: &mut Vm<'_>,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, Diagnostic> {
    match name {
        "print" => {
            let line = args
                .iter()
                .map(Value::display_for_print)
                .collect::<Vec<_>>()
                .join(" ");
            println!("{line}");
            Ok(Value::Unit)
        }
        "len" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("len expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match arg {
                Value::Str(s) => {
                    let n = s.chars().count();
                    let n128 = i128::try_from(n).map_err(|_| {
                        Diagnostic::new("string length exceeds i128 range", span, Severity::Error)
                    })?;
                    Ok(Value::Int128(n128))
                }
                Value::Array(items) => {
                    let n128 = i128::try_from(items.len()).map_err(|_| {
                        Diagnostic::new("array length exceeds i128 range", span, Severity::Error)
                    })?;
                    Ok(Value::Int128(n128))
                }
                other => Err(Diagnostic::new(
                    format!("len expects str or array, got {}", builtin_type_name(other)),
                    span,
                    Severity::Error,
                )),
            }
        }
        "error" => {
            if args.len() != 1 && args.len() != 2 {
                return Err(Diagnostic::new(
                    format!("error expects 1 or 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let message = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "error message must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let code = if let Some(code_value) = args.get(1) {
                coerce_i32_code(code_value, span, "error")?
            } else {
                1
            };
            Ok(Value::Err {
                message,
                code,
                origin: "error".to_string(),
                cause: None,
            })
        }
        "panic" => {
            if args.len() != 1 && args.len() != 2 {
                return Err(Diagnostic::new(
                    format!("panic expects 1 or 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let message = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "panic message must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let code = if let Some(code_value) = args.get(1) {
                coerce_i32_code(code_value, span, "panic")?
            } else {
                1
            };
            Err(Diagnostic::new(
                format!("panic: {} (code={})", message, code),
                span,
                Severity::Error,
            ))
        }
        "wrap" => {
            if args.len() != 2 && args.len() != 3 {
                return Err(Diagnostic::new(
                    format!("wrap expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let base_err = args[0].clone();
            let message = match &args[1] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!("wrap message must be str, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let code = if let Some(code_value) = args.get(2) {
                Some(coerce_i32_code(code_value, span, "wrap")?)
            } else {
                None
            };
            wrap_err_value(
                base_err,
                message,
                code,
                "wrap".to_string(),
                span,
                &vm.chunk.struct_field_slots,
            )
        }
        "typeof" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("typeof expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(canonical_type_name(arg)))
        }
        "helper" => Ok(Value::Str("std/core ready".to_string())),
        "assert" => {
            if args.is_empty() || args.len() > 2 {
                return Err(Diagnostic::new(
                    format!("assert expects 1 or 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let cond = match &args[0] {
                Value::Bool(v) => *v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "assert condition must be bool, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            if cond {
                Ok(Value::Unit)
            } else {
                let message = match args.get(1) {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            format!(
                                "assert message must be str, got {}",
                                builtin_type_name(other)
                            ),
                            span,
                            Severity::Error,
                        ));
                    }
                    None => "assertion failed".to_string(),
                };
                Err(Diagnostic::new(
                    format!("panic: {message} (code=1)"),
                    span,
                    Severity::Error,
                ))
            }
        }
        "assert_eq_i32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(Diagnostic::new(
                    format!("assert_eq_i32 expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let a = as_i128(&args[0]).ok_or_else(|| {
                Diagnostic::new(
                    "assert_eq_i32 first arg must be i32-compatible",
                    span,
                    Severity::Error,
                )
            })?;
            let b = as_i128(&args[1]).ok_or_else(|| {
                Diagnostic::new(
                    "assert_eq_i32 second arg must be i32-compatible",
                    span,
                    Severity::Error,
                )
            })?;
            if a == b {
                Ok(Value::Unit)
            } else {
                let message = match args.get(2) {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            format!(
                                "assert_eq_i32 message must be str, got {}",
                                builtin_type_name(other)
                            ),
                            span,
                            Severity::Error,
                        ));
                    }
                    None => "i32 values differ".to_string(),
                };
                Err(Diagnostic::new(
                    format!("panic: {message} (code=1)"),
                    span,
                    Severity::Error,
                ))
            }
        }
        "assert_eq_str" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(Diagnostic::new(
                    format!("assert_eq_str expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let a = match &args[0] {
                Value::Str(v) => v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "assert_eq_str first arg must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let b = match &args[1] {
                Value::Str(v) => v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "assert_eq_str second arg must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            if a == b {
                Ok(Value::Unit)
            } else {
                let message = match args.get(2) {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            format!(
                                "assert_eq_str message must be str, got {}",
                                builtin_type_name(other)
                            ),
                            span,
                            Severity::Error,
                        ));
                    }
                    None => "str values differ".to_string(),
                };
                Err(Diagnostic::new(
                    format!("panic: {message} (code=1)"),
                    span,
                    Severity::Error,
                ))
            }
        }
        "sum_i32" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("sum_i32 expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let arr = match arg {
                Value::Array(items) => items,
                other => {
                    return Err(Diagnostic::new(
                        format!("sum_i32 expects array, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let mut acc: i128 = 0;
            for v in arr {
                let i = as_i128(v).ok_or_else(|| {
                    Diagnostic::new("sum_i32 array must contain integers", span, Severity::Error)
                })?;
                acc += i;
            }
            Ok(Value::Int128(acc))
        }
        "str_repeat" => {
            let [s, times] = args else {
                return Err(Diagnostic::new(
                    format!("str_repeat expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let s = match s {
                Value::Str(v) => v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "str_repeat first arg must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let n = as_i128(times).ok_or_else(|| {
                Diagnostic::new(
                    "str_repeat second arg must be i32-compatible",
                    span,
                    Severity::Error,
                )
            })?;
            if n <= 0 {
                return Ok(Value::Str(String::new()));
            }
            Ok(Value::Str(s.repeat(n as usize)))
        }
        "str_pad_left" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(Diagnostic::new(
                    format!("str_pad_left expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let s = match &args[0] {
                Value::Str(v) => v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "str_pad_left first arg must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let total = as_i128(&args[1]).ok_or_else(|| {
                Diagnostic::new(
                    "str_pad_left second arg must be i32-compatible",
                    span,
                    Severity::Error,
                )
            })?;
            let pad = match args.get(2) {
                Some(Value::Str(v)) => v.as_str(),
                Some(other) => {
                    return Err(Diagnostic::new(
                        format!(
                            "str_pad_left pad must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
                None => " ",
            };
            Ok(Value::Str(str_pad_impl(s, total, pad, true)))
        }
        "str_pad_right" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(Diagnostic::new(
                    format!("str_pad_right expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let s = match &args[0] {
                Value::Str(v) => v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "str_pad_right first arg must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let total = as_i128(&args[1]).ok_or_else(|| {
                Diagnostic::new(
                    "str_pad_right second arg must be i32-compatible",
                    span,
                    Severity::Error,
                )
            })?;
            let pad = match args.get(2) {
                Some(Value::Str(v)) => v.as_str(),
                Some(other) => {
                    return Err(Diagnostic::new(
                        format!(
                            "str_pad_right pad must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
                None => " ",
            };
            Ok(Value::Str(str_pad_impl(s, total, pad, false)))
        }
        "count_true" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("count_true expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let arr = match arg {
                Value::Array(items) => items,
                other => {
                    return Err(Diagnostic::new(
                        format!("count_true expects array, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let mut count: i128 = 0;
            for v in arr {
                match v {
                    Value::Bool(true) => count += 1,
                    Value::Bool(false) => {}
                    _ => {
                        return Err(Diagnostic::new(
                            "count_true array must contain bool values",
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Ok(Value::Int128(count))
        }
        "io_read_text" | "read_text" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_read_text expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = match path {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_read_text path must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            match std::fs::read_to_string(path) {
                Ok(content) => Ok(Value::Tuple(vec![Value::Str(content), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("io_read_text", e),
                ])),
            }
        }
        "io_write_text" | "write_text" => {
            let [path, content] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_write_text expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = match path {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_write_text path must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let content = match content {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_write_text content must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            match std::fs::write(path, content) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("io_write_text", e)),
            }
        }
        "io_append_text" | "append_text" => {
            let [path, content] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_append_text expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = match path {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_append_text path must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let content = match content {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_append_text content must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let res = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, content.as_bytes()));
            match res {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("io_append_text", e)),
            }
        }
        "io_exists" | "exists" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("io_exists expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = match path {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_exists path must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            Ok(Value::Bool(std::path::Path::new(path).exists()))
        }
        "io_remove_file" | "remove_file" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_remove_file expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = match path {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_remove_file path must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            match std::fs::remove_file(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("io_remove_file", e)),
            }
        }
        "io_read_stdin_line" | "stdin_read_line" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("io_read_stdin_line expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(_) => {
                    while line.ends_with('\n') || line.ends_with('\r') {
                        line.pop();
                    }
                    Ok(Value::Tuple(vec![Value::Str(line), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("io_read_stdin_line", e),
                ])),
            }
        }
        "io_stdout_write" | "stdout_write" => {
            let [text] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_stdout_write expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let text = match text {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_stdout_write text must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            print!("{text}");
            Ok(Value::Unit)
        }
        "io_stderr_write" | "stderr_write" => {
            let [text] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "io_stderr_write expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let text = match text {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "io_stderr_write text must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            eprint!("{text}");
            Ok(Value::Unit)
        }
        "stdout_writeln" => {
            let [text] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "stdout_writeln expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let text = match text {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "stdout_writeln text must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            println!("{text}");
            Ok(Value::Unit)
        }
        "stderr_writeln" => {
            let [text] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "stderr_writeln expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let text = match text {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "stderr_writeln text must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            eprintln!("{text}");
            Ok(Value::Unit)
        }
        "buffer_new" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("buffer_new expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(String::new()))
        }
        "buffer_write" => {
            let [buffer, chunk] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "buffer_write expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let buffer = match buffer {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "buffer_write buffer must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let chunk = match chunk {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "buffer_write chunk must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            Ok(Value::Str(format!("{buffer}{chunk}")))
        }
        "buffer_writeln" => {
            let [buffer, line] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "buffer_writeln expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let buffer = match buffer {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "buffer_writeln buffer must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let line = match line {
                Value::Str(s) => s,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "buffer_writeln line must be str, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            Ok(Value::Str(format!("{buffer}{line}\n")))
        }
        "buffer_clear" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("buffer_clear expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(String::new()))
        }
        "fs_exists" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("fs_exists expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_exists path", span)?;
            Ok(Value::Bool(std::path::Path::new(path).exists()))
        }
        "fs_is_file" | "is_file" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("fs_is_file expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_is_file path", span)?;
            Ok(Value::Bool(std::path::Path::new(path).is_file()))
        }
        "fs_is_dir" | "is_dir" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("fs_is_dir expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_is_dir path", span)?;
            Ok(Value::Bool(std::path::Path::new(path).is_dir()))
        }
        "fs_create_dir" | "create_dir" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_create_dir expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_create_dir path", span)?;
            match std::fs::create_dir(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_create_dir", e)),
            }
        }
        "fs_create_dir_all" | "create_dir_all" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_create_dir_all expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_create_dir_all path", span)?;
            match std::fs::create_dir_all(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_create_dir_all", e)),
            }
        }
        "fs_read_dir" | "read_dir" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("fs_read_dir expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_read_dir path", span)?;
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let mut out = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(e) => out.push(Value::Str(e.path().to_string_lossy().to_string())),
                            Err(e) => {
                                return Ok(Value::Tuple(vec![
                                    Value::Array(Vec::new()),
                                    io_err("fs_read_dir", e),
                                ]));
                            }
                        }
                    }
                    Ok(Value::Tuple(vec![Value::Array(out), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Array(Vec::new()),
                    io_err("fs_read_dir", e),
                ])),
            }
        }
        "fs_remove_file" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_remove_file expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_remove_file path", span)?;
            match std::fs::remove_file(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_remove_file", e)),
            }
        }
        "fs_remove_dir" | "remove_dir" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_remove_dir expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_remove_dir path", span)?;
            match std::fs::remove_dir(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_remove_dir", e)),
            }
        }
        "fs_remove_dir_all" | "remove_dir_all" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_remove_dir_all expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_remove_dir_all path", span)?;
            match std::fs::remove_dir_all(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_remove_dir_all", e)),
            }
        }
        "fs_rename" | "rename" => {
            let [from, to] = args else {
                return Err(Diagnostic::new(
                    format!("fs_rename expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let from = expect_str_arg(from, "fs_rename from", span)?;
            let to = expect_str_arg(to, "fs_rename to", span)?;
            match std::fs::rename(from, to) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_rename", e)),
            }
        }
        "fs_copy" | "copy" => {
            let [from, to] = args else {
                return Err(Diagnostic::new(
                    format!("fs_copy expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let from = expect_str_arg(from, "fs_copy from", span)?;
            let to = expect_str_arg(to, "fs_copy to", span)?;
            match std::fs::copy(from, to) {
                Ok(n) => Ok(Value::Tuple(vec![Value::UInt128(n as u128), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![Value::UInt128(0), io_err("fs_copy", e)])),
            }
        }
        "fs_cwd" | "cwd" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("fs_cwd expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match std::env::current_dir() {
                Ok(p) => Ok(Value::Tuple(vec![
                    Value::Str(p.to_string_lossy().to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("fs_cwd", e),
                ])),
            }
        }
        "fs_set_cwd" | "set_cwd" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("fs_set_cwd expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_set_cwd path", span)?;
            match std::env::set_current_dir(path) {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("fs_set_cwd", e)),
            }
        }
        "fs_path_join" | "path_join" => {
            let [a, b] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_join expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let a = expect_str_arg(a, "fs_path_join left", span)?;
            let b = expect_str_arg(b, "fs_path_join right", span)?;
            Ok(Value::Str(
                std::path::Path::new(a)
                    .join(b)
                    .to_string_lossy()
                    .to_string(),
            ))
        }
        "fs_path_parent" | "path_parent" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_parent expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_parent path", span)?;
            let p = std::path::Path::new(path);
            match p.parent() {
                Some(parent) => Ok(Value::Tuple(vec![
                    Value::Str(parent.to_string_lossy().to_string()),
                    Value::Null,
                ])),
                None => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "path has no parent".to_string(),
                        code: 1,
                        origin: "fs_path_parent".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "fs_path_filename" | "path_filename" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_filename expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_filename path", span)?;
            let p = std::path::Path::new(path);
            match p.file_name().and_then(|s| s.to_str()) {
                Some(name) => Ok(Value::Tuple(vec![
                    Value::Str(name.to_string()),
                    Value::Null,
                ])),
                None => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "path has no filename".to_string(),
                        code: 1,
                        origin: "fs_path_filename".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "fs_path_extension" | "path_extension" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_extension expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_extension path", span)?;
            let p = std::path::Path::new(path);
            match p.extension().and_then(|s| s.to_str()) {
                Some(ext) => Ok(Value::Tuple(vec![Value::Str(ext.to_string()), Value::Null])),
                None => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "path has no extension".to_string(),
                        code: 1,
                        origin: "fs_path_extension".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "fs_path_stem" | "path_stem" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_stem expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_stem path", span)?;
            let p = std::path::Path::new(path);
            match p.file_stem().and_then(|s| s.to_str()) {
                Some(stem) => Ok(Value::Tuple(vec![
                    Value::Str(stem.to_string()),
                    Value::Null,
                ])),
                None => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "path has no stem".to_string(),
                        code: 1,
                        origin: "fs_path_stem".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "fs_path_normalize" | "path_normalize" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_normalize expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_normalize path", span)?;
            Ok(Value::Str(path_normalize_lex(path)))
        }
        "fs_path_is_abs" | "path_is_abs" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_is_abs expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_is_abs path", span)?;
            Ok(Value::Bool(std::path::Path::new(path).is_absolute()))
        }
        "fs_path_is_relative" | "path_is_relative" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_path_is_relative expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_path_is_relative path", span)?;
            Ok(Value::Bool(std::path::Path::new(path).is_relative()))
        }
        "fs_metadata_len" | "metadata_len" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_metadata_len expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_metadata_len path", span)?;
            match std::fs::metadata(path) {
                Ok(meta) => Ok(Value::Tuple(vec![
                    Value::UInt128(meta.len() as u128),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    io_err("fs_metadata_len", e),
                ])),
            }
        }
        "fs_metadata_readonly" | "metadata_readonly" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_metadata_readonly expects exactly 1 argument, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_metadata_readonly path", span)?;
            match std::fs::metadata(path) {
                Ok(meta) => Ok(Value::Tuple(vec![
                    Value::Bool(meta.permissions().readonly()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Bool(false),
                    io_err("fs_metadata_readonly", e),
                ])),
            }
        }
        "fs_set_readonly" | "set_readonly" => {
            let [path, readonly] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "fs_set_readonly expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "fs_set_readonly path", span)?;
            let readonly = match readonly {
                Value::Bool(v) => *v,
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "fs_set_readonly readonly must be bool, got {}",
                            builtin_type_name(other)
                        ),
                        span,
                        Severity::Error,
                    ));
                }
            };
            match std::fs::metadata(path) {
                Ok(meta) => {
                    let mut perms = meta.permissions();
                    perms.set_readonly(readonly);
                    match std::fs::set_permissions(path, perms) {
                        Ok(_) => Ok(Value::Null),
                        Err(e) => Ok(io_err("fs_set_readonly", e)),
                    }
                }
                Err(e) => Ok(io_err("fs_set_readonly", e)),
            }
        }
        "math_pi" | "pi" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("math_pi expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Float(std::f64::consts::PI))
        }
        "math_e" | "e" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("math_e expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Float(std::f64::consts::E))
        }
        "math_tau" | "tau" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("math_tau expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Float(std::f64::consts::TAU))
        }
        "math_abs" | "abs" | "math_sqrt" | "sqrt" | "math_exp" | "exp" | "math_log" | "log"
        | "math_log10" | "log10" | "math_floor" | "floor" | "math_ceil" | "ceil" | "math_round"
        | "round" | "math_trunc" | "trunc" | "math_fract" | "fract" | "math_sin" | "sin"
        | "math_cos" | "cos" | "math_tan" | "tan" | "math_asin" | "asin" | "math_acos" | "acos"
        | "math_atan" | "atan" => {
            let [x] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let x = expect_f64_arg(x, name, span)?;
            let out = match name {
                "math_abs" | "abs" => x.abs(),
                "math_sqrt" | "sqrt" => x.sqrt(),
                "math_exp" | "exp" => x.exp(),
                "math_log" | "log" => x.ln(),
                "math_log10" | "log10" => x.log10(),
                "math_floor" | "floor" => x.floor(),
                "math_ceil" | "ceil" => x.ceil(),
                "math_round" | "round" => x.round(),
                "math_trunc" | "trunc" => x.trunc(),
                "math_fract" | "fract" => x.fract(),
                "math_sin" | "sin" => x.sin(),
                "math_cos" | "cos" => x.cos(),
                "math_tan" | "tan" => x.tan(),
                "math_asin" | "asin" => x.asin(),
                "math_acos" | "acos" => x.acos(),
                "math_atan" | "atan" => x.atan(),
                _ => x,
            };
            Ok(Value::Float(out))
        }
        "math_pow" | "pow" => {
            let [base, exp] = args else {
                return Err(Diagnostic::new(
                    format!("math_pow expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let base = expect_f64_arg(base, "math_pow base", span)?;
            let exp = expect_f64_arg(exp, "math_pow exp", span)?;
            Ok(Value::Float(base.powf(exp)))
        }
        "math_rand_f64" | "rand_f64" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("math_rand_f64 expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let n = math_next_u64();
            Ok(Value::Float((n as f64) / (u64::MAX as f64)))
        }
        "math_rand_i32" | "rand_i32" => {
            let [min, max] = args else {
                return Err(Diagnostic::new(
                    format!(
                        "math_rand_i32 expects exactly 2 arguments, got {}",
                        args.len()
                    ),
                    span,
                    Severity::Error,
                ));
            };
            let min = expect_i32_like(min, "math_rand_i32 min", span)?;
            let max = expect_i32_like(max, "math_rand_i32 max", span)?;
            if min > max {
                return Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Err {
                        message: "min must be <= max".to_string(),
                        code: 1,
                        origin: "math_rand_i32".to_string(),
                        cause: None,
                    },
                ]));
            }
            let width = (max as i64 - min as i64 + 1) as u64;
            let n = math_next_u64() % width;
            let v = min as i64 + n as i64;
            Ok(Value::Tuple(vec![Value::Int128(v as i128), Value::Null]))
        }
        "time_now_unix_secs" | "now_unix_secs" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(d) => Ok(Value::UInt128(d.as_secs() as u128)),
                Err(_) => Ok(Value::UInt128(0)),
            }
        }
        "time_now_unix_millis" | "now_unix_millis" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(d) => Ok(Value::UInt128(d.as_millis())),
                Err(_) => Ok(Value::UInt128(0)),
            }
        }
        "time_sleep_ms" | "sleep_ms" => {
            let [ms] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let ms = expect_u64_arg(ms, "sleep_ms ms", span)?;
            std::thread::sleep(Duration::from_millis(ms));
            Ok(Value::Null)
        }
        "time_tick_ms" | "tick_ms" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let start = time_tick_start();
            Ok(Value::UInt128(start.elapsed().as_millis()))
        }
        "time_elapsed_ms" | "elapsed_ms" => {
            let [start_ms] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let start_ms = expect_u64_arg(start_ms, "elapsed_ms start", span)?;
            let now_ms = time_tick_start()
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
            let elapsed = now_ms.saturating_sub(start_ms);
            Ok(Value::UInt128(elapsed as u128))
        }
        "time_now_iso_utc" | "now_iso_utc" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(d) => Ok(Value::Tuple(vec![
                    Value::Str(format_unix_secs_iso_utc(d.as_secs())),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("time_now_iso_utc", std::io::Error::other(e.to_string())),
                ])),
            }
        }
        "time_from_unix_secs_iso_utc" | "from_unix_secs_iso_utc" => {
            let [secs] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let secs = expect_u64_arg(secs, "from_unix_secs_iso_utc secs", span)?;
            Ok(Value::Tuple(vec![
                Value::Str(format_unix_secs_iso_utc(secs)),
                Value::Null,
            ]))
        }
        "os_platform" | "platform" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(std::env::consts::OS.to_string()))
        }
        "os_arch" | "arch" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(std::env::consts::ARCH.to_string()))
        }
        "os_pid" | "pid" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::UInt128(std::process::id() as u128))
        }
        "os_getenv" | "getenv" => {
            let [key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "getenv key", span)?;
            match std::env::var(key) {
                Ok(v) => Ok(Value::Tuple(vec![Value::Str(v), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "getenv".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "os_setenv" | "setenv" => {
            let [key, value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "setenv key", span)?;
            let value = expect_str_arg(value, "setenv value", span)?;
            // SAFETY: process-level env mutation is intended std/os behavior for Pandora.
            unsafe { std::env::set_var(key, value) };
            Ok(Value::Null)
        }
        "os_unsetenv" | "unsetenv" => {
            let [key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "unsetenv key", span)?;
            // SAFETY: process-level env mutation is intended std/os behavior for Pandora.
            unsafe { std::env::remove_var(key) };
            Ok(Value::Null)
        }
        "os_args" | "args" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Array(
                std::env::args().map(Value::Str).collect::<Vec<_>>(),
            ))
        }
        "os_exec" | "exec" => {
            let [command] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let command = expect_str_arg(command, "exec command", span)?;
            #[cfg(target_os = "windows")]
            let output = std::process::Command::new("cmd")
                .args(["/C", command])
                .output();
            #[cfg(not(target_os = "windows"))]
            let output = std::process::Command::new("sh")
                .args(["-c", command])
                .output();
            match output {
                Ok(out) => {
                    let code = out.status.code().unwrap_or(-1);
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    Ok(Value::Tuple(vec![
                        Value::Int128(code as i128),
                        Value::Str(stdout),
                        Value::Str(stderr),
                        Value::Null,
                    ]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    io_err("exec", e),
                ])),
            }
        }
        "os_signal_term" | "signal_term" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Int128(15))
        }
        "os_signal_kill" | "signal_kill" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Int128(9))
        }
        "os_signal_int" | "signal_int" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Int128(2))
        }
        "os_send_signal" | "send_signal" => {
            let [pid, signal] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pid = expect_u64_arg(pid, "send_signal pid", span)?;
            let signal = expect_i32_like(signal, "send_signal signal", span)?;
            #[cfg(not(target_os = "windows"))]
            {
                let status = std::process::Command::new("kill")
                    .args(["-s", &signal.to_string(), &pid.to_string()])
                    .status();
                match status {
                    Ok(s) if s.success() => Ok(Value::Null),
                    Ok(_) => Ok(Value::Err {
                        message: "kill command failed".to_string(),
                        code: 1,
                        origin: "send_signal".to_string(),
                        cause: None,
                    }),
                    Err(e) => Ok(io_err("send_signal", e)),
                }
            }
            #[cfg(target_os = "windows")]
            {
                let _ = (pid, signal);
                Ok(Value::Err {
                    message: "send_signal is unsupported on windows backend".to_string(),
                    code: 1,
                    origin: "send_signal".to_string(),
                    cause: None,
                })
            }
        }
        "proc_spawn" | "spawn" => {
            let [command] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let command = expect_str_arg(command, "spawn command", span)?;
            let child = spawn_shell_command(command).map_err(|e| {
                Diagnostic::new(
                    format!("failed to spawn process: {e}"),
                    span,
                    Severity::Error,
                )
            })?;
            let pid = child.id();
            let mut table = proc_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock process table", span, Severity::Error)
            })?;
            table.insert(pid, child);
            Ok(Value::Tuple(vec![Value::UInt128(pid as u128), Value::Null]))
        }
        "proc_wait" | "wait" => {
            let [pid] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pid = expect_u64_arg(pid, "wait pid", span)?;
            let Some(mut child) = proc_take_child(pid, span)? else {
                return Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Err {
                        message: format!("unknown child pid {pid}"),
                        code: 1,
                        origin: "wait".to_string(),
                        cause: None,
                    },
                ]));
            };
            match child.wait() {
                Ok(status) => Ok(Value::Tuple(vec![
                    Value::Int128(status.code().unwrap_or(-1) as i128),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![Value::Int128(-1), io_err("wait", e)])),
            }
        }
        "proc_kill" | "kill" => {
            let [pid] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pid = expect_u64_arg(pid, "kill pid", span)?;
            let mut table = proc_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock process table", span, Severity::Error)
            })?;
            let Some(child) = table.get_mut(&(pid as u32)) else {
                return Ok(Value::Err {
                    message: format!("unknown child pid {pid}"),
                    code: 1,
                    origin: "kill".to_string(),
                    cause: None,
                });
            };
            match child.kill() {
                Ok(_) => Ok(Value::Null),
                Err(e) => Ok(io_err("kill", e)),
            }
        }
        "proc_exec" | "exec_proc" => {
            let [command] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let command = expect_str_arg(command, "exec_proc command", span)?;
            match run_shell_command(command) {
                Ok(out) => Ok(Value::Tuple(vec![
                    Value::Int128(out.status.code().unwrap_or(-1) as i128),
                    Value::Str(String::from_utf8_lossy(&out.stdout).to_string()),
                    Value::Str(String::from_utf8_lossy(&out.stderr).to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    io_err("exec_proc", e),
                ])),
            }
        }
        "proc_pipe" | "pipe" => {
            let [left, right] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let left = expect_str_arg(left, "pipe left", span)?;
            let right = expect_str_arg(right, "pipe right", span)?;
            let command = format!("{left} | {right}");
            match run_shell_command(&command) {
                Ok(out) => Ok(Value::Tuple(vec![
                    Value::Int128(out.status.code().unwrap_or(-1) as i128),
                    Value::Str(String::from_utf8_lossy(&out.stdout).to_string()),
                    Value::Str(String::from_utf8_lossy(&out.stderr).to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    io_err("pipe", e),
                ])),
            }
        }
        "thread_spawn" | "spawn_thread" => {
            let [command] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let command = expect_str_arg(command, "spawn_thread command", span)?.to_string();
            let tid = next_thread_id();
            let handle = std::thread::spawn(move || match run_shell_command(&command) {
                Ok(output) => output.status.code().unwrap_or(-1),
                Err(_) => -1,
            });
            let mut table = thread_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock thread table", span, Severity::Error)
            })?;
            table.insert(tid, handle);
            Ok(Value::Tuple(vec![Value::UInt128(tid as u128), Value::Null]))
        }
        "thread_join" | "join_thread" => {
            let [tid] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let tid = expect_u64_arg(tid, "join_thread tid", span)?;
            let handle = {
                let mut table = thread_table().lock().map_err(|_| {
                    Diagnostic::new("failed to lock thread table", span, Severity::Error)
                })?;
                table.remove(&tid)
            };
            let Some(handle) = handle else {
                return Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Err {
                        message: format!("unknown thread id {tid}"),
                        code: 1,
                        origin: "join_thread".to_string(),
                        cause: None,
                    },
                ]));
            };
            match handle.join() {
                Ok(code) => Ok(Value::Tuple(vec![Value::Int128(code as i128), Value::Null])),
                Err(_) => Ok(Value::Tuple(vec![
                    Value::Int128(-1),
                    Value::Err {
                        message: "thread panicked during join".to_string(),
                        code: 1,
                        origin: "join_thread".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "thread_sleep_ms" | "sleep_thread_ms" => {
            let [ms] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let ms = expect_u64_arg(ms, "sleep_thread_ms ms", span)?;
            std::thread::sleep(Duration::from_millis(ms));
            Ok(Value::Null)
        }
        "thread_mutex_new" | "mutex_new" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = next_mutex_id();
            let mut table = mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock mutex table", span, Severity::Error)
            })?;
            table.insert(id, false);
            Ok(Value::UInt128(id as u128))
        }
        "thread_mutex_lock" | "mutex_lock" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "mutex_lock id", span)?;
            let mut table = mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock mutex table", span, Severity::Error)
            })?;
            let Some(locked) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown mutex id {id}"),
                    code: 1,
                    origin: "mutex_lock".to_string(),
                    cause: None,
                });
            };
            if *locked {
                return Ok(Value::Err {
                    message: "mutex already locked".to_string(),
                    code: 1,
                    origin: "mutex_lock".to_string(),
                    cause: None,
                });
            }
            *locked = true;
            Ok(Value::Null)
        }
        "thread_mutex_try_lock" | "mutex_try_lock" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "mutex_try_lock id", span)?;
            let mut table = mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock mutex table", span, Severity::Error)
            })?;
            let Some(locked) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Bool(false),
                    Value::Err {
                        message: format!("unknown mutex id {id}"),
                        code: 1,
                        origin: "mutex_try_lock".to_string(),
                        cause: None,
                    },
                ]));
            };
            if *locked {
                Ok(Value::Tuple(vec![Value::Bool(false), Value::Null]))
            } else {
                *locked = true;
                Ok(Value::Tuple(vec![Value::Bool(true), Value::Null]))
            }
        }
        "thread_mutex_unlock" | "mutex_unlock" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "mutex_unlock id", span)?;
            let mut table = mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock mutex table", span, Severity::Error)
            })?;
            let Some(locked) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown mutex id {id}"),
                    code: 1,
                    origin: "mutex_unlock".to_string(),
                    cause: None,
                });
            };
            if !*locked {
                return Ok(Value::Err {
                    message: "mutex is not locked".to_string(),
                    code: 1,
                    origin: "mutex_unlock".to_string(),
                    cause: None,
                });
            }
            *locked = false;
            Ok(Value::Null)
        }
        "sync_mutex_new" | "mutex_new_sync" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = next_sync_mutex_id();
            let mut table = sync_mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock sync mutex table", span, Severity::Error)
            })?;
            table.insert(id, false);
            Ok(Value::UInt128(id as u128))
        }
        "sync_mutex_lock" | "mutex_lock_sync" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "mutex_lock_sync id", span)?;
            let mut table = sync_mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock sync mutex table", span, Severity::Error)
            })?;
            let Some(locked) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown sync mutex id {id}"),
                    code: 1,
                    origin: "mutex_lock_sync".to_string(),
                    cause: None,
                });
            };
            if *locked {
                return Ok(Value::Err {
                    message: "sync mutex already locked".to_string(),
                    code: 1,
                    origin: "mutex_lock_sync".to_string(),
                    cause: None,
                });
            }
            *locked = true;
            Ok(Value::Null)
        }
        "sync_mutex_unlock" | "mutex_unlock_sync" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "mutex_unlock_sync id", span)?;
            let mut table = sync_mutex_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock sync mutex table", span, Severity::Error)
            })?;
            let Some(locked) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown sync mutex id {id}"),
                    code: 1,
                    origin: "mutex_unlock_sync".to_string(),
                    cause: None,
                });
            };
            if !*locked {
                return Ok(Value::Err {
                    message: "sync mutex is not locked".to_string(),
                    code: 1,
                    origin: "mutex_unlock_sync".to_string(),
                    cause: None,
                });
            }
            *locked = false;
            Ok(Value::Null)
        }
        "sync_atomic_i64_new" | "atomic_i64_new" => {
            let [initial] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let initial = expect_i64_like(initial, "atomic_i64_new initial", span)?;
            let id = next_atomic_id();
            let mut table = atomic_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock atomic table", span, Severity::Error)
            })?;
            table.insert(id, initial);
            Ok(Value::UInt128(id as u128))
        }
        "sync_atomic_i64_load" | "atomic_i64_load" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "atomic_i64_load id", span)?;
            let table = atomic_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock atomic table", span, Severity::Error)
            })?;
            let Some(v) = table.get(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Err {
                        message: format!("unknown atomic id {id}"),
                        code: 1,
                        origin: "atomic_i64_load".to_string(),
                        cause: None,
                    },
                ]));
            };
            Ok(Value::Tuple(vec![Value::Int128(*v as i128), Value::Null]))
        }
        "sync_atomic_i64_store" | "atomic_i64_store" => {
            let [id, value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "atomic_i64_store id", span)?;
            let value = expect_i64_like(value, "atomic_i64_store value", span)?;
            let mut table = atomic_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock atomic table", span, Severity::Error)
            })?;
            let Some(slot) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown atomic id {id}"),
                    code: 1,
                    origin: "atomic_i64_store".to_string(),
                    cause: None,
                });
            };
            *slot = value;
            Ok(Value::Null)
        }
        "sync_atomic_i64_add" | "atomic_i64_add" => {
            let [id, delta] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "atomic_i64_add id", span)?;
            let delta = expect_i64_like(delta, "atomic_i64_add delta", span)?;
            let mut table = atomic_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock atomic table", span, Severity::Error)
            })?;
            let Some(slot) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Err {
                        message: format!("unknown atomic id {id}"),
                        code: 1,
                        origin: "atomic_i64_add".to_string(),
                        cause: None,
                    },
                ]));
            };
            *slot = slot.saturating_add(delta);
            Ok(Value::Tuple(vec![
                Value::Int128(*slot as i128),
                Value::Null,
            ]))
        }
        "sync_channel_new" | "channel_new" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = next_channel_id();
            let mut table = channel_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock channel table", span, Severity::Error)
            })?;
            table.insert(id, VecDeque::new());
            Ok(Value::UInt128(id as u128))
        }
        "sync_channel_send" | "channel_send" => {
            let [id, value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "channel_send id", span)?;
            let mut table = channel_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock channel table", span, Severity::Error)
            })?;
            let Some(queue) = table.get_mut(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown channel id {id}"),
                    code: 1,
                    origin: "channel_send".to_string(),
                    cause: None,
                });
            };
            queue.push_back(value.clone());
            Ok(Value::Null)
        }
        "sync_channel_recv" | "channel_recv" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "channel_recv id", span)?;
            let mut table = channel_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock channel table", span, Severity::Error)
            })?;
            let Some(queue) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: format!("unknown channel id {id}"),
                        code: 1,
                        origin: "channel_recv".to_string(),
                        cause: None,
                    },
                ]));
            };
            match queue.pop_front() {
                Some(v) => Ok(Value::Tuple(vec![v, Value::Null])),
                None => Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: "channel is empty".to_string(),
                        code: 1,
                        origin: "channel_recv".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "sync_channel_try_recv" | "channel_try_recv" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "channel_try_recv id", span)?;
            let mut table = channel_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock channel table", span, Severity::Error)
            })?;
            let Some(queue) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Bool(false),
                    Value::Err {
                        message: format!("unknown channel id {id}"),
                        code: 1,
                        origin: "channel_try_recv".to_string(),
                        cause: None,
                    },
                ]));
            };
            match queue.pop_front() {
                Some(v) => Ok(Value::Tuple(vec![v, Value::Bool(true), Value::Null])),
                None => Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Bool(false),
                    Value::Null,
                ])),
            }
        }
        "net_dns_lookup" | "dns_lookup" => {
            let [host] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let host = expect_str_arg(host, "dns_lookup host", span)?;
            match (host, 0).to_socket_addrs() {
                Ok(iter) => {
                    let mut out = Vec::new();
                    for addr in iter {
                        out.push(Value::Str(addr.ip().to_string()));
                    }
                    Ok(Value::Tuple(vec![Value::Array(out), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Array(vec![]),
                    io_err("dns_lookup", e),
                ])),
            }
        }
        "net_udp_bind" | "udp_bind" => {
            let [addr] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let addr = expect_str_arg(addr, "udp_bind addr", span)?;
            match UdpSocket::bind(addr) {
                Ok(sock) => {
                    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
                    let id = next_udp_socket_id();
                    let mut table = udp_socket_table().lock().map_err(|_| {
                        Diagnostic::new("failed to lock udp socket table", span, Severity::Error)
                    })?;
                    table.insert(id, sock);
                    Ok(Value::Tuple(vec![Value::UInt128(id as u128), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![Value::UInt128(0), io_err("udp_bind", e)])),
            }
        }
        "net_udp_local_addr" | "udp_local_addr" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "udp_local_addr id", span)?;
            let table = udp_socket_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock udp socket table", span, Severity::Error)
            })?;
            let Some(sock) = table.get(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: format!("unknown udp socket id {id}"),
                        code: 1,
                        origin: "udp_local_addr".to_string(),
                        cause: None,
                    },
                ]));
            };
            match sock.local_addr() {
                Ok(addr) => Ok(Value::Tuple(vec![
                    Value::Str(addr.to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("udp_local_addr", e),
                ])),
            }
        }
        "net_udp_send_to" | "udp_send_to" => {
            let [id, payload, to] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "udp_send_to id", span)?;
            let payload = expect_str_arg(payload, "udp_send_to payload", span)?;
            let to = expect_str_arg(to, "udp_send_to to", span)?;
            let table = udp_socket_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock udp socket table", span, Severity::Error)
            })?;
            let Some(sock) = table.get(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    Value::Err {
                        message: format!("unknown udp socket id {id}"),
                        code: 1,
                        origin: "udp_send_to".to_string(),
                        cause: None,
                    },
                ]));
            };
            match sock.send_to(payload.as_bytes(), to) {
                Ok(n) => Ok(Value::Tuple(vec![Value::UInt128(n as u128), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    io_err("udp_send_to", e),
                ])),
            }
        }
        "net_udp_recv_from" | "udp_recv_from" => {
            let [id, max] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "udp_recv_from id", span)?;
            let max = expect_u64_arg(max, "udp_recv_from max", span)? as usize;
            let table = udp_socket_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock udp socket table", span, Severity::Error)
            })?;
            let Some(sock) = table.get(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    Value::Err {
                        message: format!("unknown udp socket id {id}"),
                        code: 1,
                        origin: "udp_recv_from".to_string(),
                        cause: None,
                    },
                ]));
            };
            let mut buf = vec![0_u8; max.max(1)];
            match sock.recv_from(&mut buf) {
                Ok((n, from)) => Ok(Value::Tuple(vec![
                    Value::Str(String::from_utf8_lossy(&buf[..n]).to_string()),
                    Value::Str(from.to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    io_err("udp_recv_from", e),
                ])),
            }
        }
        "net_tcp_connect" | "tcp_connect" => {
            let [addr] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let addr = expect_str_arg(addr, "tcp_connect addr", span)?;
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let id = next_tcp_stream_id();
                    let mut table = tcp_stream_table().lock().map_err(|_| {
                        Diagnostic::new("failed to lock tcp stream table", span, Severity::Error)
                    })?;
                    table.insert(id, stream);
                    Ok(Value::Tuple(vec![Value::UInt128(id as u128), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    io_err("tcp_connect", e),
                ])),
            }
        }
        "net_tcp_send" | "tcp_send" => {
            let [id, payload] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "tcp_send id", span)?;
            let payload = expect_str_arg(payload, "tcp_send payload", span)?;
            let mut table = tcp_stream_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock tcp stream table", span, Severity::Error)
            })?;
            let Some(stream) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    Value::Err {
                        message: format!("unknown tcp stream id {id}"),
                        code: 1,
                        origin: "tcp_send".to_string(),
                        cause: None,
                    },
                ]));
            };
            match stream.write(payload.as_bytes()) {
                Ok(n) => Ok(Value::Tuple(vec![Value::UInt128(n as u128), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![Value::UInt128(0), io_err("tcp_send", e)])),
            }
        }
        "net_tcp_recv" | "tcp_recv" => {
            let [id, max] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "tcp_recv id", span)?;
            let max = expect_u64_arg(max, "tcp_recv max", span)? as usize;
            let mut table = tcp_stream_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock tcp stream table", span, Severity::Error)
            })?;
            let Some(stream) = table.get_mut(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: format!("unknown tcp stream id {id}"),
                        code: 1,
                        origin: "tcp_recv".to_string(),
                        cause: None,
                    },
                ]));
            };
            let mut buf = vec![0_u8; max.max(1)];
            match stream.read(&mut buf) {
                Ok(n) => Ok(Value::Tuple(vec![
                    Value::Str(String::from_utf8_lossy(&buf[..n]).to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("tcp_recv", e),
                ])),
            }
        }
        "net_tcp_close" | "tcp_close" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "tcp_close id", span)?;
            let mut table = tcp_stream_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock tcp stream table", span, Severity::Error)
            })?;
            if table.remove(&id).is_some() {
                Ok(Value::Null)
            } else {
                Ok(Value::Err {
                    message: format!("unknown tcp stream id {id}"),
                    code: 1,
                    origin: "tcp_close".to_string(),
                    cause: None,
                })
            }
        }
        "http_parse_headers" | "parse_headers" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_headers raw", span)?;
            Ok(Value::Tuple(vec![parse_http_headers(raw), Value::Null]))
        }
        "http_parse_response" | "parse_response" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_response raw", span)?;
            match parse_http_response(raw) {
                Ok((code, reason, headers, body)) => Ok(Value::Tuple(vec![
                    Value::Int128(code as i128),
                    Value::Str(reason),
                    headers,
                    Value::Str(body),
                    Value::Null,
                ])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Str(String::new()),
                    Value::Map(vec![]),
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "parse_response".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "http_parse_request" | "parse_request" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_request raw", span)?;
            match parse_http_request(raw) {
                Ok((method, path, version, headers, body)) => Ok(Value::Tuple(vec![
                    Value::Str(method),
                    Value::Str(path),
                    Value::Str(version),
                    headers,
                    Value::Str(body),
                    Value::Null,
                ])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    Value::Str(String::new()),
                    Value::Map(vec![]),
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "parse_request".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "http_get" | "get" => {
            let [url] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let url = expect_str_arg(url, "get url", span)?;
            match http_get_simple(url) {
                Ok(raw_response) => match parse_http_response(&raw_response) {
                    Ok((code, reason, headers, body)) => Ok(Value::Tuple(vec![
                        Value::Int128(code as i128),
                        Value::Str(reason),
                        headers,
                        Value::Str(body),
                        Value::Null,
                    ])),
                    Err(msg) => Ok(Value::Tuple(vec![
                        Value::Int128(0),
                        Value::Str(String::new()),
                        Value::Map(vec![]),
                        Value::Str(String::new()),
                        Value::Err {
                            message: msg,
                            code: 1,
                            origin: "get".to_string(),
                            cause: None,
                        },
                    ])),
                },
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Str(String::new()),
                    Value::Map(vec![]),
                    Value::Str(String::new()),
                    io_err("get", e),
                ])),
            }
        }
        "http_listen" | "listen" => {
            let [addr] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let addr = expect_str_arg(addr, "listen addr", span)?;
            match TcpListener::bind(addr) {
                Ok(listener) => {
                    let id = next_http_listener_id();
                    let mut table = http_listener_table().lock().map_err(|_| {
                        Diagnostic::new("failed to lock http listener table", span, Severity::Error)
                    })?;
                    table.insert(id, listener);
                    Ok(Value::Tuple(vec![Value::UInt128(id as u128), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![Value::UInt128(0), io_err("listen", e)])),
            }
        }
        "http_local_addr" | "local_addr" => {
            let [id] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "local_addr id", span)?;
            let table = http_listener_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock http listener table", span, Severity::Error)
            })?;
            let Some(listener) = table.get(&id) else {
                return Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: format!("unknown http listener id {id}"),
                        code: 1,
                        origin: "local_addr".to_string(),
                        cause: None,
                    },
                ]));
            };
            match listener.local_addr() {
                Ok(addr) => Ok(Value::Tuple(vec![
                    Value::Str(addr.to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("local_addr", e),
                ])),
            }
        }
        "http_respond_once" | "respond_once" => {
            let [id, response] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let id = expect_u64_arg(id, "respond_once id", span)?;
            let response = expect_str_arg(response, "respond_once response", span)?;
            let table = http_listener_table().lock().map_err(|_| {
                Diagnostic::new("failed to lock http listener table", span, Severity::Error)
            })?;
            let Some(listener) = table.get(&id) else {
                return Ok(Value::Err {
                    message: format!("unknown http listener id {id}"),
                    code: 1,
                    origin: "respond_once".to_string(),
                    cause: None,
                });
            };
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
                    let mut sink = [0_u8; 1024];
                    let _ = stream.read(&mut sink);
                    match stream.write_all(response.as_bytes()) {
                        Ok(_) => Ok(Value::Null),
                        Err(e) => Ok(io_err("respond_once", e)),
                    }
                }
                Err(e) => Ok(io_err("respond_once", e)),
            }
        }
        "crypto_sha256" | "sha256" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "sha256 input", span)?;
            Ok(Value::Str(sha256_hex(input.as_bytes())))
        }
        "crypto_random_bytes" | "random_bytes" => {
            let [len] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let len = expect_u64_arg(len, "random_bytes len", span)? as usize;
            let mut out = vec![0_u8; len];
            match secure_random_fill(&mut out) {
                Ok(()) => Ok(Value::Tuple(vec![
                    Value::Str(hex_encode(&out)),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("random_bytes", e),
                ])),
            }
        }
        "crypto_random_u64" | "random_u64" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let mut out = [0_u8; 8];
            match secure_random_fill(&mut out) {
                Ok(()) => Ok(Value::Tuple(vec![
                    Value::UInt128(u64::from_le_bytes(out) as u128),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    io_err("random_u64", e),
                ])),
            }
        }
        "crypto_encrypt" | "encrypt" => {
            let [plain, key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let plain = expect_str_arg(plain, "encrypt plaintext", span)?;
            let key = expect_str_arg(key, "encrypt key", span)?;
            match encrypt_with_key(plain.as_bytes(), key.as_bytes()) {
                Ok(blob) => Ok(Value::Tuple(vec![Value::Str(blob), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    io_err("encrypt", e),
                ])),
            }
        }
        "crypto_decrypt" | "decrypt" => {
            let [blob, key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let blob = expect_str_arg(blob, "decrypt blob", span)?;
            let key = expect_str_arg(key, "decrypt key", span)?;
            match decrypt_with_key(blob, key.as_bytes()) {
                Ok(plain) => Ok(Value::Tuple(vec![Value::Str(plain), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e,
                        code: 1,
                        origin: "decrypt".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "rand_seed" | "seed" => {
            let [seed] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let seed = expect_u64_arg(seed, "seed value", span)?;
            rand_set_seed(seed);
            Ok(Value::Null)
        }
        "rand_next_u64" | "next_u64" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::UInt128(math_next_u64() as u128))
        }
        "rand_next_f64" | "next_f64" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Float((math_next_u64() as f64) / (u64::MAX as f64)))
        }
        "rand_range_i32" | "range_i32" => {
            let [min, max] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let min = expect_i32_like(min, "range_i32 min", span)?;
            let max = expect_i32_like(max, "range_i32 max", span)?;
            if min > max {
                return Ok(Value::Tuple(vec![
                    Value::Int128(0),
                    Value::Err {
                        message: "min must be <= max".to_string(),
                        code: 1,
                        origin: "range_i32".to_string(),
                        cause: None,
                    },
                ]));
            }
            let width = (max as i64 - min as i64 + 1) as u64;
            let v = min as i64 + (math_next_u64() % width) as i64;
            Ok(Value::Tuple(vec![Value::Int128(v as i128), Value::Null]))
        }
        "rand_range_u64" | "range_u64" => {
            let [min, max] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let min = expect_u64_arg(min, "range_u64 min", span)?;
            let max = expect_u64_arg(max, "range_u64 max", span)?;
            if min > max {
                return Ok(Value::Tuple(vec![
                    Value::UInt128(0),
                    Value::Err {
                        message: "min must be <= max".to_string(),
                        code: 1,
                        origin: "range_u64".to_string(),
                        cause: None,
                    },
                ]));
            }
            let width = max.saturating_sub(min).saturating_add(1);
            let v = min.saturating_add(math_next_u64() % width);
            Ok(Value::Tuple(vec![Value::UInt128(v as u128), Value::Null]))
        }
        "rand_bytes_hex" | "bytes_hex" => {
            let [len] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let len = expect_u64_arg(len, "bytes_hex len", span)? as usize;
            let mut out = vec![0_u8; len];
            for b in &mut out {
                *b = (math_next_u64() & 0xff) as u8;
            }
            Ok(Value::Tuple(vec![
                Value::Str(hex_encode(&out)),
                Value::Null,
            ]))
        }
        "encoding_base64_encode" | "base64_encode" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "base64_encode input", span)?;
            Ok(Value::Str(BASE64_STD.encode(input.as_bytes())))
        }
        "encoding_base64_decode" | "base64_decode" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "base64_decode input", span)?;
            match BASE64_STD.decode(input.as_bytes()) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => Ok(Value::Tuple(vec![Value::Str(s), Value::Null])),
                    Err(_) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err {
                            message: "decoded bytes are not valid utf-8".to_string(),
                            code: 1,
                            origin: "base64_decode".to_string(),
                            cause: None,
                        },
                    ])),
                },
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "base64_decode".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "encoding_hex_encode" | "hex_encode" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "hex_encode input", span)?;
            Ok(Value::Str(hex_encode(input.as_bytes())))
        }
        "encoding_hex_decode" | "hex_decode" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "hex_decode input", span)?;
            match hex_decode(input) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => Ok(Value::Tuple(vec![Value::Str(s), Value::Null])),
                    Err(_) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err {
                            message: "decoded bytes are not valid utf-8".to_string(),
                            code: 1,
                            origin: "hex_decode".to_string(),
                            cause: None,
                        },
                    ])),
                },
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e,
                        code: 1,
                        origin: "hex_decode".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "encoding_is_ascii" | "is_ascii" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "is_ascii input", span)?;
            Ok(Value::Bool(input.is_ascii()))
        }
        "encoding_ascii_upper" | "ascii_upper" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "ascii_upper input", span)?;
            Ok(Value::Str(input.to_ascii_uppercase()))
        }
        "encoding_ascii_lower" | "ascii_lower" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "ascii_lower input", span)?;
            Ok(Value::Str(input.to_ascii_lowercase()))
        }
        "encoding_utf8_len" | "utf8_len" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "utf8_len input", span)?;
            Ok(Value::UInt128(input.chars().count() as u128))
        }
        "encoding_utf8_is_valid" | "utf8_is_valid" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "utf8_is_valid input", span)?;
            Ok(Value::Bool(std::str::from_utf8(input.as_bytes()).is_ok()))
        }
        "regex_is_match" | "is_match" => {
            let [pattern, input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pattern = expect_str_arg(pattern, "is_match pattern", span)?;
            let input = expect_str_arg(input, "is_match input", span)?;
            match Regex::new(pattern) {
                Ok(re) => Ok(Value::Tuple(vec![
                    Value::Bool(re.is_match(input)),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Bool(false),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "is_match".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "regex_find" | "find" => {
            let [pattern, input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pattern = expect_str_arg(pattern, "find pattern", span)?;
            let input = expect_str_arg(input, "find input", span)?;
            match Regex::new(pattern) {
                Ok(re) => {
                    let out = re
                        .find(input)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    Ok(Value::Tuple(vec![Value::Str(out), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "find".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "regex_find_all" | "find_all" => {
            let [pattern, input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pattern = expect_str_arg(pattern, "find_all pattern", span)?;
            let input = expect_str_arg(input, "find_all input", span)?;
            match Regex::new(pattern) {
                Ok(re) => {
                    let out = re
                        .find_iter(input)
                        .map(|m| Value::Str(m.as_str().to_string()))
                        .collect::<Vec<_>>();
                    Ok(Value::Tuple(vec![Value::Array(out), Value::Null]))
                }
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Array(vec![]),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "find_all".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "regex_replace" | "replace_regex" => {
            let [pattern, input, replacement] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pattern = expect_str_arg(pattern, "replace_regex pattern", span)?;
            let input = expect_str_arg(input, "replace_regex input", span)?;
            let replacement = expect_str_arg(replacement, "replace_regex replacement", span)?;
            match Regex::new(pattern) {
                Ok(re) => Ok(Value::Tuple(vec![
                    Value::Str(re.replace(input, replacement).to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "replace_regex".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "regex_replace_all" | "replace_all_regex" => {
            let [pattern, input, replacement] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let pattern = expect_str_arg(pattern, "replace_all_regex pattern", span)?;
            let input = expect_str_arg(input, "replace_all_regex input", span)?;
            let replacement = expect_str_arg(replacement, "replace_all_regex replacement", span)?;
            match Regex::new(pattern) {
                Ok(re) => Ok(Value::Tuple(vec![
                    Value::Str(re.replace_all(input, replacement).to_string()),
                    Value::Null,
                ])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "replace_all_regex".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "cli_args" | "args_cli" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Array(
                cli_script_args()
                    .into_iter()
                    .map(Value::Str)
                    .collect::<Vec<_>>(),
            ))
        }
        "cli_arg_count" | "arg_count" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::UInt128(cli_script_args().len() as u128))
        }
        "cli_positional" | "positional" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Array(
                cli_positional_args()
                    .into_iter()
                    .map(Value::Str)
                    .collect::<Vec<_>>(),
            ))
        }
        "cli_command" | "command" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let positional = cli_positional_args();
            if let Some(cmd) = positional.first() {
                Ok(Value::Tuple(vec![Value::Str(cmd.clone()), Value::Null]))
            } else {
                Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "no command provided".to_string(),
                        code: 1,
                        origin: "command".to_string(),
                        cause: None,
                    },
                ]))
            }
        }
        "cli_has_flag" | "has_flag" => {
            let [flag] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let flag = expect_str_arg(flag, "has_flag flag", span)?;
            Ok(Value::Bool(cli_has_flag(flag)))
        }
        "cli_flag_value" | "flag_value" => {
            let [flag] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let flag = expect_str_arg(flag, "flag_value flag", span)?;
            match cli_flag_value(flag) {
                Some(v) => Ok(Value::Tuple(vec![Value::Str(v), Value::Null])),
                None => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: format!("flag '{flag}' not found"),
                        code: 1,
                        origin: "flag_value".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "cli_help_requested" | "help_requested" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Bool(cli_has_flag("help") || cli_has_flag("h")))
        }
        "cli_version_requested" | "version_requested" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Bool(cli_has_flag("version") || cli_has_flag("v")))
        }
        "env_get" | "get_env" => {
            let [key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "get_env key", span)?;
            match std::env::var(key) {
                Ok(v) => Ok(Value::Tuple(vec![Value::Str(v), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "get_env".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "env_get_or" | "get_env_or" => {
            let [key, default] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "get_env_or key", span)?;
            let default = expect_str_arg(default, "get_env_or default", span)?;
            Ok(Value::Str(
                std::env::var(key).unwrap_or_else(|_| default.to_string()),
            ))
        }
        "env_set" | "set_env" => {
            let [key, value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "set_env key", span)?;
            let value = expect_str_arg(value, "set_env value", span)?;
            // SAFETY: process-level env mutation is intended std/env behavior.
            unsafe { std::env::set_var(key, value) };
            Ok(Value::Null)
        }
        "env_unset" | "unset_env" => {
            let [key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "unset_env key", span)?;
            // SAFETY: process-level env mutation is intended std/env behavior.
            unsafe { std::env::remove_var(key) };
            Ok(Value::Null)
        }
        "env_has" | "has_env" => {
            let [key] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let key = expect_str_arg(key, "has_env key", span)?;
            Ok(Value::Bool(std::env::var(key).is_ok()))
        }
        "env_list" | "list_env" => {
            let [] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects 0 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let entries = std::env::vars()
                .map(|(k, v)| (Value::Str(k), Value::Str(v)))
                .collect::<Vec<_>>();
            Ok(Value::Map(entries))
        }
        "env_list_prefix" | "list_env_prefix" => {
            let [prefix] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let prefix = expect_str_arg(prefix, "list_env_prefix prefix", span)?;
            let entries = std::env::vars()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (Value::Str(k), Value::Str(v)))
                .collect::<Vec<_>>();
            Ok(Value::Map(entries))
        }
        "log_set_level" | "set_log_level" => {
            let [level] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let level = expect_str_arg(level, "set_log_level level", span)?;
            match parse_log_level(level) {
                Some(lv) => {
                    log_state_set_level(lv);
                    Ok(Value::Null)
                }
                None => Ok(Value::Err {
                    message: format!("invalid log level '{level}'"),
                    code: 1,
                    origin: "set_log_level".to_string(),
                    cause: None,
                }),
            }
        }
        "log_set_prefix" | "set_log_prefix" => {
            let [prefix] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let prefix = expect_str_arg(prefix, "set_log_prefix prefix", span)?;
            log_state_set_prefix(prefix.to_string());
            Ok(Value::Null)
        }
        "log_set_json" | "set_log_json" => {
            let [json] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let Value::Bool(json) = json else {
                return Err(Diagnostic::new(
                    "set_log_json expects bool argument",
                    span,
                    Severity::Error,
                ));
            };
            log_state_set_json(*json);
            Ok(Value::Null)
        }
        "log_write" | "write_log" => {
            let [level, message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let level = expect_str_arg(level, "log level", span)?;
            let message = expect_str_arg(message, "log message", span)?;
            match parse_log_level(level) {
                Some(lv) => {
                    log_emit(lv, message);
                    Ok(Value::Null)
                }
                None => Ok(Value::Err {
                    message: format!("invalid log level '{level}'"),
                    code: 1,
                    origin: "log".to_string(),
                    cause: None,
                }),
            }
        }
        "log_trace" | "trace" => {
            let [message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            log_emit(
                LogLevel::Trace,
                expect_str_arg(message, "trace message", span)?,
            );
            Ok(Value::Null)
        }
        "log_debug" | "debug" => {
            let [message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            log_emit(
                LogLevel::Debug,
                expect_str_arg(message, "debug message", span)?,
            );
            Ok(Value::Null)
        }
        "log_info" | "info" => {
            let [message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            log_emit(
                LogLevel::Info,
                expect_str_arg(message, "info message", span)?,
            );
            Ok(Value::Null)
        }
        "log_warn" | "warn" => {
            let [message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            log_emit(
                LogLevel::Warn,
                expect_str_arg(message, "warn message", span)?,
            );
            Ok(Value::Null)
        }
        "log_error" | "error_msg" => {
            let [message] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            log_emit(
                LogLevel::Error,
                expect_str_arg(message, "error message", span)?,
            );
            Ok(Value::Null)
        }
        "json_parse" | "parse_json" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_json raw", span)?;
            match serde_json::from_str::<JsonValue>(raw) {
                Ok(json) => Ok(Value::Tuple(vec![json_to_value(&json), Value::Null])),
                Err(e) => Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: e.to_string(),
                        code: 1,
                        origin: "parse_json".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "json_stringify" | "stringify_json" => {
            let [value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match value_to_json(value) {
                Ok(json) => match serde_json::to_string(&json) {
                    Ok(out) => Ok(Value::Tuple(vec![Value::Str(out), Value::Null])),
                    Err(e) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err {
                            message: e.to_string(),
                            code: 1,
                            origin: "stringify_json".to_string(),
                            cause: None,
                        },
                    ])),
                },
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "stringify_json".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "json_stringify_pretty" | "stringify_json_pretty" => {
            let [value] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match value_to_json(value) {
                Ok(json) => match serde_json::to_string_pretty(&json) {
                    Ok(out) => Ok(Value::Tuple(vec![Value::Str(out), Value::Null])),
                    Err(e) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err {
                            message: e.to_string(),
                            code: 1,
                            origin: "stringify_json_pretty".to_string(),
                            cause: None,
                        },
                    ])),
                },
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "stringify_json_pretty".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "csv_parse" | "parse_csv" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_csv raw", span)?;
            match parse_csv_basic(raw) {
                Ok(rows) => Ok(Value::Tuple(vec![rows, Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "parse_csv".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "csv_stringify" | "stringify_csv" => {
            let [rows] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match stringify_csv_basic(rows) {
                Ok(out) => Ok(Value::Tuple(vec![Value::Str(out), Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "stringify_csv".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "mime_guess" | "guess_mime" => {
            let [path] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let path = expect_str_arg(path, "guess_mime path", span)?;
            Ok(Value::Str(mime_guess_from_path(path)))
        }
        "mime_from_ext" | "from_extension" => {
            let [ext] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let ext = expect_str_arg(ext, "from_extension ext", span)?;
            Ok(Value::Str(mime_from_extension(ext)))
        }
        "mime_is_text" | "is_text_mime" => {
            let [mime] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let mime = expect_str_arg(mime, "is_text_mime mime", span)?;
            Ok(Value::Bool(mime_is_textual(mime)))
        }
        "url_parse" | "parse_url" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_url raw", span)?;
            match parse_url_basic(raw) {
                Ok(parts) => Ok(Value::Tuple(vec![parts, Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Map(vec![]),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "parse_url".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "url_build" | "build_url" => {
            let [scheme, host, path, query, fragment] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 5 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let scheme = expect_str_arg(scheme, "build_url scheme", span)?;
            let host = expect_str_arg(host, "build_url host", span)?;
            let path = expect_str_arg(path, "build_url path", span)?;
            let query = expect_str_arg(query, "build_url query", span)?;
            let fragment = expect_str_arg(fragment, "build_url fragment", span)?;
            match build_url_basic(scheme, host, path, query, fragment) {
                Ok(url) => Ok(Value::Tuple(vec![Value::Str(url), Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "build_url".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "url_encode_component" | "encode_url_component" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "encode_url_component input", span)?;
            Ok(Value::Str(url_encode_component_basic(input)))
        }
        "url_decode_component" | "decode_url_component" => {
            let [input] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let input = expect_str_arg(input, "decode_url_component input", span)?;
            match url_decode_component_basic(input) {
                Ok(out) => Ok(Value::Tuple(vec![Value::Str(out), Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "decode_url_component".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "xml_parse" | "parse_xml" => {
            let [raw] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let raw = expect_str_arg(raw, "parse_xml raw", span)?;
            match parse_xml_basic(raw) {
                Ok(v) => Ok(Value::Tuple(vec![v, Value::Null])),
                Err(msg) => Ok(Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: msg,
                        code: 1,
                        origin: "parse_xml".to_string(),
                        cause: None,
                    },
                ])),
            }
        }
        "xml_stringify" | "stringify_xml" => {
            let [name, text] = args else {
                return Err(Diagnostic::new(
                    format!("{name} expects exactly 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            let name = expect_str_arg(name, "stringify_xml name", span)?;
            let text = expect_str_arg(text, "stringify_xml text", span)?;
            if name.is_empty() {
                return Ok(Value::Tuple(vec![
                    Value::Str(String::new()),
                    Value::Err {
                        message: "xml tag name cannot be empty".to_string(),
                        code: 1,
                        origin: "stringify_xml".to_string(),
                        cause: None,
                    },
                ]));
            }
            let escaped = xml_escape_text(text);
            let out = format!("<{name}>{escaped}</{name}>");
            Ok(Value::Tuple(vec![Value::Str(out), Value::Null]))
        }
        name if name.starts_with("__meth_is_") || name.starts_with("__meth_iu_") => {
            dispatch_integer_method(name, args, span)
        }
        name if name.starts_with("__meth_f_") => dispatch_float_method(name, args, span),
        name if name.starts_with("__meth_b_") => dispatch_bool_method(name, args, span),
        name if name.starts_with("__meth_c_") => dispatch_char_method(name, args, span),
        name if name.starts_with("__meth_s_") => dispatch_str_method(name, args, span),
        name if name.starts_with("__meth_a_") => dispatch_array_method(name, args, span),
        name if name.starts_with("__meth_fn_") => dispatch_function_method(vm, name, args, span),
        name if name.starts_with("__meth_m_") => dispatch_map_method(vm, name, args, span),
        name if name.starts_with("__meth_set_") => dispatch_set_method(name, args, span),
        _ => Err(Diagnostic::new(
            format!("unknown builtin '{name}'"),
            span,
            Severity::Error,
        )),
    }
}

fn dispatch_integer_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "integer method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let method = name
        .strip_prefix("__meth_is_")
        .or_else(|| name.strip_prefix("__meth_iu_"))
        .unwrap_or(name);
    let Some(recv_int) = value_to_typed_int(recv) else {
        return Err(Diagnostic::new(
            "integer method receiver must be integer",
            span,
            Severity::Error,
        ));
    };

    let arg_int = |idx: usize| -> Result<super::int::TypedInt, Diagnostic> {
        match tail.get(idx) {
            Some(v) => {
                let raw = value_to_typed_int(v).ok_or_else(|| {
                    Diagnostic::new("expected integer argument", span, Severity::Error)
                })?;
                cast_typed_int_to_tag(raw, recv_int.tag())
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))
            }
            _ => Err(Diagnostic::new(
                "expected integer argument",
                span,
                Severity::Error,
            )),
        }
    };

    let typed_zero = || -> Value {
        match recv_int.payload() {
            super::int::IntPayload::Signed(_) => Value::Int(
                super::int::TypedInt::try_from_signed(recv_int.tag(), 0).expect("zero signed"),
            ),
            super::int::IntPayload::Unsigned(_) => Value::Int(
                super::int::TypedInt::try_from_unsigned(recv_int.tag(), 0).expect("zero unsigned"),
            ),
        }
    };

    let checked = |ok: Option<Value>, label: &str| -> Value {
        if let Some(v) = ok {
            Value::Tuple(vec![v, Value::Null])
        } else {
            Value::Tuple(vec![
                typed_zero(),
                Value::Err {
                    message: format!("{label} overflow"),
                    code: 1,
                    origin: "checked".to_string(),
                    cause: None,
                },
            ])
        }
    };

    let out =
        match method {
            "add" => Value::Int(
                recv_int
                    .checked_add(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "sub" => Value::Int(
                recv_int
                    .checked_sub(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "mul" => Value::Int(
                recv_int
                    .checked_mul(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "div" => Value::Int(
                recv_int
                    .checked_div(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "mod" => Value::Int(
                recv_int
                    .checked_mod(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "neg" => Value::Int(
                recv_int
                    .checked_neg()
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "abs" => match recv_int.payload() {
                super::int::IntPayload::Signed(v) => Value::Int(
                    super::int::TypedInt::try_from_signed(recv_int.tag(), v.abs())
                        .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                ),
                super::int::IntPayload::Unsigned(_) => Value::Int(recv_int),
            },
            "and" => Value::Int(
                recv_int
                    .checked_bitwise_and(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "or" => Value::Int(
                recv_int
                    .checked_bitwise_or(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "xor" => Value::Int(
                recv_int
                    .checked_bitwise_xor(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "not" => Value::Int(
                recv_int
                    .bit_not()
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "shl" => Value::Int(
                recv_int
                    .checked_shift(arg_int(0)?, true)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "shr" => Value::Int(
                recv_int
                    .checked_shift(arg_int(0)?, false)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "rotl" => Value::Int(
                int_rotate(recv_int, arg_int(0)?, true)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "rotr" => Value::Int(
                int_rotate(recv_int, arg_int(0)?, false)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "eq" => Value::Bool(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    == std::cmp::Ordering::Equal,
            ),
            "ne" => Value::Bool(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    != std::cmp::Ordering::Equal,
            ),
            "lt" => Value::Bool(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    == std::cmp::Ordering::Less,
            ),
            "le" => Value::Bool(matches!(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            )),
            "gt" => Value::Bool(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    == std::cmp::Ordering::Greater,
            ),
            "ge" => Value::Bool(matches!(
                recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            )),
            "cmp" => {
                let ord = recv_int
                    .cmp_same_type(arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                let v = match ord {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                Value::int_i128(v)
            }
            "min" => {
                let rhs = arg_int(0)?;
                let ord = recv_int
                    .cmp_same_type(rhs)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                Value::Int(if ord == std::cmp::Ordering::Greater {
                    rhs
                } else {
                    recv_int
                })
            }
            "max" => {
                let rhs = arg_int(0)?;
                let ord = recv_int
                    .cmp_same_type(rhs)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?;
                Value::Int(if ord == std::cmp::Ordering::Less {
                    rhs
                } else {
                    recv_int
                })
            }
            "clamp" => {
                let lo = arg_int(0)?;
                let hi = arg_int(1)?;
                let below = recv_int
                    .cmp_same_type(lo)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    == std::cmp::Ordering::Less;
                let above = recv_int
                    .cmp_same_type(hi)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?
                    == std::cmp::Ordering::Greater;
                Value::Int(if below {
                    lo
                } else if above {
                    hi
                } else {
                    recv_int
                })
            }
            "is_zero" => Value::Bool(match recv_int.payload() {
                super::int::IntPayload::Signed(v) => v == 0,
                super::int::IntPayload::Unsigned(v) => v == 0,
            }),
            "is_even" => Value::Bool(match recv_int.payload() {
                super::int::IntPayload::Signed(v) => v % 2 == 0,
                super::int::IntPayload::Unsigned(v) => v % 2 == 0,
            }),
            "is_odd" => Value::Bool(match recv_int.payload() {
                super::int::IntPayload::Signed(v) => v % 2 != 0,
                super::int::IntPayload::Unsigned(v) => v % 2 != 0,
            }),
            "checked_add" => checked(
                recv_int.checked_add(arg_int(0)?).ok().map(Value::Int),
                "checked_add",
            ),
            "checked_sub" => checked(
                recv_int.checked_sub(arg_int(0)?).ok().map(Value::Int),
                "checked_sub",
            ),
            "checked_mul" => checked(
                recv_int.checked_mul(arg_int(0)?).ok().map(Value::Int),
                "checked_mul",
            ),
            "checked_div" => checked(
                recv_int.checked_div(arg_int(0)?).ok().map(Value::Int),
                "checked_div",
            ),
            "wrapping_add" => Value::Int(
                int_wrapping_add(recv_int, arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "wrapping_sub" => Value::Int(
                int_wrapping_sub(recv_int, arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "saturating_add" => Value::Int(
                int_saturating_add(recv_int, arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "saturating_sub" => Value::Int(
                int_saturating_sub(recv_int, arg_int(0)?)
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            ),
            "to_i32" => {
                let raw = recv
                    .as_i128()
                    .ok_or_else(|| Diagnostic::new("to_i32 overflow", span, Severity::Error))?;
                Value::Int(
                    super::int::TypedInt::try_from_signed(
                        super::int::IntTag::I32,
                        i128::from(i32::try_from(raw).map_err(|_| {
                            Diagnostic::new("to_i32 overflow", span, Severity::Error)
                        })?),
                    )
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                )
            }
            "to_u32" => {
                let raw = recv
                    .as_u128()
                    .ok_or_else(|| Diagnostic::new("to_u32 overflow", span, Severity::Error))?;
                Value::Int(
                    super::int::TypedInt::try_from_unsigned(
                        super::int::IntTag::U32,
                        u128::from(u32::try_from(raw).map_err(|_| {
                            Diagnostic::new("to_u32 overflow", span, Severity::Error)
                        })?),
                    )
                    .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
                )
            }
            "to_f32" => Value::Float(match recv_int.payload() {
                super::int::IntPayload::Signed(v) => v as f64,
                super::int::IntPayload::Unsigned(v) => v as f64,
            }),
            "to_bool" => Value::Bool(match recv_int.payload() {
                super::int::IntPayload::Signed(v) => v != 0,
                super::int::IntPayload::Unsigned(v) => v != 0,
            }),
            "to_str" => Value::Str(recv.display_for_print()),
            _ => {
                return Err(Diagnostic::new(
                    format!("unknown integer method '{method}'"),
                    span,
                    Severity::Error,
                ));
            }
        };
    Ok(out)
}

fn dispatch_float_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "float method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Float(r) = recv else {
        return Err(Diagnostic::new(
            "float method receiver must be float",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_f_").unwrap_or(name);
    let af = |idx: usize| -> Result<f64, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Float(v)) => Ok(*v),
            _ => Err(Diagnostic::new(
                "expected float argument",
                span,
                Severity::Error,
            )),
        }
    };
    let out = match method {
        "add" => Value::Float(*r + af(0)?),
        "sub" => Value::Float(*r - af(0)?),
        "mul" => Value::Float(*r * af(0)?),
        "div" => Value::Float(*r / af(0)?),
        "mod" => Value::Float(*r % af(0)?),
        "neg" => Value::Float(-*r),
        "eq" => Value::Bool(*r == af(0)?),
        "lt" => Value::Bool(*r < af(0)?),
        "gt" => Value::Bool(*r > af(0)?),
        "cmp" => {
            let rhs = af(0)?;
            let cmp = if *r < rhs {
                -1
            } else if *r > rhs {
                1
            } else {
                0
            };
            Value::int_i128(cmp)
        }
        "abs" => Value::Float(r.abs()),
        "sqrt" => Value::Float(r.sqrt()),
        "pow" => Value::Float(r.powf(af(0)?)),
        "exp" => Value::Float(r.exp()),
        "log" => Value::Float(r.ln()),
        "log10" => Value::Float(r.log10()),
        "floor" => Value::Float(r.floor()),
        "ceil" => Value::Float(r.ceil()),
        "round" => Value::Float(r.round()),
        "trunc" => Value::Float(r.trunc()),
        "fract" => Value::Float(r.fract()),
        "sin" => Value::Float(r.sin()),
        "cos" => Value::Float(r.cos()),
        "tan" => Value::Float(r.tan()),
        "asin" => Value::Float(r.asin()),
        "acos" => Value::Float(r.acos()),
        "atan" => Value::Float(r.atan()),
        "is_nan" => Value::Bool(r.is_nan()),
        "is_inf" => Value::Bool(r.is_infinite()),
        "is_finite" => Value::Bool(r.is_finite()),
        "to_i32" => {
            if !r.is_finite() {
                return Err(Diagnostic::new(
                    "to_i32 requires finite float",
                    span,
                    Severity::Error,
                ));
            }
            if *r < i32::MIN as f64 || *r > i32::MAX as f64 {
                return Err(Diagnostic::new("to_i32 overflow", span, Severity::Error));
            }
            Value::Int(
                super::int::TypedInt::try_from_signed(
                    super::int::IntTag::I32,
                    i128::from(*r as i32),
                )
                .map_err(|msg| Diagnostic::new(msg, span, Severity::Error))?,
            )
        }
        "to_str" => Value::Str(recv.display_for_print()),
        _ => {
            return Err(Diagnostic::new(
                format!("unknown float method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_bool_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "bool method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Bool(r) = recv else {
        return Err(Diagnostic::new(
            "bool method receiver must be bool",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_b_").unwrap_or(name);
    let ab = |idx: usize| -> Result<bool, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Bool(v)) => Ok(*v),
            _ => Err(Diagnostic::new(
                "expected bool argument",
                span,
                Severity::Error,
            )),
        }
    };
    let out = match method {
        "and" => Value::Bool(*r && ab(0)?),
        "or" => Value::Bool(*r || ab(0)?),
        "xor" => Value::Bool(*r ^ ab(0)?),
        "not" => Value::Bool(!*r),
        "eq" => Value::Bool(*r == ab(0)?),
        "to_i32" => Value::Int128(if *r { 1 } else { 0 }),
        "to_str" => Value::Str(recv.display_for_print()),
        _ => {
            return Err(Diagnostic::new(
                format!("unknown bool method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_char_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "char method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Char(r) = recv else {
        return Err(Diagnostic::new(
            "char method receiver must be char",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_c_").unwrap_or(name);
    let ac = |idx: usize| -> Result<char, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Char(v)) => Ok(*v),
            _ => Err(Diagnostic::new(
                "expected char argument",
                span,
                Severity::Error,
            )),
        }
    };
    let out = match method {
        "eq" => Value::Bool(*r == ac(0)?),
        "is_digit" => Value::Bool(r.is_ascii_digit()),
        "is_alpha" => Value::Bool(r.is_alphabetic()),
        "is_alnum" => Value::Bool(r.is_alphanumeric()),
        "is_whitespace" => Value::Bool(r.is_whitespace()),
        "to_upper" => Value::Char(r.to_ascii_uppercase()),
        "to_lower" => Value::Char(r.to_ascii_lowercase()),
        "to_i32" => Value::Int128((*r as u32) as i128),
        "to_str" => Value::Str(recv.display_for_print()),
        _ => {
            return Err(Diagnostic::new(
                format!("unknown char method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_str_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "str method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Str(s) = recv else {
        return Err(Diagnostic::new(
            "str method receiver must be str",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_s_").unwrap_or(name);
    let astr = |idx: usize| -> Result<&str, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Str(v)) => Ok(v.as_str()),
            _ => Err(Diagnostic::new(
                "expected str argument",
                span,
                Severity::Error,
            )),
        }
    };
    let aidx = |idx: usize| -> Result<usize, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Int(v)) => match v.payload() {
                super::int::IntPayload::Unsigned(u) => usize::try_from(u).map_err(|_| {
                    Diagnostic::new("index does not fit usize", span, Severity::Error)
                }),
                super::int::IntPayload::Signed(i) if i >= 0 => usize::try_from(i).map_err(|_| {
                    Diagnostic::new("index does not fit usize", span, Severity::Error)
                }),
                _ => Err(Diagnostic::new(
                    "index must be non-negative",
                    span,
                    Severity::Error,
                )),
            },
            Some(Value::UInt128(v)) => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index does not fit usize", span, Severity::Error)),
            Some(Value::Int128(v)) if *v >= 0 => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index does not fit usize", span, Severity::Error)),
            _ => Err(Diagnostic::new(
                "expected u64-like index argument",
                span,
                Severity::Error,
            )),
        }
    };

    let out = match method {
        "len" => Value::UInt128(s.chars().count() as u128),
        "is_empty" => Value::Bool(s.is_empty()),
        "char_at" => {
            let i = aidx(0)?;
            let ch = s.chars().nth(i);
            match ch {
                Some(c) => Value::Tuple(vec![Value::Char(c), Value::Null]),
                None => Value::Tuple(vec![
                    Value::Char('\0'),
                    Value::Err {
                        message: format!(
                            "char_at index out of bounds: index={}, len={}",
                            i,
                            s.chars().count()
                        ),
                        code: 1,
                        origin: "char_at".to_string(),
                        cause: None,
                    },
                ]),
            }
        }
        "contains" => Value::Bool(s.contains(astr(0)?)),
        "starts_with" => Value::Bool(s.starts_with(astr(0)?)),
        "ends_with" => Value::Bool(s.ends_with(astr(0)?)),
        "find" => Value::Int128(s.find(astr(0)?).map(|i| i as i128).unwrap_or(-1)),
        "rfind" => Value::Int128(s.rfind(astr(0)?).map(|i| i as i128).unwrap_or(-1)),
        "slice" => {
            let start = aidx(0)?;
            let end = aidx(1)?;
            let chars: Vec<char> = s.chars().collect();
            if start > end || end > chars.len() {
                return Err(Diagnostic::new(
                    format!(
                        "invalid slice bounds: start={}, end={}, len={}",
                        start,
                        end,
                        chars.len()
                    ),
                    span,
                    Severity::Error,
                ));
            }
            Value::Str(chars[start..end].iter().collect())
        }
        "split" => Value::Array(
            s.split(astr(0)?)
                .map(|x| Value::Str(x.to_string()))
                .collect(),
        ),
        "replace" => Value::Str(s.replace(astr(0)?, astr(1)?)),
        "trim" => Value::Str(s.trim().to_string()),
        "trim_start" => Value::Str(s.trim_start().to_string()),
        "trim_end" => Value::Str(s.trim_end().to_string()),
        "to_upper" => Value::Str(s.to_uppercase()),
        "to_lower" => Value::Str(s.to_lowercase()),
        "reverse" => Value::Str(s.chars().rev().collect()),
        "to_i32" => match s.parse::<i32>() {
            Ok(v) => Value::Tuple(vec![Value::Int128(i128::from(v)), Value::Null]),
            Err(_) => Value::Tuple(vec![
                Value::Int128(0),
                Value::Err {
                    message: format!("cannot parse '{s}' as i32"),
                    code: 1,
                    origin: "to_i32".to_string(),
                    cause: None,
                },
            ]),
        },
        "to_f64" => match s.parse::<f64>() {
            Ok(v) => Value::Tuple(vec![Value::Float(v), Value::Null]),
            Err(_) => Value::Tuple(vec![
                Value::Float(0.0),
                Value::Err {
                    message: format!("cannot parse '{s}' as f64"),
                    code: 1,
                    origin: "to_f64".to_string(),
                    cause: None,
                },
            ]),
        },
        _ => {
            return Err(Diagnostic::new(
                format!("unknown str method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_array_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "array method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Array(items) = recv else {
        return Err(Diagnostic::new(
            "array method receiver must be array",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_a_").unwrap_or(name);
    let aidx = |idx: usize| -> Result<usize, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Int(v)) => match v.payload() {
                super::int::IntPayload::Unsigned(u) => usize::try_from(u)
                    .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
                super::int::IntPayload::Signed(i) if i >= 0 => usize::try_from(i)
                    .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
                _ => Err(Diagnostic::new(
                    "index must be non-negative",
                    span,
                    Severity::Error,
                )),
            },
            Some(Value::UInt128(v)) => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
            Some(Value::Int128(v)) if *v >= 0 => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
            _ => Err(Diagnostic::new(
                "expected u64-like index argument",
                span,
                Severity::Error,
            )),
        }
    };
    let out = match method {
        "len" => Value::UInt128(items.len() as u128),
        "is_empty" => Value::Bool(items.is_empty()),
        "get" => {
            let i = aidx(0)?;
            match items.get(i).cloned() {
                Some(v) => Value::Tuple(vec![v, Value::Null]),
                None => Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: format!(
                            "get index out of bounds: index={}, len={}",
                            i,
                            items.len()
                        ),
                        code: 1,
                        origin: "get".to_string(),
                        cause: None,
                    },
                ]),
            }
        }
        "set" => {
            let i = aidx(0)?;
            let mut arr = items.clone();
            let Some(v) = tail.get(1) else {
                return Err(Diagnostic::new("set expects value", span, Severity::Error));
            };
            if i >= arr.len() {
                return Err(Diagnostic::new(
                    format!("set index out of bounds: index={}, len={}", i, arr.len()),
                    span,
                    Severity::Error,
                ));
            }
            arr[i] = v.clone();
            Value::Unit
        }
        "push" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new("push expects value", span, Severity::Error));
            };
            let mut arr = items.clone();
            arr.push(v.clone());
            Value::Unit
        }
        "pop" => {
            let mut arr = items.clone();
            arr.pop().unwrap_or(Value::Null)
        }
        "insert" => {
            let i = aidx(0)?;
            let Some(v) = tail.get(1) else {
                return Err(Diagnostic::new(
                    "insert expects value",
                    span,
                    Severity::Error,
                ));
            };
            let mut arr = items.clone();
            if i > arr.len() {
                return Err(Diagnostic::new(
                    format!("insert index out of bounds: index={}, len={}", i, arr.len()),
                    span,
                    Severity::Error,
                ));
            }
            arr.insert(i, v.clone());
            Value::Unit
        }
        "remove" => {
            let i = aidx(0)?;
            let mut arr = items.clone();
            if i >= arr.len() {
                Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: format!(
                            "remove index out of bounds: index={}, len={}",
                            i,
                            arr.len()
                        ),
                        code: 1,
                        origin: "remove".to_string(),
                        cause: None,
                    },
                ])
            } else {
                Value::Tuple(vec![arr.remove(i), Value::Null])
            }
        }
        "clear" => Value::Unit,
        "find" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new("find expects value", span, Severity::Error));
            };
            let idx = items
                .iter()
                .position(|x| x == v)
                .map(|i| i as i128)
                .unwrap_or(-1);
            Value::Int128(idx)
        }
        "contains" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new(
                    "contains expects value",
                    span,
                    Severity::Error,
                ));
            };
            Value::Bool(items.iter().any(|x| x == v))
        }
        "reverse" => {
            let mut arr = items.clone();
            arr.reverse();
            Value::Array(arr)
        }
        "sort" => {
            let mut arr = items.clone();
            arr.sort_by_key(|v| v.display_for_print());
            Value::Array(arr)
        }
        "slice" => {
            let start = aidx(0)?;
            let end = aidx(1)?;
            if start > end || end > items.len() {
                return Err(Diagnostic::new(
                    format!(
                        "invalid slice bounds: start={}, end={}, len={}",
                        start,
                        end,
                        items.len()
                    ),
                    span,
                    Severity::Error,
                ));
            }
            Value::Array(items[start..end].to_vec())
        }
        _ => {
            return Err(Diagnostic::new(
                format!("unknown array method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_function_method(
    _vm: &mut Vm<'_>,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "function method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_fn_").unwrap_or(name);
    match method {
        "call" => Err(Diagnostic::new(
            "function.call is not supported yet; use direct call syntax f(...)",
            span,
            Severity::Error,
        )),
        "arity" => match recv {
            Value::Function { function, .. } => Ok(Value::UInt128(function.params.len() as u128)),
            Value::Builtin(_) => Ok(Value::UInt128(0)),
            _ => Err(Diagnostic::new(
                "arity receiver must be function",
                span,
                Severity::Error,
            )),
        },
        "bind" => Ok(recv.clone()),
        "compose" => {
            let Some(Value::Function { .. } | Value::Builtin(_)) = tail.first() else {
                return Err(Diagnostic::new(
                    "compose expects function argument",
                    span,
                    Severity::Error,
                ));
            };
            Ok(recv.clone())
        }
        "partial" => Ok(recv.clone()),
        _ => Err(Diagnostic::new(
            format!("unknown function method '{method}'"),
            span,
            Severity::Error,
        )),
    }
}

fn dispatch_map_method(
    _vm: &mut Vm<'_>,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "map method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Map(entries) = recv else {
        return Err(Diagnostic::new(
            "map method receiver must be map",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_m_").unwrap_or(name);
    let map = entries.clone();
    let find_idx = |m: &[(Value, Value)], key: &Value| m.iter().position(|(k, _)| k == key);
    let out = match method {
        "len" => Value::UInt128(map.len() as u128),
        "is_empty" => Value::Bool(map.is_empty()),
        "get" => tail
            .first()
            .and_then(|k| find_idx(&map, k).map(|i| map[i].1.clone()))
            .unwrap_or(Value::Null),
        "get_or" => {
            let k = tail
                .first()
                .ok_or_else(|| Diagnostic::new("get_or expects key", span, Severity::Error))?;
            let d = tail
                .get(1)
                .ok_or_else(|| Diagnostic::new("get_or expects default", span, Severity::Error))?;
            find_idx(&map, k)
                .map(|i| map[i].1.clone())
                .unwrap_or_else(|| d.clone())
        }
        "get_or_insert" => {
            let k = tail.first().ok_or_else(|| {
                Diagnostic::new("get_or_insert expects key", span, Severity::Error)
            })?;
            let d = tail.get(1).ok_or_else(|| {
                Diagnostic::new("get_or_insert expects default", span, Severity::Error)
            })?;
            if let Some(i) = find_idx(&map, k) {
                map[i].1.clone()
            } else {
                d.clone()
            }
        }
        "contains_key" => {
            let k = tail.first().ok_or_else(|| {
                Diagnostic::new("contains_key expects key", span, Severity::Error)
            })?;
            Value::Bool(find_idx(&map, k).is_some())
        }
        "insert" => {
            let k = tail
                .first()
                .ok_or_else(|| Diagnostic::new("insert expects key", span, Severity::Error))?;
            let _v = tail
                .get(1)
                .ok_or_else(|| Diagnostic::new("insert expects value", span, Severity::Error))?;
            if let Some(i) = find_idx(&map, k) {
                map[i].1.clone()
            } else {
                Value::Null
            }
        }
        "remove" => {
            let k = tail
                .first()
                .ok_or_else(|| Diagnostic::new("remove expects key", span, Severity::Error))?;
            find_idx(&map, k)
                .map(|i| map[i].1.clone())
                .unwrap_or(Value::Null)
        }
        "clear" => Value::Unit,
        "update" => {
            let k = tail
                .first()
                .ok_or_else(|| Diagnostic::new("update expects key", span, Severity::Error))?;
            if let Some(i) = find_idx(&map, k) {
                map[i].1.clone()
            } else {
                Value::Null
            }
        }
        "keys" => Value::Array(map.iter().map(|(k, _)| k.clone()).collect()),
        "values" => Value::Array(map.iter().map(|(_, v)| v.clone()).collect()),
        "entries" => Value::Array(
            map.iter()
                .map(|(k, v)| Value::Tuple(vec![k.clone(), v.clone()]))
                .collect(),
        ),
        "merge" => {
            let other = tail
                .first()
                .ok_or_else(|| Diagnostic::new("merge expects other map", span, Severity::Error))?;
            if let Value::Map(other_entries) = other {
                let mut merged = map.clone();
                for (k, v) in other_entries {
                    if let Some(i) = find_idx(&merged, k) {
                        merged[i].1 = v.clone();
                    } else {
                        merged.push((k.clone(), v.clone()));
                    }
                }
                Value::Map(merged)
            } else {
                return Err(Diagnostic::new(
                    "merge expects map argument",
                    span,
                    Severity::Error,
                ));
            }
        }
        "merge_with" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("merge_with expects other map", span, Severity::Error)
            })?;
            if let Value::Map(other_entries) = other {
                let mut merged = map.clone();
                for (k, v_other) in other_entries {
                    if let Some(i) = find_idx(&merged, k) {
                        merged[i].1 = v_other.clone();
                    } else {
                        merged.push((k.clone(), v_other.clone()));
                    }
                }
                Value::Map(merged)
            } else {
                return Err(Diagnostic::new(
                    "merge_with expects map argument",
                    span,
                    Severity::Error,
                ));
            }
        }
        "clone" => Value::Map(map.clone()),
        "eq" => {
            let other = tail
                .first()
                .ok_or_else(|| Diagnostic::new("eq expects map argument", span, Severity::Error))?;
            Value::Bool(matches!(other, Value::Map(o) if o == &map))
        }
        "ne" => {
            let other = tail
                .first()
                .ok_or_else(|| Diagnostic::new("ne expects map argument", span, Severity::Error))?;
            Value::Bool(!matches!(other, Value::Map(o) if o == &map))
        }
        _ => {
            return Err(Diagnostic::new(
                format!("unknown map method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn dispatch_set_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new(
            "set method missing receiver",
            span,
            Severity::Error,
        ));
    };
    let Value::Set(items) = recv else {
        return Err(Diagnostic::new(
            "set method receiver must be set",
            span,
            Severity::Error,
        ));
    };
    let method = name.strip_prefix("__meth_set_").unwrap_or(name);
    let mut set = items.clone();
    let contains = |s: &[Value], v: &Value| s.iter().any(|x| x == v);
    let out = match method {
        "len" => Value::UInt128(set.len() as u128),
        "is_empty" => Value::Bool(set.is_empty()),
        "contains" => {
            let v = tail
                .first()
                .ok_or_else(|| Diagnostic::new("contains expects value", span, Severity::Error))?;
            Value::Bool(contains(&set, v))
        }
        "insert" => {
            let v = tail
                .first()
                .ok_or_else(|| Diagnostic::new("insert expects value", span, Severity::Error))?;
            let fresh = !contains(&set, v);
            Value::Bool(fresh)
        }
        "remove" => {
            let v = tail
                .first()
                .ok_or_else(|| Diagnostic::new("remove expects value", span, Severity::Error))?;
            Value::Bool(contains(&set, v))
        }
        "clear" => Value::Unit,
        "values" => Value::Array(set.clone()),
        "union" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("union expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "union expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            for v in other_set {
                if !contains(&set, v) {
                    set.push(v.clone());
                }
            }
            Value::Set(set)
        }
        "intersection" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("intersection expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "intersection expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            Value::Set(set.into_iter().filter(|v| contains(other_set, v)).collect())
        }
        "difference" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("difference expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "difference expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            Value::Set(
                set.into_iter()
                    .filter(|v| !contains(other_set, v))
                    .collect(),
            )
        }
        "symmetric_difference" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new(
                    "symmetric_difference expects set argument",
                    span,
                    Severity::Error,
                )
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "symmetric_difference expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            let mut out = Vec::new();
            for v in &set {
                if !contains(other_set, v) {
                    out.push(v.clone());
                }
            }
            for v in other_set {
                if !contains(&set, v) {
                    out.push(v.clone());
                }
            }
            Value::Set(out)
        }
        "is_subset" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("is_subset expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "is_subset expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            Value::Bool(set.iter().all(|v| contains(other_set, v)))
        }
        "is_superset" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("is_superset expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "is_superset expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            Value::Bool(other_set.iter().all(|v| contains(&set, v)))
        }
        "is_disjoint" => {
            let other = tail.first().ok_or_else(|| {
                Diagnostic::new("is_disjoint expects set argument", span, Severity::Error)
            })?;
            let Value::Set(other_set) = other else {
                return Err(Diagnostic::new(
                    "is_disjoint expects set argument",
                    span,
                    Severity::Error,
                ));
            };
            Value::Bool(set.iter().all(|v| !contains(other_set, v)))
        }
        "clone" => Value::Set(set.clone()),
        "eq" => {
            let other = tail
                .first()
                .ok_or_else(|| Diagnostic::new("eq expects set argument", span, Severity::Error))?;
            Value::Bool(matches!(other, Value::Set(o) if o == &set))
        }
        "ne" => {
            let other = tail
                .first()
                .ok_or_else(|| Diagnostic::new("ne expects set argument", span, Severity::Error))?;
            Value::Bool(!matches!(other, Value::Set(o) if o == &set))
        }
        _ => {
            return Err(Diagnostic::new(
                format!("unknown set method '{method}'"),
                span,
                Severity::Error,
            ));
        }
    };
    Ok(out)
}

fn err_value_from_panic(diagnostic: &Diagnostic) -> Value {
    let msg = diagnostic
        .message
        .strip_prefix("panic: ")
        .unwrap_or(&diagnostic.message);
    if let Some((message, code_part)) = msg.rsplit_once(" (code=")
        && let Some(raw_code) = code_part.strip_suffix(')')
        && let Ok(code) = raw_code.parse::<i32>()
    {
        return Value::Err {
            message: message.to_string(),
            code,
            origin: "panic".to_string(),
            cause: None,
        };
    }
    Value::Err {
        message: msg.to_string(),
        code: 1,
        origin: "panic".to_string(),
        cause: None,
    }
}

fn builtin_type_name(v: &Value) -> &'static str {
    match v {
        Value::Int128(_) => "int",
        Value::UInt128(_) => "uint",
        Value::Int(v) => {
            if v.tag().is_signed() {
                "int"
            } else {
                "uint"
            }
        }
        Value::Bool(_) => "bool",
        Value::Str(_) => "str",
        Value::Float(_) => "float",
        Value::Char(_) => "char",
        Value::Unit => "unit",
        Value::Null => "null",
        Value::Builtin(_) => "function",
        Value::Function { .. } => "function",
        Value::Tuple(_) => "tuple",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Set(_) => "set",
        Value::Err { .. } => "err",
        Value::StructInstance { .. } => "struct",
    }
}

fn canonical_type_name(v: &Value) -> String {
    match v {
        Value::Int128(_) => "i128".to_string(),
        Value::UInt128(_) => "u128".to_string(),
        Value::Int(v) => v.type_name().to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Str(_) => "str".to_string(),
        Value::Float(_) => "f64".to_string(),
        Value::Char(_) => "char".to_string(),
        Value::Unit => "unit".to_string(),
        Value::Null => "null".to_string(),
        Value::Builtin(_) | Value::Function { .. } => "fn(any) -> any".to_string(),
        Value::Tuple(items) => {
            let inner = items
                .iter()
                .map(canonical_type_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        Value::Array(items) => {
            if let Some(first) = items.first() {
                format!("[{}]", canonical_type_name(first))
            } else {
                "[unknown]".to_string()
            }
        }
        Value::Map(entries) => {
            if let Some((k, v)) = entries.first() {
                format!("map[{}]{}", canonical_type_name(k), canonical_type_name(v))
            } else {
                "map[unknown]unknown".to_string()
            }
        }
        Value::Set(items) => {
            if let Some(first) = items.first() {
                format!("set[{}]", canonical_type_name(first))
            } else {
                "set[unknown]".to_string()
            }
        }
        Value::Err { .. } => "err".to_string(),
        Value::StructInstance { type_name, .. } => type_name.clone(),
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int128(i) => *i != 0,
        Value::UInt128(u) => *u != 0,
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Signed(i) => i != 0,
            super::int::IntPayload::Unsigned(u) => u != 0,
        },
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Char(c) => *c != '\0',
        Value::Unit => false,
        Value::Null => false,
        Value::Builtin(_) | Value::Function { .. } => true,
        Value::Tuple(items) => !items.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Map(entries) => !entries.is_empty(),
        Value::Set(items) => !items.is_empty(),
        Value::Err { .. } => true,
        Value::StructInstance { .. } => true,
    }
}

fn to_usize_index(value: &Value, span: Span) -> Result<usize, Diagnostic> {
    match value {
        Value::Int(i) => match i.payload() {
            super::int::IntPayload::Signed(v) => usize::try_from(v).map_err(|_| {
                Diagnostic::new(
                    format!(
                        "array index must be non-negative usize-compatible int, got {}",
                        v
                    ),
                    span,
                    Severity::Error,
                )
            }),
            super::int::IntPayload::Unsigned(v) => usize::try_from(v).map_err(|_| {
                Diagnostic::new(
                    format!("array index too large for usize: {}", v),
                    span,
                    Severity::Error,
                )
            }),
        },
        Value::Int128(i) => usize::try_from(*i).map_err(|_| {
            Diagnostic::new(
                format!(
                    "array index must be non-negative usize-compatible int, got {}",
                    i
                ),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(u) => usize::try_from(*u).map_err(|_| {
            Diagnostic::new(
                format!("array index too large for usize: {}", u),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!(
                "array index must be integer, got {}",
                builtin_type_name(other)
            ),
            span,
            Severity::Error,
        )),
    }
}

fn wrap_err_value(
    base_err: Value,
    message: String,
    code_override: Option<i32>,
    origin: String,
    span: Span,
    struct_field_slots: &HashMap<String, HashMap<String, usize>>,
) -> Result<Value, Diagnostic> {
    match base_err {
        Value::Err {
            message: base_message,
            code: base_code,
            origin: base_origin,
            cause: base_cause,
        } => {
            let is_propagation_wrap = message == "propagated by ?";
            let final_message = if is_propagation_wrap {
                base_message.clone()
            } else {
                message
            };
            let final_origin = if is_propagation_wrap {
                "propagate".to_string()
            } else {
                origin
            };
            Ok(Value::Err {
                message: final_message,
                code: code_override.unwrap_or(base_code),
                origin: final_origin,
                cause: Some(Box::new(Value::Err {
                    message: base_message,
                    code: base_code,
                    origin: base_origin,
                    cause: base_cause,
                })),
            })
        }
        Value::StructInstance { type_name, fields } => {
            let Some(type_slots) = struct_field_slots.get(type_name.as_str()) else {
                return Err(Diagnostic::new(
                    format!("unknown struct type '{}'", type_name),
                    span,
                    Severity::Error,
                ));
            };
            let Some(message_slot) = type_slots.get("message").copied() else {
                return Err(Diagnostic::new(
                    format!("error-like struct `{type_name}` must have `message: str`"),
                    span,
                    Severity::Error,
                ));
            };
            let base_message = match fields.get(message_slot) {
                Some(Value::Str(s)) => s.clone(),
                _ => {
                    return Err(Diagnostic::new(
                        format!("error-like struct `{type_name}` must have `message: str`"),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let Some(code_slot) = type_slots.get("code").copied() else {
                return Err(Diagnostic::new(
                    format!("error-like struct `{type_name}` must have `code: i32`"),
                    span,
                    Severity::Error,
                ));
            };
            let base_code = match fields.get(code_slot) {
                Some(Value::Int128(n)) => i32::try_from(*n).map_err(|_| {
                    Diagnostic::new(
                        format!("error-like struct `{type_name}` has code outside i32 range"),
                        span,
                        Severity::Error,
                    )
                })?,
                Some(Value::UInt128(n)) => i32::try_from(*n).map_err(|_| {
                    Diagnostic::new(
                        format!("error-like struct `{type_name}` has code outside i32 range"),
                        span,
                        Severity::Error,
                    )
                })?,
                _ => {
                    return Err(Diagnostic::new(
                        format!("error-like struct `{type_name}` must have `code: i32`"),
                        span,
                        Severity::Error,
                    ));
                }
            };
            let is_propagation_wrap = message == "propagated by ?";
            let final_message = if is_propagation_wrap {
                base_message
            } else {
                message
            };
            let final_origin = if is_propagation_wrap {
                "propagate".to_string()
            } else {
                origin
            };
            Ok(Value::Err {
                message: final_message,
                code: code_override.unwrap_or(base_code),
                origin: final_origin,
                cause: Some(Box::new(Value::StructInstance { type_name, fields })),
            })
        }
        other => Err(Diagnostic::new(
            format!(
                "wrap expects err-like as first argument, got {}",
                builtin_type_name(&other)
            ),
            span,
            Severity::Error,
        )),
    }
}

fn coerce_i32_code(value: &Value, span: Span, fn_name: &str) -> Result<i32, Diagnostic> {
    match value {
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Signed(i) => i32::try_from(i).map_err(|_| {
                Diagnostic::new(
                    format!("{fn_name} code is out of i32 range"),
                    span,
                    Severity::Error,
                )
            }),
            super::int::IntPayload::Unsigned(u) => i32::try_from(u).map_err(|_| {
                Diagnostic::new(
                    format!("{fn_name} code is out of i32 range"),
                    span,
                    Severity::Error,
                )
            }),
        },
        Value::Int128(n) => i32::try_from(*n).map_err(|_| {
            Diagnostic::new(
                format!("{fn_name} code is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(n) => i32::try_from(*n).map_err(|_| {
            Diagnostic::new(
                format!("{fn_name} code is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!(
                "{fn_name} code must be i32-compatible, got {}",
                builtin_type_name(other)
            ),
            span,
            Severity::Error,
        )),
    }
}

fn io_err(origin: &str, err: std::io::Error) -> Value {
    Value::Err {
        message: err.to_string(),
        code: 1,
        origin: origin.to_string(),
        cause: None,
    }
}

fn expect_str_arg<'a>(value: &'a Value, label: &str, span: Span) -> Result<&'a str, Diagnostic> {
    match value {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(Diagnostic::new(
            format!("{label} must be str, got {}", builtin_type_name(other)),
            span,
            Severity::Error,
        )),
    }
}

fn expect_f64_arg(value: &Value, label: &str, span: Span) -> Result<f64, Diagnostic> {
    match value {
        Value::Float(v) => Ok(*v),
        other => Err(Diagnostic::new(
            format!("{label} must be f64, got {}", builtin_type_name(other)),
            span,
            Severity::Error,
        )),
    }
}

fn expect_i32_like(value: &Value, label: &str, span: Span) -> Result<i32, Diagnostic> {
    match value {
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Signed(i) => i32::try_from(i).map_err(|_| {
                Diagnostic::new(
                    format!("{label} is out of i32 range"),
                    span,
                    Severity::Error,
                )
            }),
            super::int::IntPayload::Unsigned(u) => i32::try_from(u).map_err(|_| {
                Diagnostic::new(
                    format!("{label} is out of i32 range"),
                    span,
                    Severity::Error,
                )
            }),
        },
        Value::Int128(v) => i32::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label} is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(v) => i32::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label} is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!(
                "{label} must be i32-compatible, got {}",
                builtin_type_name(other)
            ),
            span,
            Severity::Error,
        )),
    }
}

fn expect_u64_arg(value: &Value, label: &str, span: Span) -> Result<u64, Diagnostic> {
    match value {
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Unsigned(u) => u64::try_from(u).map_err(|_| {
                Diagnostic::new(
                    format!("{label} is out of u64 range"),
                    span,
                    Severity::Error,
                )
            }),
            super::int::IntPayload::Signed(i) if i >= 0 => u64::try_from(i).map_err(|_| {
                Diagnostic::new(
                    format!("{label} is out of u64 range"),
                    span,
                    Severity::Error,
                )
            }),
            _ => Err(Diagnostic::new(
                format!("{label} must be non-negative"),
                span,
                Severity::Error,
            )),
        },
        Value::UInt128(v) => u64::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label} is out of u64 range"),
                span,
                Severity::Error,
            )
        }),
        Value::Int128(v) if *v >= 0 => u64::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label} is out of u64 range"),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!(
                "{label} must be u64-compatible, got {}",
                builtin_type_name(other)
            ),
            span,
            Severity::Error,
        )),
    }
}

fn time_tick_start() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

fn proc_table() -> &'static Mutex<HashMap<u32, Child>> {
    static CHILDREN: OnceLock<Mutex<HashMap<u32, Child>>> = OnceLock::new();
    CHILDREN.get_or_init(|| Mutex::new(HashMap::default()))
}

fn proc_take_child(pid: u64, span: Span) -> Result<Option<Child>, Diagnostic> {
    let pid = u32::try_from(pid)
        .map_err(|_| Diagnostic::new("pid out of range", span, Severity::Error))?;
    let mut table = proc_table()
        .lock()
        .map_err(|_| Diagnostic::new("failed to lock process table", span, Severity::Error))?;
    Ok(table.remove(&pid))
}

fn run_shell_command(command: &str) -> std::io::Result<std::process::Output> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", command]).output()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh").args(["-c", command]).output()
    }
}

fn spawn_shell_command(command: &str) -> std::io::Result<Child> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", command])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .args(["-c", command])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
}

fn thread_table() -> &'static Mutex<HashMap<u64, std::thread::JoinHandle<i32>>> {
    static THREADS: OnceLock<Mutex<HashMap<u64, std::thread::JoinHandle<i32>>>> = OnceLock::new();
    THREADS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_thread_id() -> u64 {
    static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed)
}

fn mutex_table() -> &'static Mutex<HashMap<u64, bool>> {
    static MUTEXES: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
    MUTEXES.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_mutex_id() -> u64 {
    static NEXT_MUTEX_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_MUTEX_ID.fetch_add(1, Ordering::Relaxed)
}

fn sync_mutex_table() -> &'static Mutex<HashMap<u64, bool>> {
    static SYNC_MUTEXES: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
    SYNC_MUTEXES.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_sync_mutex_id() -> u64 {
    static NEXT_SYNC_MUTEX_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_SYNC_MUTEX_ID.fetch_add(1, Ordering::Relaxed)
}

fn atomic_table() -> &'static Mutex<HashMap<u64, i64>> {
    static ATOMICS: OnceLock<Mutex<HashMap<u64, i64>>> = OnceLock::new();
    ATOMICS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_atomic_id() -> u64 {
    static NEXT_ATOMIC_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ATOMIC_ID.fetch_add(1, Ordering::Relaxed)
}

fn channel_table() -> &'static Mutex<HashMap<u64, VecDeque<Value>>> {
    static CHANNELS: OnceLock<Mutex<HashMap<u64, VecDeque<Value>>>> = OnceLock::new();
    CHANNELS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_channel_id() -> u64 {
    static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed)
}

fn expect_i64_like(value: &Value, label: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Signed(i) => i64::try_from(i).map_err(|_| {
                Diagnostic::new(
                    format!("{label}: integer out of range for i64"),
                    span,
                    Severity::Error,
                )
            }),
            super::int::IntPayload::Unsigned(u) => i64::try_from(u).map_err(|_| {
                Diagnostic::new(
                    format!("{label}: unsigned integer out of range for i64"),
                    span,
                    Severity::Error,
                )
            }),
        },
        Value::Int128(v) => i64::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label}: integer out of range for i64"),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(v) => i64::try_from(*v).map_err(|_| {
            Diagnostic::new(
                format!("{label}: unsigned integer out of range for i64"),
                span,
                Severity::Error,
            )
        }),
        _ => Err(Diagnostic::new(
            format!("{label}: expected integer argument"),
            span,
            Severity::Error,
        )),
    }
}

fn udp_socket_table() -> &'static Mutex<HashMap<u64, UdpSocket>> {
    static UDP_SOCKETS: OnceLock<Mutex<HashMap<u64, UdpSocket>>> = OnceLock::new();
    UDP_SOCKETS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_udp_socket_id() -> u64 {
    static NEXT_UDP_SOCKET_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_UDP_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

fn tcp_stream_table() -> &'static Mutex<HashMap<u64, TcpStream>> {
    static TCP_STREAMS: OnceLock<Mutex<HashMap<u64, TcpStream>>> = OnceLock::new();
    TCP_STREAMS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_tcp_stream_id() -> u64 {
    static NEXT_TCP_STREAM_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_TCP_STREAM_ID.fetch_add(1, Ordering::Relaxed)
}

fn http_listener_table() -> &'static Mutex<HashMap<u64, TcpListener>> {
    static HTTP_LISTENERS: OnceLock<Mutex<HashMap<u64, TcpListener>>> = OnceLock::new();
    HTTP_LISTENERS.get_or_init(|| Mutex::new(HashMap::default()))
}

fn next_http_listener_id() -> u64 {
    static NEXT_HTTP_LISTENER_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_HTTP_LISTENER_ID.fetch_add(1, Ordering::Relaxed)
}

fn parse_http_headers(raw: &str) -> Value {
    let mut entries = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            entries.push((
                Value::Str(k.trim().to_string()),
                Value::Str(v.trim().to_string()),
            ));
        }
    }
    Value::Map(entries)
}

fn parse_http_response(raw: &str) -> Result<(i32, String, Value, String), String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or_else(|| "invalid HTTP response: missing header/body separator".to_string())?;
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "invalid HTTP response: missing status line".to_string())?;
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts
        .next()
        .ok_or_else(|| "invalid status line".to_string())?;
    let code_str = parts
        .next()
        .ok_or_else(|| "invalid status line".to_string())?;
    let reason = parts.next().unwrap_or("").to_string();
    let code = code_str
        .parse::<i32>()
        .map_err(|_| "invalid status code".to_string())?;
    let headers = parse_http_headers(&lines.collect::<Vec<_>>().join("\n"));
    Ok((code, reason, headers, body.to_string()))
}

fn parse_http_request(raw: &str) -> Result<(String, String, String, Value, String), String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or_else(|| "invalid HTTP request: missing header/body separator".to_string())?;
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| "invalid HTTP request: missing request line".to_string())?;
    let mut parts = request_line.splitn(3, ' ');
    let method = parts
        .next()
        .ok_or_else(|| "invalid request line".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "invalid request line".to_string())?
        .to_string();
    let version = parts
        .next()
        .ok_or_else(|| "invalid request line".to_string())?
        .to_string();
    let headers = parse_http_headers(&lines.collect::<Vec<_>>().join("\n"));
    Ok((method, path, version, headers, body.to_string()))
}

fn http_get_simple(url: &str) -> std::io::Result<String> {
    let without_scheme = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = match without_scheme.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p)),
        None => (without_scheme, "/".to_string()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(80)),
        None => (host_port, 80),
    };
    let mut stream = TcpStream::connect((host, port))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    stream.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    hex_encode(&digest)
}

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("invalid hex length".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let to_nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        let hi = to_nibble(bytes[i]).ok_or_else(|| "invalid hex char".to_string())?;
        let lo = to_nibble(bytes[i + 1]).ok_or_else(|| "invalid hex char".to_string())?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn secure_random_fill(buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut f = File::open("/dev/urandom")?;
        f.read_exact(buf)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = buf;
        Err(std::io::Error::other(
            "secure random is not implemented on this platform",
        ))
    }
}

fn keystream_bytes(key: &[u8], nonce: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut counter: u64 = 0;
    while out.len() < len {
        let mut h = Sha256::new();
        h.update(key);
        h.update(nonce);
        h.update(counter.to_le_bytes());
        let block = h.finalize();
        let need = (len - out.len()).min(block.len());
        out.extend_from_slice(&block[..need]);
        counter = counter.wrapping_add(1);
    }
    out
}

fn encrypt_with_key(plain: &[u8], key: &[u8]) -> std::io::Result<String> {
    let mut nonce = [0_u8; 12];
    secure_random_fill(&mut nonce)?;
    let stream = keystream_bytes(key, &nonce, plain.len());
    let cipher: Vec<u8> = plain
        .iter()
        .zip(stream.iter())
        .map(|(p, k)| p ^ k)
        .collect();
    Ok(format!("{}:{}", hex_encode(&nonce), hex_encode(&cipher)))
}

fn decrypt_with_key(blob: &str, key: &[u8]) -> Result<String, String> {
    let (nonce_hex, cipher_hex) = blob
        .split_once(':')
        .ok_or_else(|| "invalid encrypted payload format".to_string())?;
    let nonce = hex_decode(nonce_hex)?;
    let cipher = hex_decode(cipher_hex)?;
    let stream = keystream_bytes(key, &nonce, cipher.len());
    let plain: Vec<u8> = cipher
        .iter()
        .zip(stream.iter())
        .map(|(c, k)| c ^ k)
        .collect();
    String::from_utf8(plain).map_err(|_| "decrypted data is not valid utf-8".to_string())
}

fn cli_script_args() -> Vec<String> {
    let all = std::env::args().collect::<Vec<_>>();
    if all.len() <= 2 {
        Vec::new()
    } else {
        all.into_iter().skip(2).collect()
    }
}

fn cli_positional_args() -> Vec<String> {
    let args = cli_script_args();
    let mut positional = Vec::new();
    let mut parse_options = true;
    for arg in args {
        if parse_options && arg == "--" {
            parse_options = false;
            continue;
        }
        if parse_options && arg.starts_with('-') {
            continue;
        }
        positional.push(arg);
    }
    positional
}

fn normalize_flag(flag: &str) -> String {
    flag.trim_start_matches('-').to_string()
}

fn cli_has_flag(flag: &str) -> bool {
    let want = normalize_flag(flag);
    let args = cli_script_args();
    let mut parse_options = true;
    for arg in args {
        if parse_options && arg == "--" {
            parse_options = false;
            continue;
        }
        if !parse_options {
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or_default();
            if name == want {
                return true;
            }
            continue;
        }
        if let Some(shorts) = arg.strip_prefix('-') {
            if shorts.is_empty() {
                continue;
            }
            if shorts.chars().any(|c| c.to_string() == want) {
                return true;
            }
        }
    }
    false
}

fn cli_flag_value(flag: &str) -> Option<String> {
    let want = normalize_flag(flag);
    let args = cli_script_args();
    let mut parse_options = true;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if parse_options && arg == "--" {
            parse_options = false;
            i += 1;
            continue;
        }
        if !parse_options {
            i += 1;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            if let Some((name, value)) = long.split_once('=') {
                if name == want {
                    return Some(value.to_string());
                }
            } else if long == want && i + 1 < args.len() && !args[i + 1].starts_with('-') {
                return Some(args[i + 1].clone());
            }
            i += 1;
            continue;
        }
        if let Some(shorts) = arg.strip_prefix('-') {
            if shorts == want && i + 1 < args.len() && !args[i + 1].starts_with('-') {
                return Some(args[i + 1].clone());
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
struct LogState {
    min_level: LogLevel,
    prefix: String,
    json: bool,
}

fn log_state() -> &'static Mutex<LogState> {
    static LOG_STATE: OnceLock<Mutex<LogState>> = OnceLock::new();
    LOG_STATE.get_or_init(|| {
        Mutex::new(LogState {
            min_level: LogLevel::Info,
            prefix: String::new(),
            json: false,
        })
    })
}

fn parse_log_level(level: &str) -> Option<LogLevel> {
    match level.to_ascii_lowercase().as_str() {
        "trace" => Some(LogLevel::Trace),
        "debug" => Some(LogLevel::Debug),
        "info" => Some(LogLevel::Info),
        "warn" | "warning" => Some(LogLevel::Warn),
        "error" => Some(LogLevel::Error),
        _ => None,
    }
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "TRACE",
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

fn log_state_set_level(level: LogLevel) {
    if let Ok(mut state) = log_state().lock() {
        state.min_level = level;
    }
}

fn log_state_set_prefix(prefix: String) {
    if let Ok(mut state) = log_state().lock() {
        state.prefix = prefix;
    }
}

fn log_state_set_json(json: bool) {
    if let Ok(mut state) = log_state().lock() {
        state.json = json;
    }
}

fn log_emit(level: LogLevel, message: &str) {
    if let Ok(state) = log_state().lock() {
        if level < state.min_level {
            return;
        }
        if state.json {
            let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
            if state.prefix.is_empty() {
                println!(r#"{{"level":"{}","msg":"{}"}}"#, level_name(level), escaped);
            } else {
                let p = state.prefix.replace('\\', "\\\\").replace('"', "\\\"");
                println!(
                    r#"{{"level":"{}","prefix":"{}","msg":"{}"}}"#,
                    level_name(level),
                    p,
                    escaped
                );
            }
        } else if state.prefix.is_empty() {
            println!("[{}] {}", level_name(level), message);
        } else {
            println!("{} [{}] {}", state.prefix, level_name(level), message);
        }
    }
}

fn json_to_value(json: &JsonValue) -> Value {
    match json {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(v) => Value::Bool(*v),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int128(i as i128)
            } else if let Some(u) = n.as_u64() {
                Value::UInt128(u as u128)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        JsonValue::String(s) => Value::Str(s.clone()),
        JsonValue::Array(items) => Value::Array(items.iter().map(json_to_value).collect()),
        JsonValue::Object(map) => Value::Map(
            map.iter()
                .map(|(k, v)| (Value::Str(k.clone()), json_to_value(v)))
                .collect(),
        ),
    }
}

fn value_to_json(value: &Value) -> Result<JsonValue, String> {
    match value {
        Value::Null => Ok(JsonValue::Null),
        Value::Bool(v) => Ok(JsonValue::Bool(*v)),
        Value::Int128(v) => {
            let iv = i64::try_from(*v).map_err(|_| "integer out of JSON i64 range".to_string())?;
            Ok(JsonValue::from(iv))
        }
        Value::UInt128(v) => {
            let uv = u64::try_from(*v).map_err(|_| "integer out of JSON u64 range".to_string())?;
            Ok(JsonValue::from(uv))
        }
        Value::Float(v) => serde_json::Number::from_f64(*v)
            .map(JsonValue::Number)
            .ok_or_else(|| "cannot encode NaN/Infinity in JSON".to_string()),
        Value::Str(s) => Ok(JsonValue::String(s.clone())),
        Value::Array(items) => items
            .iter()
            .map(value_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array),
        Value::Map(entries) => {
            let mut out = serde_json::Map::new();
            for (k, v) in entries {
                let Value::Str(key) = k else {
                    return Err("json stringify supports map keys of type str only".to_string());
                };
                out.insert(key.clone(), value_to_json(v)?);
            }
            Ok(JsonValue::Object(out))
        }
        _ => Err("value is not JSON-serializable".to_string()),
    }
}

fn parse_csv_basic(raw: &str) -> Result<Value, String> {
    let mut reader = CsvReaderBuilder::new()
        .has_headers(false)
        .from_reader(raw.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        rows.push(Value::Array(
            record
                .iter()
                .map(|field| Value::Str(field.to_string()))
                .collect(),
        ));
    }
    Ok(Value::Array(rows))
}

fn stringify_csv_basic(value: &Value) -> Result<String, String> {
    let Value::Array(rows) = value else {
        return Err("csv stringify expects array rows".to_string());
    };
    let mut writer = CsvWriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    for row in rows {
        let Value::Array(cells) = row else {
            return Err("csv stringify expects rows as arrays".to_string());
        };
        let mut record = Vec::with_capacity(cells.len());
        for cell in cells {
            let Value::Str(text) = cell else {
                return Err("csv stringify expects cell values of type str".to_string());
            };
            record.push(text.as_str());
        }
        writer.write_record(&record).map_err(|e| e.to_string())?;
    }
    let bytes = writer.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn mime_guess_from_path(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    mime_from_extension(ext)
}

fn mime_from_extension(ext: &str) -> String {
    let normalized = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match normalized.as_str() {
        "txt" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn mime_is_textual(mime: &str) -> bool {
    let bare = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    bare.starts_with("text/")
        || bare == "application/json"
        || bare == "application/xml"
        || bare == "application/javascript"
        || bare == "image/svg+xml"
}

fn parse_url_basic(raw: &str) -> Result<Value, String> {
    let scheme_end = raw
        .find("://")
        .ok_or_else(|| "url must contain scheme separator '://'".to_string())?;
    let scheme = &raw[..scheme_end];
    if scheme.is_empty() {
        return Err("url scheme is empty".to_string());
    }
    let rest = &raw[(scheme_end + 3)..];
    if rest.is_empty() {
        return Err("url host is empty".to_string());
    }
    let (before_fragment, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let (before_query, query) = match before_fragment.split_once('?') {
        Some((a, b)) => (a, b),
        None => (before_fragment, ""),
    };
    let (host, path) = match before_query.find('/') {
        Some(idx) => (&before_query[..idx], &before_query[idx..]),
        None => (before_query, "/"),
    };
    if host.is_empty() {
        return Err("url host is empty".to_string());
    }
    Ok(Value::Map(vec![
        (
            Value::Str("scheme".to_string()),
            Value::Str(scheme.to_string()),
        ),
        (Value::Str("host".to_string()), Value::Str(host.to_string())),
        (Value::Str("path".to_string()), Value::Str(path.to_string())),
        (
            Value::Str("query".to_string()),
            Value::Str(query.to_string()),
        ),
        (
            Value::Str("fragment".to_string()),
            Value::Str(fragment.to_string()),
        ),
    ]))
}

fn build_url_basic(
    scheme: &str,
    host: &str,
    path: &str,
    query: &str,
    fragment: &str,
) -> Result<String, String> {
    if scheme.is_empty() {
        return Err("url scheme cannot be empty".to_string());
    }
    if host.is_empty() {
        return Err("url host cannot be empty".to_string());
    }
    let safe_path = if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let mut out = format!("{scheme}://{host}{safe_path}");
    if !query.is_empty() {
        out.push('?');
        out.push_str(query);
    }
    if !fragment.is_empty() {
        out.push('#');
        out.push_str(fragment);
    }
    Ok(out)
}

fn url_encode_component_basic(input: &str) -> String {
    let mut out = String::new();
    for b in input.as_bytes() {
        let keep = (*b >= b'a' && *b <= b'z')
            || (*b >= b'A' && *b <= b'Z')
            || (*b >= b'0' && *b <= b'9')
            || matches!(*b, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn url_decode_component_basic(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("incomplete percent-encoding".to_string());
                }
                let hi =
                    hex_val(bytes[i + 1]).ok_or_else(|| "invalid percent-encoding".to_string())?;
                let lo =
                    hex_val(bytes[i + 2]).ok_or_else(|| "invalid percent-encoding".to_string())?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "decoded url component is not valid utf-8".to_string())
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + (c - b'a')),
        b'A'..=b'F' => Some(10 + (c - b'A')),
        _ => None,
    }
}

fn parse_xml_basic(raw: &str) -> Result<Value, String> {
    let mut reader = XmlReader::from_str(raw);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut root_name = String::new();
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) if root_name.is_empty() => {
                root_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Ok(XmlEvent::Text(t)) => {
                let s = String::from_utf8_lossy(t.as_ref()).to_string();
                if !s.trim().is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(s.trim());
                }
            }
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        buf.clear();
    }
    if root_name.is_empty() {
        return Err("xml has no root element".to_string());
    }
    Ok(Value::Map(vec![
        (Value::Str("name".to_string()), Value::Str(root_name)),
        (Value::Str("text".to_string()), Value::Str(text)),
    ]))
}

fn xml_escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn path_normalize_lex(path: &str) -> String {
    use std::path::{Component, Path};

    let p = Path::new(path);
    let mut prefix: Option<String> = None;
    let mut has_root = false;
    let mut parts: Vec<String> = Vec::new();

    for comp in p.components() {
        match comp {
            Component::Prefix(px) => {
                prefix = Some(px.as_os_str().to_string_lossy().to_string());
            }
            Component::RootDir => {
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = parts.last() {
                    if last != ".." {
                        parts.pop();
                    } else if !has_root {
                        parts.push("..".to_string());
                    }
                } else if !has_root {
                    parts.push("..".to_string());
                }
            }
            Component::Normal(seg) => {
                parts.push(seg.to_string_lossy().to_string());
            }
        }
    }

    let mut out = String::new();
    if let Some(px) = prefix {
        out.push_str(&px);
    }
    if has_root {
        out.push(std::path::MAIN_SEPARATOR);
    }
    if !parts.is_empty() {
        if !out.is_empty() && !out.ends_with(std::path::MAIN_SEPARATOR) {
            out.push(std::path::MAIN_SEPARATOR);
        }
        let sep = std::path::MAIN_SEPARATOR.to_string();
        out.push_str(&parts.join(&sep));
    }

    if out.is_empty() {
        if has_root {
            std::path::MAIN_SEPARATOR.to_string()
        } else {
            ".".to_string()
        }
    } else {
        out
    }
}

fn format_unix_secs_iso_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let sod = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = sod / 3600;
    let min = (sod % 3600) / 60;
    let sec = sod % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = (yoe as i32) + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (mp + if mp < 10 { 3 } else { -9 }) as u32;
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

fn math_next_u64() -> u64 {
    let state = rand_state();
    let mut s = state.load(Ordering::Relaxed);
    if s == 0 {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        s = seed ^ 0xD1B54A32D192ED03;
    }
    s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    state.store(s, Ordering::Relaxed);
    s
}

fn rand_set_seed(seed: u64) {
    let state = rand_state();
    let seeded = seed ^ 0xD1B54A32D192ED03;
    state.store(seeded.max(1), Ordering::Relaxed);
}

fn rand_state() -> &'static AtomicU64 {
    static RNG_STATE: AtomicU64 = AtomicU64::new(0);
    &RNG_STATE
}

fn as_i128(value: &Value) -> Option<i128> {
    match value {
        Value::Int(v) => match v.payload() {
            super::int::IntPayload::Signed(i) => Some(i),
            super::int::IntPayload::Unsigned(u) => i128::try_from(u).ok(),
        },
        Value::Int128(v) => Some(*v),
        Value::UInt128(v) => i128::try_from(*v).ok(),
        _ => None,
    }
}

fn value_to_typed_int(value: &Value) -> Option<super::int::TypedInt> {
    match value {
        Value::Int(v) => Some(*v),
        Value::Int128(v) => {
            super::int::TypedInt::try_from_signed(super::int::IntTag::I128, *v).ok()
        }
        Value::UInt128(v) => {
            super::int::TypedInt::try_from_unsigned(super::int::IntTag::U128, *v).ok()
        }
        _ => None,
    }
}

fn cast_typed_int_to_tag(
    value: super::int::TypedInt,
    target: super::int::IntTag,
) -> Result<super::int::TypedInt, &'static str> {
    match value.payload() {
        super::int::IntPayload::Signed(i) => {
            if target.is_signed() {
                super::int::TypedInt::try_from_signed(target, i)
            } else if i >= 0 {
                super::int::TypedInt::try_from_unsigned(target, i as u128)
            } else {
                Err("integer type mismatch")
            }
        }
        super::int::IntPayload::Unsigned(u) => {
            if target.is_signed() {
                let i = i128::try_from(u).map_err(|_| "integer type mismatch")?;
                super::int::TypedInt::try_from_signed(target, i)
            } else {
                super::int::TypedInt::try_from_unsigned(target, u)
            }
        }
    }
}

fn int_rotate(
    lhs: super::int::TypedInt,
    rhs: super::int::TypedInt,
    left: bool,
) -> Result<super::int::TypedInt, &'static str> {
    let shift = match rhs.payload() {
        super::int::IntPayload::Signed(i) if i >= 0 => i as u32,
        super::int::IntPayload::Unsigned(u) => {
            u32::try_from(u).map_err(|_| "shift amount out of range")?
        }
        _ => return Err("shift amount must be non-negative"),
    };
    match lhs.tag() {
        super::int::IntTag::I8 => {
            let v = lhs.as_signed().ok_or("integer payload mismatch")? as i8;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_signed(lhs.tag(), i128::from(out))
        }
        super::int::IntTag::I16 => {
            let v = lhs.as_signed().ok_or("integer payload mismatch")? as i16;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_signed(lhs.tag(), i128::from(out))
        }
        super::int::IntTag::I32 => {
            let v = lhs.as_signed().ok_or("integer payload mismatch")? as i32;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_signed(lhs.tag(), i128::from(out))
        }
        super::int::IntTag::I64 => {
            let v = lhs.as_signed().ok_or("integer payload mismatch")? as i64;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_signed(lhs.tag(), i128::from(out))
        }
        super::int::IntTag::I128 => {
            let v = lhs.as_signed().ok_or("integer payload mismatch")?;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_signed(lhs.tag(), out)
        }
        super::int::IntTag::U1 => super::int::TypedInt::try_from_unsigned(
            lhs.tag(),
            lhs.as_unsigned().ok_or("integer payload mismatch")?,
        ),
        super::int::IntTag::U8 => {
            let v = lhs.as_unsigned().ok_or("integer payload mismatch")? as u8;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_unsigned(lhs.tag(), u128::from(out))
        }
        super::int::IntTag::U16 => {
            let v = lhs.as_unsigned().ok_or("integer payload mismatch")? as u16;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_unsigned(lhs.tag(), u128::from(out))
        }
        super::int::IntTag::U32 => {
            let v = lhs.as_unsigned().ok_or("integer payload mismatch")? as u32;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_unsigned(lhs.tag(), u128::from(out))
        }
        super::int::IntTag::U64 => {
            let v = lhs.as_unsigned().ok_or("integer payload mismatch")? as u64;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_unsigned(lhs.tag(), u128::from(out))
        }
        super::int::IntTag::U128 => {
            let v = lhs.as_unsigned().ok_or("integer payload mismatch")?;
            let out = if left {
                v.rotate_left(shift)
            } else {
                v.rotate_right(shift)
            };
            super::int::TypedInt::try_from_unsigned(lhs.tag(), out)
        }
    }
}

fn compare_integer_values(
    lhs: &Value,
    rhs: &Value,
) -> Option<Result<std::cmp::Ordering, &'static str>> {
    let l = value_to_typed_int(lhs)?;
    let r = value_to_typed_int(rhs)?;
    Some(match (l.payload(), r.payload()) {
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Signed(b)) => Ok(a.cmp(&b)),
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Unsigned(b)) => Ok(a.cmp(&b)),
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Unsigned(b)) => {
            if a < 0 {
                Ok(std::cmp::Ordering::Less)
            } else {
                Ok((a as u128).cmp(&b))
            }
        }
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Signed(b)) => {
            if b < 0 {
                Ok(std::cmp::Ordering::Greater)
            } else {
                Ok(a.cmp(&(b as u128)))
            }
        }
    })
}

fn int_wrapping_add(
    lhs: super::int::TypedInt,
    rhs: super::int::TypedInt,
) -> Result<super::int::TypedInt, &'static str> {
    if lhs.tag() != rhs.tag() {
        return Err("integer type mismatch");
    }
    match (lhs.payload(), rhs.payload()) {
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Signed(b)) => {
            super::int::TypedInt::try_from_signed(lhs.tag(), a.wrapping_add(b))
        }
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Unsigned(b)) => {
            super::int::TypedInt::try_from_unsigned(lhs.tag(), a.wrapping_add(b))
        }
        _ => Err("integer payload mismatch"),
    }
}

fn int_wrapping_sub(
    lhs: super::int::TypedInt,
    rhs: super::int::TypedInt,
) -> Result<super::int::TypedInt, &'static str> {
    if lhs.tag() != rhs.tag() {
        return Err("integer type mismatch");
    }
    match (lhs.payload(), rhs.payload()) {
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Signed(b)) => {
            super::int::TypedInt::try_from_signed(lhs.tag(), a.wrapping_sub(b))
        }
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Unsigned(b)) => {
            super::int::TypedInt::try_from_unsigned(lhs.tag(), a.wrapping_sub(b))
        }
        _ => Err("integer payload mismatch"),
    }
}

fn int_saturating_add(
    lhs: super::int::TypedInt,
    rhs: super::int::TypedInt,
) -> Result<super::int::TypedInt, &'static str> {
    if lhs.tag() != rhs.tag() {
        return Err("integer type mismatch");
    }
    match (lhs.payload(), rhs.payload()) {
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Signed(b)) => {
            super::int::TypedInt::try_from_signed(lhs.tag(), a.saturating_add(b))
        }
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Unsigned(b)) => {
            super::int::TypedInt::try_from_unsigned(lhs.tag(), a.saturating_add(b))
        }
        _ => Err("integer payload mismatch"),
    }
}

fn int_saturating_sub(
    lhs: super::int::TypedInt,
    rhs: super::int::TypedInt,
) -> Result<super::int::TypedInt, &'static str> {
    if lhs.tag() != rhs.tag() {
        return Err("integer type mismatch");
    }
    match (lhs.payload(), rhs.payload()) {
        (super::int::IntPayload::Signed(a), super::int::IntPayload::Signed(b)) => {
            super::int::TypedInt::try_from_signed(lhs.tag(), a.saturating_sub(b))
        }
        (super::int::IntPayload::Unsigned(a), super::int::IntPayload::Unsigned(b)) => {
            super::int::TypedInt::try_from_unsigned(lhs.tag(), a.saturating_sub(b))
        }
        _ => Err("integer payload mismatch"),
    }
}

fn str_pad_impl(s: &str, total: i128, pad: &str, left: bool) -> String {
    let current = s.chars().count() as i128;
    if total <= current {
        return s.to_string();
    }
    let needed = (total - current) as usize;
    let pad_unit = if pad.is_empty() { " " } else { pad };
    let mut fill = String::new();
    while fill.chars().count() < needed {
        fill.push_str(pad_unit);
    }
    let trimmed = fill.chars().take(needed).collect::<String>();
    if left {
        format!("{trimmed}{s}")
    } else {
        format!("{s}{trimmed}")
    }
}

/// Run a full program chunk. Requires `chunk.invariant_holds()` and terminates with [`Op::Return`].
/// Arguments are passed to the program as a global `argv` array of strings.
pub fn execute(chunk: &Chunk, symbols: &SymbolTable, args: Vec<String>) -> Result<(), Diagnostics> {
    debug_assert!(chunk.invariant_holds());
    let mut initial_env = HashMap::default();

    // Create argv as an array of strings if args are provided
    if !args.is_empty() {
        let argv_values: Vec<Value> = args.into_iter().map(Value::Str).collect();
        // Try to find argv in symbol table; use a high SymbolId as fallback
        let argv_sym = symbols
            .resolve(crate::hir::symbols::ScopeId(0), "argv")
            .unwrap_or(SymbolId(u32::MAX - 1));
        initial_env.insert(argv_sym, Value::Array(argv_values));
    }

    let mut vm = Vm::new(chunk, symbols, initial_env);
    vm.run()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use foundation::ids::FileId;

    use crate::builtins::default_registry;
    use crate::hir::symbols::SymbolTable;
    use crate::vm::bytecode::Op;
    use crate::vm::chunk::ChunkBuilder;

    use super::*;

    fn span() -> Span {
        Span::new_unchecked(FileId::from_u32(0), 0, 1)
    }

    fn dispatch_builtin_source_names() -> HashSet<&'static str> {
        let source = include_str!("engine.rs");
        let start = source
            .find("fn dispatch_builtin(")
            .expect("dispatch_builtin start");
        let end = source[start..]
            .find("fn dispatch_integer_method(")
            .map(|offset| start + offset)
            .expect("dispatch_builtin end");
        let dispatch_source = &source[start..end];

        let mut names = HashSet::new();
        for line in dispatch_source.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('"') || !trimmed.contains("=>") {
                continue;
            }
            let Some(patterns) = trimmed.split("=>").next() else {
                continue;
            };
            for branch in patterns.split('|') {
                let branch = branch.trim();
                if let Some(name) = branch
                    .strip_prefix('"')
                    .and_then(|rest| rest.split('"').next())
                {
                    names.insert(name);
                }
            }
        }

        names
    }

    #[test]
    fn runtime_dispatch_covers_every_registered_builtin_function() {
        let registry = default_registry();
        let runtime_names = dispatch_builtin_source_names();
        let registry_names = registry
            .functions
            .iter()
            .map(|function| function.name)
            .filter(|name| !name.starts_with("__meth_"))
            .collect::<HashSet<_>>();

        let missing_in_runtime = registry_names
            .difference(&runtime_names)
            .copied()
            .collect::<Vec<_>>();
        let missing_in_registry = runtime_names
            .difference(&registry_names)
            .copied()
            .collect::<Vec<_>>();

        assert!(
            missing_in_runtime.is_empty(),
            "registered builtin functions missing runtime dispatch: {missing_in_runtime:?}"
        );
        assert!(
            missing_in_registry.is_empty(),
            "runtime dispatch exposes builtin functions missing from registry: {missing_in_registry:?}"
        );
    }

    #[test]
    fn execute_int_add_returns_empty_stack() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("ok");
    }

    #[test]
    fn div_by_zero_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::Div, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols, Vec::new()).expect_err("div0");
        assert!(
            err.iter().any(|d| {
                d.message.contains("division by zero")
                    || d.message.contains("integer division by zero")
            }),
            "unexpected div by zero diagnostics: {:?}",
            err
        );
    }

    #[test]
    fn exit_scope_removes_local_binding() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let local = symbols.define(
            root,
            "local".to_string(),
            crate::analyzer::Type::Int {
                signed: true,
                bits: 32,
            },
            crate::hir::SymbolOrigin::User,
            false,
        );

        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::EnterScope, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::Bind(local), s);
        b.emit(Op::ExitScope, s);
        b.emit(Op::Load(local), s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let err = execute(&chunk, &symbols, Vec::new()).expect_err("missing local after exit");
        assert!(err.iter().any(|d| {
            d.message
                .contains("load of uninitialized or missing symbol")
        }));
    }

    #[test]
    fn exit_scope_keeps_outer_binding_alive() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let outer = symbols.define(
            root,
            "outer".to_string(),
            crate::analyzer::Type::Int {
                signed: true,
                bits: 32,
            },
            crate::hir::SymbolOrigin::User,
            false,
        );
        let inner_scope = symbols.create_scope(Some(root));
        let inner = symbols.define(
            inner_scope,
            "outer".to_string(),
            crate::analyzer::Type::Int {
                signed: true,
                bits: 32,
            },
            crate::hir::SymbolOrigin::User,
            false,
        );

        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(10), s);
        b.emit(Op::Bind(outer), s);
        b.emit(Op::EnterScope, s);
        b.emit(Op::ConstI128(20), s);
        b.emit(Op::Bind(inner), s);
        b.emit(Op::ExitScope, s);
        b.emit(Op::Load(outer), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        execute(&chunk, &symbols, Vec::new()).expect("outer binding should remain");
    }

    #[test]
    fn executes_mod_and_comparison_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(5), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::Mod, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::Eq, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("mod/comparison should execute");
    }

    #[test]
    fn executes_bitwise_and_shift_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstU128(0b1010), s);
        b.emit(Op::ConstU128(0b1100), s);
        b.emit(Op::BitAnd, s);
        b.emit(Op::ConstU128(1), s);
        b.emit(Op::Shl, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("bitwise/shift should execute");
    }

    #[test]
    fn executes_logical_and_not_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstBool(true), s);
        b.emit(Op::ConstBool(false), s);
        b.emit(Op::LogicalOr, s);
        b.emit(Op::Not, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("logical ops should execute");
    }

    #[test]
    fn modulo_by_zero_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(4), s);
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::Mod, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols, Vec::new()).expect_err("mod zero");
        assert!(
            err.iter().any(|d| {
                d.message.contains("modulo by zero") || d.message.contains("integer modulo by zero")
            }),
            "unexpected modulo by zero diagnostics: {:?}",
            err
        );
    }

    #[test]
    fn jump_if_false_uses_truthy_semantics() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::JumpIfFalse(4), s);
        b.emit(Op::ConstI128(99), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("should jump and finish");
    }

    #[test]
    fn invalid_jump_target_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Jump(999), s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols, Vec::new()).expect_err("invalid jump");
        assert!(
            err.iter()
                .any(|d| d.message.contains("invalid jump target"))
        );
    }

    #[test]
    fn string_concat_two_strings() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("hello".to_string()), s);
        b.emit(Op::ConstStr(" world".to_string()), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("concat should execute");
    }

    #[test]
    fn string_concat_string_and_int() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("count: ".to_string()), s);
        b.emit(Op::ConstI128(42), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("concat should execute");
    }

    #[test]
    fn string_concat_int_and_string() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(42), s);
        b.emit(Op::ConstStr(" items".to_string()), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("concat should execute");
    }

    #[test]
    fn string_concat_string_and_bool() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("flag: ".to_string()), s);
        b.emit(Op::ConstBool(true), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("concat should execute");
    }

    #[test]
    fn executes_tuple_build_access_and_eq() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Dup, s);
        b.emit(Op::TupleGet(1), s);
        b.emit(Op::Pop, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Eq, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols, Vec::new()).expect("tuple ops should execute");
    }

    #[test]
    fn builtin_error_produces_err_value_and_field_access() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("boom".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Dup, s);
        b.emit(Op::StructGet("message".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::StructGet("code".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "error".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Err),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        execute(&b.finish(), &symbols, Vec::new()).expect("error builtin should execute");
    }

    #[test]
    fn builtin_panic_aborts_execution() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("fatal".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "panic".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Unit),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        let err = execute(&b.finish(), &symbols, Vec::new()).expect_err("panic must abort");
        assert!(err.iter().any(|d| d.message.contains("panic: fatal")));
    }

    #[test]
    fn try_context_converts_panic_into_err_value() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::TryStart(6), s);
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("fatal".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::TryEnd, s);
        b.emit(Op::Jump(10), s);
        b.emit(Op::ConstNull, s);
        b.emit(Op::Swap, s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);

        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "panic".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Unit),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        execute(&b.finish(), &symbols, Vec::new()).expect("panic should be converted inside try");
    }

    #[test]
    fn builtin_wrap_creates_error_cause_chain() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s); // error
        b.emit(Op::ConstStr("base".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Load(SymbolId(1)), s); // wrap
        b.emit(Op::Swap, s);
        b.emit(Op::ConstStr("ctx".to_string()), s);
        b.emit(Op::CallValue(2), s);
        b.emit(Op::Dup, s);
        b.emit(Op::StructGet("origin".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::StructGet("cause".to_string()), s);
        b.emit(Op::StructGet("message".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);

        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "error".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Err),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        symbols.define(
            root,
            "wrap".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Err),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        execute(&b.finish(), &symbols, Vec::new()).expect("wrap should produce chained err");
    }

    #[test]
    fn executes_array_get_and_set() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let arr = symbols.define(
            root,
            "arr".to_string(),
            crate::analyzer::Type::Array(Box::new(crate::analyzer::Type::Int {
                signed: true,
                bits: 32,
            })),
            crate::hir::SymbolOrigin::User,
            false,
        );
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeArray(2), s);
        b.emit(Op::Bind(arr), s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(9), s);
        b.emit(Op::ArrayAssign(arr), s);
        b.emit(Op::Load(arr), s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ArrayGet, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        execute(&b.finish(), &symbols, Vec::new()).expect("array get/set should execute");
    }

    #[test]
    fn array_get_out_of_bounds_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::MakeArray(1), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::ArrayGet, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        let err = execute(&b.finish(), &symbols, Vec::new()).expect_err("array get oob");
        assert!(
            err.iter()
                .any(|d| d.message.contains("index out of bounds"))
        );
    }

    #[test]
    fn array_extend_opcode_merges_arrays() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::MakeArray(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::MakeArray(2), s);
        b.emit(Op::ArrayExtend, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        execute(&b.finish(), &symbols, Vec::new()).expect("array extend should execute");
    }

    #[test]
    fn builtin_typeof_returns_type_name() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstI128(7), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "typeof".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Str),
            },
            crate::hir::SymbolOrigin::Intrinsic,
            true,
        );
        execute(&b.finish(), &symbols, Vec::new()).expect("typeof should execute");
    }

    #[test]
    fn make_range_opcode_builds_array() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::MakeRange(false), s);
        b.emit(Op::ArrayLen, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        execute(&b.finish(), &symbols, Vec::new()).expect("range should execute");
    }
}
