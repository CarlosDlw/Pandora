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
- Struct field assignment (for example, `state.debug = false`)
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
