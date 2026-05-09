# Pandora Syntax

This document describes the currently implemented language syntax.

## Program Structure

A Pandora program is a sequence of statements.

Supported top-level statements include:
- variable declarations
- function declarations
- struct/trait/impl declarations
- control flow (`if`, `while`, `for`, `for ... in`)
- `import` and `from ... import`
- expression statements

Blocks use braces:

```pandora
{
    print("inside block")
}
```

Semicolons are optional statement separators in many cases.

## Variable Declarations

Pandora supports three declaration forms:

1. Typed mutable declaration:

```pandora
age: i32 = 20
```

2. Type-inferred mutable declaration:

```pandora
name := "John"
```

3. Typed constant declaration:

```pandora
PI:: f32 = 3.14159
```

Assignment after declaration uses `=`:

```pandora
age = age + 1
```

Tuple destructuring is supported:

```pandora
(a, b) := pair
x, y: (i32, i32) = pair
```

## Primitive Types

Primitive type names used by the language include:
- signed integers: `i1`, `i8`, `i16`, `i32`, `i64`, `i128`
- unsigned integers: `u1`, `u8`, `u16`, `u32`, `u64`, `u128`
- floating-point: `f32`, `f64`
- `bool`
- `str`
- `char`
- `unit`
- `null`
- `err`

Composite type syntax includes:
- function types: `fn(i32, i32) -> i32`
- tuple types: `(i32, str)`
- array types: `[i32]`
- map types: `map[str]i32`
- set types: `set[i32]`

## Control Flow

### if / else if / else

```pandora
if condition {
    print("A")
} else if other_condition {
    print("B")
} else {
    print("C")
}
```

### while

```pandora
while i < 10 {
    i = i + 1
}
```

### for (C-style)

```pandora
for i: i32 = 0; i < 10; i++ {
    print(i)
}
```

The init and condition sections can be omitted where supported:

```pandora
j: i32 = 0
for ; j < 3; j++ {
    print(j)
}
```

### for-in

```pandora
for item: i32 in arr {
    print(item)
}

for x in 0..=3 {
    print(x)
}
```

### break / continue / return

```pandora
while true {
    if stop {
        break
    }
    continue
}

fn f() -> unit {
    return
}
```

## Functions

Function declaration syntax:

```pandora
fn add(a: i32, b: i32) -> i32 {
    return a + b
}
```

Function types are first-class in annotations:

```pandora
fn apply(f: fn(i32) -> i32, x: i32) -> i32 {
    return f(x)
}
```

Functions returning no value use `unit`:

```pandora
fn log_value(v: i32) -> unit {
    print(v)
    return
}
```

Optional parameters are supported with default values, and optional parameters must be trailing.

## Scope

Pandora uses block scope.

- Names declared in an outer scope are visible in inner blocks.
- Names declared inside a block are not visible outside that block.

Example:

```pandora
username: str = "salo"

{
    print(username)
    name: str = "carlos"
    print(name)
}

# name is not visible here
```

## Imports (Current Syntax)

Import syntax currently includes:

```pandora
import "std/core" as core
from "std/core" import helper
```

For `import`, alias is required.
