# Pandora Runtime

This document describes the current runtime behavior implemented in the VM.

## Memory Model

The VM executes a linear bytecode chunk with explicit runtime state:
- Operand stack (`Vec<Value>`) for expression/intermediate values
- Locals (`Vec<(SymbolId, Value)>`) for bindings in the current frame
- Globals (`HashMap<SymbolId, Value>`) for global/module-level bindings
- Scope frame stack (`Vec<Vec<SymbolId>>`) for lexical scope cleanup
- Call stack (`Vec<CallFrame>`) for function calls
- Try stack (`Vec<TryContext>`) for panic recovery in `try` regions

Runtime symbol resolution in `Load` follows this order:
- current `locals`
- `globals`
- current function self-symbol (for recursion)

On block exit, symbols tracked by the current scope frame are removed from `locals`.

### Closure Capture Semantics (Current Behavior)

Closures capture runtime bindings using a snapshot of current `locals` (`snapshot_locals`) at closure creation time.
This captured map is stored inside `Value::Function { captured, ... }`.

When a closure is called, a new frame is created by cloning captured bindings into call `locals` and then binding call arguments.

Implication: captured mutable state is not shared across calls to the same closure value.
Example pattern:
- counter closure that does `c += 1` returns `1, 1, 1...` (current behavior)

This is value-snapshot closure semantics, not shared-cell/by-reference closure semantics.

There is also a foundation-side indexed model used by shared infrastructure:
- `Arena<T>` stores items in `Vec<T>` and addresses them by stable `ArenaId(u32)`
- `VirtualFileSystem` assigns monotonic `FileId(u32)` values
- Caches are keyed by `FileId` and store `CacheId(u32)`

So the project currently uses two complementary models:
- VM execution state (stack machine + dynamic Rust containers)
- Foundation indexing model (fixed-width IDs over deterministic storage/indexes)

## Stack vs Heap

### Stack-like runtime structures
- Operand evaluation is stack-based (push/pop per opcode)
- Function calls push a call frame and restore it on return
- Scope entry/exit uses scope frame stacks to track and remove local bindings

### Heap-backed value storage
There is no custom VM heap allocator in the runtime module.
Runtime values that need dynamic storage use Rust-owned heap-backed containers:
- `String`
- `Vec<Value>`
- `HashMap<...>`
- `Box<...>`

So the VM is stack-machine based for execution, while dynamic payloads are stored in Rust-managed allocations.

For foundation data, storage is index-based as well:
- arenas are contiguous vectors indexed by `ArenaId`
- VFS/caches are maps keyed by fixed-width IDs (`FileId`, `CacheId`)

## Internal Value Representation

Runtime values are represented by the `Value` enum.
Current variants include:
- Numeric: `Int128`, `UInt128`, `Float`
- Scalar: `Bool`, `Str`, `Char`, `Unit`, `Null`
- Callable: `Builtin`, `Function { function, captured, self_symbol }`
- Collections: `Tuple`, `Array`, `Map`, `Set`
- Error: `Err { message, code, origin, cause }`
- User data: `StructInstance { type_name, fields }`

This representation is also used by printing, comparisons, and intrinsic dispatch.

## How Expression Evaluation Works

Pandora evaluates expressions by compiling HIR expressions to bytecode opcodes, then executing those opcodes in the VM.

### Compile stage (HIR -> bytecode)
- Literals emit constant opcodes
- Unary operations emit operand first, then unary opcode
- Binary operations emit left operand, then right operand, then operation opcode
- Calls emit callee and args, then call opcode
- Collections emit item expressions and constructors (`MakeTuple`, `MakeArray`, `MakeMap`, `MakeSet`)

### Execute stage (bytecode interpreter)
- Most ops pop required operands from the stack
- The VM computes the result and pushes it back
- Control flow uses jump opcodes (`JumpIfFalse`, `Jump`)
- Calls use `Call`/`CallValue`; closures restore execution using call frames
- Errors are returned as diagnostics; panics can be recovered by `try` contexts

### Truthiness in control flow
`JumpIfFalse` uses runtime truthiness rules. Falsy values include:
- `false`
- numeric zero
- empty string
- `\0` character
- `unit`
- `null`
- empty tuple/array/map/set

All other runtime values are treated as truthy.
