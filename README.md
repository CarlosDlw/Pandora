# Pandora

Pandora is a programming language runtime implemented in Rust.
It parses source code, lowers it, performs semantic/type checking, emits bytecode, and executes on a VM.

## Minimal Example

```pandora
print("Hello, Pandora")
```

## Key Features

- Static type checking (with diagnostics)
- Bytecode-based execution on a VM
- Slot-based struct field access and assignment in VM hot paths
- Direct symbol-target struct field assignment path (no load+assign roundtrip)
- Direct symbol-slot struct field load path to avoid full-struct clone on read
- Short-arity function call fast paths in VM dispatch
- No-capture function call frame setup optimized for parameter binding
- Static call-site direct dispatch opcode (CallDirect)
- Typed integer arithmetic opcode emission for hot binary ops
- Struct field assignment (for example, `state.debug = false`)
- Named struct types inside array annotations (for example, `[ByteOp]`)
- Relational comparisons across integer widths/signs (for example, `i32 < u32`)
- CLI modes for lexing, AST/HIR inspection, checking, and execution
- Standard library modules under `stdlib/std/`

## Run

Using the built binary:

```bash
pandora examples/001_simple.pand
```

Check-only mode:

```bash
pandora examples/001_simple.pand --check
```

## Full Documentation

See full docs in [docs/](docs/).

See all examples in [examples/](examples/).
