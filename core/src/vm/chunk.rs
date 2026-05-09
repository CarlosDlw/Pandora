use foundation::span::Span;
use std::collections::HashMap;

use crate::hir::symbols::SymbolId;
use crate::vm::bytecode::Op;

/// Runnable fragment: linear bytecode with one span per opcode (debugger / diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub spans: Vec<Span>,
    pub functions: HashMap<SymbolId, FunctionChunk>,
}

impl Chunk {
    #[must_use]
    pub fn invariant_holds(&self) -> bool {
        self.code.len() == self.spans.len()
            && self.functions.values().all(|f| f.chunk.invariant_holds())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionChunk {
    pub params: Vec<SymbolId>,
    pub chunk: Chunk,
}

/// Builder-only emission path — never mutate [`Chunk::code`] manually from outside [`ChunkBuilder::emit`].
#[derive(Debug, Default, Clone)]
pub struct ChunkBuilder {
    code: Vec<Op>,
    spans: Vec<Span>,
    functions: HashMap<SymbolId, FunctionChunk>,
}

impl ChunkBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit(&mut self, op: Op, span: Span) {
        self.code.push(op);
        self.spans.push(span);
        debug_assert_eq!(self.code.len(), self.spans.len());
    }

    pub fn define_function(&mut self, symbol: SymbolId, function: FunctionChunk) {
        self.functions.insert(symbol, function);
    }

    pub fn emit_placeholder_jump_if_false(&mut self, span: Span) -> usize {
        let at = self.len();
        self.emit(Op::JumpIfFalse(usize::MAX), span);
        at
    }

    pub fn emit_placeholder_jump(&mut self, span: Span) -> usize {
        let at = self.len();
        self.emit(Op::Jump(usize::MAX), span);
        at
    }

    pub fn emit_placeholder_try_start(&mut self, span: Span) -> usize {
        let at = self.len();
        self.emit(Op::TryStart(usize::MAX), span);
        at
    }

    pub fn patch_jump_target(&mut self, at: usize, target: usize) -> bool {
        if at >= self.code.len() {
            return false;
        }
        match self.code[at] {
            Op::JumpIfFalse(_) => {
                self.code[at] = Op::JumpIfFalse(target);
                true
            }
            Op::Jump(_) => {
                self.code[at] = Op::Jump(target);
                true
            }
            Op::TryStart(_) => {
                self.code[at] = Op::TryStart(target);
                true
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn finish(mut self) -> Chunk {
        debug_assert_eq!(self.code.len(), self.spans.len());
        // Defensive reorder in debug to catch drift.
        Chunk {
            code: std::mem::take(&mut self.code),
            spans: std::mem::take(&mut self.spans),
            functions: std::mem::take(&mut self.functions),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        debug_assert_eq!(self.code.len(), self.spans.len());
        self.code.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use foundation::{ids::FileId, span::Span};

    use crate::hir::symbols::SymbolId;
    use crate::vm::bytecode::Op;

    use super::ChunkBuilder;

    fn junk_span() -> Span {
        Span::new_unchecked(FileId::from_u32(0), 0, 1)
    }

    #[test]
    fn finish_preserves_parallel_vectors() {
        let mut b = ChunkBuilder::new();
        b.emit(Op::ConstI128(1), junk_span());
        b.emit(Op::Pop, junk_span());
        let chunk = b.finish();
        assert_eq!(chunk.code.len(), chunk.spans.len());
        assert!(chunk.invariant_holds());
    }

    #[test]
    fn empty_chunk_invariant_holds() {
        let chunk = ChunkBuilder::new().finish();
        assert!(chunk.invariant_holds());
    }

    #[test]
    fn call_arities_roundtrip() {
        let mut b = ChunkBuilder::new();
        let s = junk_span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::Call(SymbolId(42), 0), s);
        let chunk = b.finish();
        assert_eq!(chunk.code.len(), 2);
        assert!(matches!(
            chunk.code[1],
            crate::vm::bytecode::Op::Call(SymbolId(42), 0)
        ));
    }

    #[test]
    fn patches_jump_targets() {
        let mut b = ChunkBuilder::new();
        let s = junk_span();
        let jf = b.emit_placeholder_jump_if_false(s);
        let j = b.emit_placeholder_jump(s);
        b.emit(Op::Return, s);
        assert!(b.patch_jump_target(jf, 2));
        assert!(b.patch_jump_target(j, 2));
        let chunk = b.finish();
        assert!(matches!(chunk.code[0], Op::JumpIfFalse(2)));
        assert!(matches!(chunk.code[1], Op::Jump(2)));
    }
}
