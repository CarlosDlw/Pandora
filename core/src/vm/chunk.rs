use foundation::span::Span;

use crate::vm::bytecode::Op;

/// Runnable fragment: linear bytecode with one span per opcode (debugger / diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub spans: Vec<Span>,
}

impl Chunk {
    #[must_use]
    pub fn invariant_holds(&self) -> bool {
        self.code.len() == self.spans.len()
    }
}

/// Builder-only emission path — never mutate [`Chunk::code`] manually from outside [`ChunkBuilder::emit`].
#[derive(Debug, Default, Clone)]
pub struct ChunkBuilder {
    code: Vec<Op>,
    spans: Vec<Span>,
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

    #[must_use]
    pub fn finish(mut self) -> Chunk {
        debug_assert_eq!(self.code.len(), self.spans.len());
        // Defensive reorder in debug to catch drift.
        Chunk {
            code: std::mem::take(&mut self.code),
            spans: std::mem::take(&mut self.spans),
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
}
