# Pandora CLI Interface

This page documents the current interpreter CLI contract implemented by the `pandora` binary.

## Commands

Pandora CLI is file-oriented and does not expose subcommands.
The command shape is:

```bash
pandora <file.pand> [mode-flag]
```

Examples:

```bash
pandora examples/001_simple.pand
pandora examples/001_simple.pand --check
pandora examples/001_simple.pand --ast
pandora examples/001_simple.pand --bytecode
```

Help output is available in two ways:

```bash
pandora --help
pandora help
```

## Flags

Mode flags are mutually exclusive (only one can be used at a time):

- `--lexeme`
  - Runs lexing and prints lexer tokens.
  - Does not continue into parse/lower/analyze/execute.
- `--ast`
  - Runs lexing + parsing and prints AST roots.
  - Does not continue into lowering/analyzer/execute.
- `--hir`
  - Runs lexing + parsing + lowering and prints HIR statements and expression arena entries.
  - Does not continue into analyzer/execute.
- `--check`
  - Runs lexing + parsing + lowering + semantic/type analysis.
  - Reports diagnostics only; does not compile bytecode or execute VM.
- `--bytecode`
  - Runs lexing + parsing + lowering + semantic/type analysis + bytecode compilation.
  - Prints the generated VM chunk for `<main>` and function chunks.
  - Does not execute VM code.

Default mode (no mode flag):
- Runs full pipeline: lex -> parse -> lower -> analyze -> bytecode compile -> VM execute.

If more than one mode flag is provided together (for example `--lexeme --ast`), argument parsing fails as a usage error.

## Input and Output

### Input

- Required positional argument: source file path (`<file.pand>`).
- Source is read from filesystem as text.
- If file read fails, CLI prints:

```text
failed to read '<path>': <io error>
```

### Standard Output (`stdout`)

- Mode dumps (`--lexeme`, `--ast`, `--hir`) are printed to `stdout`.
- Program output from language-level `print(...)` in default execution mode is printed to `stdout`.
- Help text is printed to `stdout`.

### Standard Error (`stderr`)

- Diagnostics are printed to `stderr`.
- Diagnostic renderer format is:

```text
error: <message>
  --> <path>:<line>:<column> [<start>..<end>]
   |
 <n> | <source line>
   | <carets>
   = help: <optional hint>
```

- The optional help hint is emitted only when the message matches known patterns.

### Exit Codes

- `0`: successful execution / successful mode run with no error diagnostics.
- `1`: at least one diagnostic with severity `error`, runtime failure, or file-read/help-print failure.
- `2`: CLI usage/argument parsing error (for example, incompatible mode flags).
