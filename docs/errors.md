# Pandora Errors and Diagnostics

This document describes the current, implemented error and diagnostics system.
All statements below are based on code paths and tests in this repository.

## 1. Error Types by Compilation/Execution Stage

Pandora emits diagnostics across the full pipeline:

1. Lexing errors
2. Parsing errors
3. Lowering errors
4. Semantic/type errors (analyzer)
5. Bytecode compilation errors
6. Runtime VM errors

Pipeline behavior is fail-fast by stage in the core driver:
- lexer diagnostics collected first
- parser diagnostics appended
- if errors exist, pipeline stops
- same rule repeats for lowering, analyzer, bytecode compile
- runtime diagnostics are appended only if earlier stages pass

## 2. Severity Model

Diagnostics currently use:
- `Error`
- `Warning`

The CLI output renderer maps these to text labels:
- `error`
- `warning`

In practice, the current error paths are mostly `error` diagnostics.

## 3. Diagnostic Message Format (CLI)

CLI renders diagnostics in this structure:

1. `error: <message>` (or `warning: <message>`)
2. source location line with path, line, column, and byte range
3. source line snippet
4. caret highlight (`^`)
5. optional help hint (`= help: ...`) when message matches known patterns

Current renderer format:

```text
error: expected ')'
  --> file.pand:1:6 [5..6]
   |
  1 | a := (
   |      ^
   = help: close the missing ')' at the end of the expression or call.
```

If span mapping fails, renderer falls back to:

```text
  --> file.pand:? [?] [start..end]
```

## 4. Language Error System (`err`)

Pandora has a language-level error value and explicit propagation/recovery constructs.

### 4.1 Builtins

#### `error(message: str, code: i32 = 1) -> err`
- Analyzer enforces message type `str`
- Analyzer enforces code type `i32` when provided
- Returns `err`

#### `panic(message: str, code: i32 = 1) -> unit`
- Analyzer validates argument types
- Runtime raises a diagnostic with format: `panic: <message> (code=<n>)`
- Outside `try`, this is a runtime failure
- Inside `try`, panic can be converted to an `err` for catch flow

#### `wrap(base_err, message: str, code?: i32) -> err`
- Analyzer enforces first argument is err-like
- Analyzer enforces message/code types
- Runtime wraps cause/context into a new `err`

### 4.2 `err` runtime shape

Runtime `err` values include:
- `message: str`
- `code: i32`
- `origin: str`
- `cause: err | null`

Observed in examples/tests (printed form):

```text
err(message="test", code=1, origin="error")
```

### 4.3 Err-like custom structs

Analyzer accepts custom struct error types as err-like when they contain:
- `message: str`
- `code: i32`

This is used by `try/catch` and `wrap` checks.

## 5. Propagation and Recovery Semantics

### `?` operator

`expr?` is only valid when:
- `expr` has type `(T, err-like)`
- current function return type is `(R, err-like)`
- error type from `expr` is assignable to function error type

If error tuple position is non-null, runtime wraps with message `propagated by ?` and returns early.

### `try ... catch`

`try` expression expects `(T, err-like)`.
Catch binding must be err-like or unknown.
Catch result must match success value type `T`.

This applies to both regular `err` values and recovered panics.

## 6. Runtime Error Classes

Examples of runtime diagnostics currently emitted by VM:

- `division by zero`
- `modulo by zero`
- `index out of bounds: index=..., len=...`
- `tuple index out of range`
- `attempted call on non-function value: ...`
- `invalid operands for arithmetic/comparison/bitwise/...`
- `stack underflow (internal bytecode error)`
- `panic: <message> (code=<n>)`

## 7. Real Diagnostic Examples (from test suite)

### Parse/Lex examples

- `error: invalid character`
- `error: expected ')'`
- `error: expected '}'`
- `error: import requires alias`

### Semantic/type examples

- `error: undefined symbol 'name'`
- `error: return used outside of function`
- `error: return type mismatch: expected ..., got ...`
- `error: invalid argument type at position ...`
- `error: tuple destructuring arity mismatch: pattern has ..., value has ...`
- `error: internal intrinsic 'fs_create_dir' is not part of the public stdlib API`

### Runtime examples

- `panic: unrecoverable (code=42)`
- `division by zero`
- `index out of bounds: index=5, len=2`

## 8. Help Hints in CLI

The renderer includes contextual hints for known message patterns.
Examples of hint-enabled errors:

- missing `)`
- missing `}`
- undefined symbol
- invalid numeric literal
- invalid truthy/falsy condition
- misuse of `?`
- catch typing mismatches
- return type mismatch

Hints are pattern-based and optional: unknown/custom messages do not receive `= help:`.

## 9. Sources of Truth

Implementation sources:
- CLI formatter: `cmdline/src/diagnostic_renderer.rs`
- Pipeline stage behavior: `core/src/driver.rs`
- Diagnostics types: `foundation/src/diagnostics.rs`
- Semantic checks and error contracts: `core/src/analyzer/checker.rs`
- Runtime diagnostics and panic recovery: `core/src/vm/engine.rs`

Behavioral examples:
- Diagnostics tests: `cmdline/tests/cli_diagnostics.rs`
- Runtime/error examples: `examples/019_*.pand` through `examples/024_*.pand`
- Runtime mode tests: `cmdline/tests/cli_modes.rs`
