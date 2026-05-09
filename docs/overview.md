# Pandora Technical Overview

## Why Pandora Exists

Pandora is a compact language runtime and compiler pipeline implemented in Rust.
Its current design provides an end-to-end path from source code to execution, with diagnostics at each stage.

## Execution Model

Pandora executes code through a staged pipeline:

1. Lexing
2. Parsing to AST
3. Lowering AST to HIR
4. Semantic and type analysis
5. Bytecode emission
6. VM execution

In the CLI entrypoint, diagnostics are accumulated during this pipeline and reported at the end. If there are errors, the process exits with code 1.

The CLI currently supports mode flags for:
- lexeme output
- AST output
- HIR output
- semantic check only

## Type System (Static and Strong)

Pandora performs static type checking before bytecode execution.

The semantic checker validates, among other things:
- assignment compatibility
- function argument and return compatibility
- method-call compatibility
- control-flow constraints (for example, return requirements on non-unit functions)

Type errors are emitted as diagnostics and block successful execution.

## Current Limitations

- The foundation pipeline stages parse, lower, and analyze are not exposed as standalone implemented APIs. The guidance in code is to use Pipeline.run with a PandoraFrontend from core.
- Import statements are validated semantically, but bytecode emission currently treats import and from-import statements as no-op.
- Import resolution is tied to the public stdlib surface (not a general-purpose module loader for arbitrary files).
- The CLI contract is file-oriented and expects a single source file argument.
