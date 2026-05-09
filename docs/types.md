# Pandora Type System

This document describes the type system behavior implemented by the current semantic checker.

## Primitive Types

Pandora supports these primitive categories:
- Integers: signed and unsigned (`i*`, `u*` families)
- Floats (`f32`, `f64`)
- `bool`
- `str`
- `char`
- `unit`
- `null`
- `err`

Internally, integer and float types are represented with bit width metadata.

## Composite Types

Implemented composite types include:
- Function types: `fn(T1, T2, ...) -> R`
- Tuples: `(T1, T2, ...)`
- Arrays: `[T]`
- Maps: `map[K]V`
- Sets: `set[T]`
- Struct types
- Trait types

Examples:

```pandora
values: [i32] = [1, 2, 3]
ops: [ByteOp] = [create_op(1, 7, 0)]
pair: (i32, str) = (1, "x")
lookup: map[str]i32 = {"a": 1}
items: set[i32] = set{1, 2, 3}
```

## Type Inference

Type inference is available in common places:

- Variable declarations with `:=`

```pandora
name := "John"
count := 42
```

- Collection literals infer element/key/value types from context or from first items.

Important current behavior:
- Empty array literals require explicit array type context.
- Empty map literals require explicit map type context.
- Empty set literals require explicit set type context.

## Compatibility Rules

Compatibility is checked statically through assignability rules.

At a high level:
- Exact type match is assignable.
- `Unknown` and `Any` act as permissive expected types.
- `null` is accepted where an actual value is checked against an expected type.
- Composite types are checked recursively:
  - tuple arity and each position
  - array item type
  - map key/value types
  - set item type

For non-matching unrelated types, assignment/call compatibility fails.

For relational comparisons (`<`, `<=`, `>`, `>=`):
- Integer operands can use different widths/signs (`i32` vs `u32`, etc.).
- Float operands must still use matching width (`f32` with `f32`, `f64` with `f64`).

## Casting and Conversion

There is no dedicated general cast operator documented by the parser/checker.

Numeric/string/char/bool conversions are currently exposed through methods (for example, `to_i32()`, `to_u32()`, `to_f64()`, `to_str()`) as shown in examples.

So, conversion is method-driven rather than an explicit cast syntax.

## Type Errors (When They Occur)

Type errors are emitted during semantic analysis (before bytecode execution). Common cases include:

- Assignment mismatch
- Constant reassignment attempts
- Function argument type mismatch
- Return type mismatch
- Tuple destructuring mismatch (type or arity)
- Array/map/set literal item mismatch
- Invalid array index type
- Invalid operation over incompatible operand types

Representative diagnostics include messages such as:
- `cannot assign value of type ...`
- `invalid argument type at position ...`
- `return type mismatch: expected ..., got ...`
- `tuple destructuring arity mismatch ...`
- `array literal item type mismatch ...`

If semantic diagnostics contain errors, execution is blocked.

## Notes on Literal Checking

Numeric literal fitting is validated against expected target widths in typed contexts.
This includes integer width checks and float range checks (`f32`/`f64`).
