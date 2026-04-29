# Pandora Stdlib Implementation Guide

This guide is the concrete reference for adding or evolving stdlib modules in Pandora without getting lost in the pipeline.

---

## 1) Big Picture: Where stdlib features live

When adding a new stdlib module (for example `std/time`, `std/net`, `std/collections`), changes usually touch **all** these layers:

1. **Public stdlib API (Pandora code)**
   - `stdlib/std/<module>.pand`
2. **Builtin function registration (semantic signatures)**
   - `core/src/builtins/definitions.rs`
3. **Runtime dispatch (native implementation)**
   - `core/src/vm/engine.rs`
4. **Semantic guard for internal intrinsics**
   - `core/src/analyzer/checker.rs`
5. **Examples**
   - `examples/0xx_*.pand`
6. **CLI integration tests**
   - `cmdline/tests/cli_modes.rs`
   - `cmdline/tests/cli_diagnostics.rs`

If one layer is missing, behavior will be inconsistent (parse/check/runtime/test mismatches).

---

## 2) Current syntax and conventions used in this project

### 2.1 Imports

- Alias import:
  - `import "std/core" as core`
- Granular import:
  - `from "std/math" import pi, sqrt, rand_i32`

### 2.2 Error-return style

Pandora stdlib follows explicit value+error contracts:

- `(..., err)` for fallible reads/conversions.
- `err` (nullable) for operations like write/remove/sleep.
- `null` indicates success in `err` position.

### 2.3 Internal vs public API

Internal runtime intrinsics use prefixes and are **not** public API:

- `io_*`
- `fs_*`
- `math_*`
- `time_*`

User code should call only public names exposed by stdlib wrappers (`read_text`, `path_join`, `sqrt`, `now_unix_millis`, etc.).

### 2.4 Numeric typing gotchas

Pandora is strict with integer widths/signs in comparisons.
If needed, use typed values or type methods to align:

- `u64` values compare safely with other `u64`.
- For method-based compare: `u64_value.gt((0).to_u32())` (when method contract expects that type path).

---

## 3) File-by-file map (what to edit and why)

###[core/src/builtins/definitions.rs](/home/carlos/Projects/Rust/Pandora/core/src/builtins/definitions.rs)

Purpose:
- Registers every builtin function signature seen by lowering/analyzer.
- Must include:
  1. internal intrinsic names (e.g. `time_now_unix_millis`)
  2. public aliases/wrappers (e.g. `now_unix_millis`)

Checklist:
- Add function name
- Add `Type::Function { params, ret }` signature
- Keep signatures consistent with runtime behavior and `.pand` wrappers

Common mistakes:
- Missing public alias -> `from "std/..." import x` compiles but call fails.
- Wrong tuple signature -> checker/runtime mismatch.

---

###[core/src/vm/engine.rs](/home/carlos/Projects/Rust/Pandora/core/src/vm/engine.rs)

Purpose:
- Native execution for builtin functions in `dispatch_builtin`.

Pattern:
- Use match arms with aliases, e.g.:
  - `"time_now_unix_millis" | "now_unix_millis" => { ... }`
- Validate arity and types
- Return `Diagnostic` only for contract misuse (arity/type), and `err` values for recoverable operation failures where API expects it.

Recommended helpers:
- typed argument extractors (`expect_str_arg`, `expect_u64_arg`, etc.)
- small conversion/format helpers for non-trivial logic

Common mistakes:
- Only implementing `time_*` arm but not public alias.
- Returning panic/error style inconsistent with declared signature.
- Exposing internal-only behavior by skipping checker guard.

---

###[core/src/analyzer/checker.rs](/home/carlos/Projects/Rust/Pandora/core/src/analyzer/checker.rs)

Purpose:
- Type checks calls and enforces API boundaries.

Critical section:
- `check_special_builtin_contract(...)`
- Internal intrinsic blocking rule:
  - deny direct calls to `io_*`, `fs_*`, `math_*`, `time_*`.

Why this matters:
- Prevents users from bypassing stdlib wrappers and calling raw runtime functions directly.

---

###[stdlib/std/<module>.pand](/home/carlos/Projects/Rust/Pandora/stdlib/std)

Purpose:
- Public stdlib surface that users import.
- Should be clean, documented, and stable.

Rule:
- Public function names call internal intrinsics.
- Keep return contracts explicit and predictable.

Example style:
- `fn now_unix_millis() -> u64 { return time_now_unix_millis() }`
- `fn sleep_ms(ms: u64) -> err { return time_sleep_ms(ms) }`

---

###[examples](/home/carlos/Projects/Rust/Pandora/examples)

Purpose:
- End-to-end executable docs.
- Must demonstrate real usage from public imports only.

Current sequence:
- `046_std_core_foundation.pand`
- `047_std_io_basics.pand`
- `048_std_fs_basics.pand`
- `049_std_math_basics.pand`
- `050_std_time_basics.pand`

Rules:
- Prefer stable assertions by properties, not fragile exact runtime values for time/random.
- Avoid using internal intrinsics directly in examples.

---

###[cmdline/tests/cli_modes.rs](/home/carlos/Projects/Rust/Pandora/cmdline/tests/cli_modes.rs)

Purpose:
- Integration tests for successful execution and expected output.

Pattern:
- Add `runs_example_0xx_*` test.
- Use chained `contains(...)` predicates.
- For nondeterministic values (time/random), assert boolean properties printed by example.

---

###[cmdline/tests/cli_diagnostics.rs](/home/carlos/Projects/Rust/Pandora/cmdline/tests/cli_diagnostics.rs)

Purpose:
- Integration tests for static/check diagnostics.

Always add:
- At least one invalid type/arity test for new module API.
- At least one test proving internal intrinsic is blocked.

Examples:
- `sleep_ms("10")` -> invalid argument type
- `time_now_unix_secs()` -> internal intrinsic forbidden

---

## 4) Golden workflow for new stdlib modules

1. Define API in `stdlib/std/<module>.pand` (public names first).
2. Add signatures in `core/src/builtins/definitions.rs` for:
   - internal `module_*`
   - public aliases
3. Implement runtime in `core/src/vm/engine.rs` with alias match arms.
4. Update checker guard in `core/src/analyzer/checker.rs` for `module_*`.
5. Add example in `examples/`.
6. Add/adjust `cli_modes` test.
7. Add/adjust `cli_diagnostics` test.
8. Run tests and fix regressions.
9. Run lints on touched files.

---

## 5) Test commands to always run

From repo root:

- `cargo test --test cli_modes`
- `cargo test --test cli_diagnostics`

From `cmdline/`:

- `cargo test --test cli_modes`
- `cargo test --test cli_diagnostics`

Targeted test:

- `cargo test runs_example_0xx_name`

---

## 6) Quality checklist before finishing a stdlib feature

- [ ] Public API exists in `stdlib/std/<module>.pand`
- [ ] Internal intrinsics are blocked in checker
- [ ] Runtime implements both internal and public aliases
- [ ] Signatures in registry match runtime returns exactly
- [ ] Example uses public imports only
- [ ] `cli_modes` includes new example test
- [ ] `cli_diagnostics` includes invalid-type + internal-intrinsic test
- [ ] Full `cli_modes` and `cli_diagnostics` suites are green
- [ ] No lints in modified files

---

## 7) Common pitfalls already seen in this project

1. **Function exists in stdlib file but not in registry**
   - Symptom: “attempted call on non-function value”.
2. **Public alias missing in VM dispatch**
   - Symptom: compile passes, runtime fails unknown builtin.
3. **Using reserved/semantic-sensitive names as variables**
   - Example: `err` as identifier can conflict with type semantics.
4. **Numeric literal typing mismatch in comparisons**
   - `u64` vs default `i32` comparisons fail in checker.
5. **Multiline expression formatting not accepted by parser shape**
   - Prefer simpler one-line function calls when parser is strict.

---

## 8) Minimal template for next stdlib module

1. Create `stdlib/std/<new>.pand`
2. Register:
   - `<new>_internal_name`
   - `public_name`
3. Implement in VM:
   - `"internal" | "public" => { ... }`
4. Guard checker:
   - `name.starts_with("<new>_")`
5. Add:
   - `examples/0xx_std_<new>_basics.pand`
   - `runs_example_0xx_std_<new>_basics`
   - 2 diagnostics tests (bad arg + forbidden internal)

This is the standard, repeatable path.
