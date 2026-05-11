# Pandora Formal Specification (Current Implementation)

This document defines a formal, implementation-aligned specification for Pandora.
It is intentionally tied to the current parser, analyzer, and bytecode emitter behavior.

## 1. Scope and Notation

- This is a specification of the currently implemented language.
- EBNF below is operational and mirrors accepted syntax classes.
- Typing judgments use the form $\Gamma \vdash e : T$.
- Assignability uses the relation $T_1 \sqsubseteq T_2$ meaning value type $T_2$ is assignable to expected type $T_1$.

## 2. Grammar (EBNF)

### 2.1 Lexical classes

```ebnf
identifier      = letter , { letter | digit | "_" } ;
integer_lit     = ... ;    (* lexer-defined integer forms, including base prefixes *)
float_lit       = ... ;
string_lit      = ... ;
char_lit        = ... ;
bool_lit        = "true" | "false" ;
null_lit        = "null" ;

typename_atom   = identifier | type_name_token | "null" | "Self" ;
```

### 2.2 Program and statements

```ebnf
program         = { statement , [";"] } ;

statement       = struct_decl
                | trait_decl
                | impl_block
                | fn_decl
                | return_stmt
                | while_stmt
                | for_stmt
                | break_stmt
                | continue_stmt
                | if_stmt
                | import_stmt
                | from_import_stmt
                | block_stmt
                | let_decl
                | tuple_destructure_decl
                | compound_assign_stmt
                | assign_stmt
                | expr_stmt ;

block_stmt      = "{" , { statement , [";"] } , "}" ;
expr_stmt       = expr ;

let_decl        = identifier , (":" , type_ref | "::" , type_ref | ":=") , "=" , expr
                | identifier , ":=" , expr ;

tuple_destructure_decl
                = tuple_name_list , (":=" | ":" , type_ref , "=") , expr ;

tuple_name_list = "(" , identifier , "," , identifier , { "," , identifier } , ")"
                | identifier , "," , identifier , { "," , identifier } ;

assign_stmt     = assign_target , "=" , expr ;
assign_target   = identifier , { ("[" , expr , "]") | ("." , identifier) } ;

compound_assign_stmt
                = identifier , ("+=" | "-=" | "*=" | "/=" | "%=" | "**="
                | "&=" | "|=" | "^=" | "<<=" | ">>=") , expr ;

if_stmt         = "if" , expr , block_stmt , [ "else" , (if_stmt | block_stmt) ] ;

while_stmt      = "while" , expr , block_stmt ;

for_stmt        = "for" , (for_in_header | c_for_header) , block_stmt ;
for_in_header   = identifier , [":" , type_ref] , "in" , expr ;
c_for_header    = [for_init_decl] , ";" , [expr] , ";" , [expr] ;
for_init_decl   = identifier , ":" , type_ref , "=" , expr ;

break_stmt      = "break" ;
continue_stmt   = "continue" ;

return_stmt     = "return" , [ expr , { "," , expr } ] ;

import_stmt     = "import" , string_lit , "as" , identifier ;
from_import_stmt= "from" , string_lit , "import" , identifier , { "," , identifier } ;
```

### 2.3 Declarations

```ebnf
fn_decl         = "fn" , identifier , "(" , [fn_params] , ")" , "->" , type_ref , block_stmt ;

fn_params       = fn_param , { "," , fn_param } ;
fn_param        = "self"
                | identifier , ":" , type_ref , ["=" , expr] ;

struct_decl     = "struct" , identifier , "{" , [struct_fields] , "}" ;
struct_fields   = struct_field , { "," , struct_field } ;
struct_field    = identifier , ":" , type_ref ;

trait_decl      = "trait" , identifier , "{" , { trait_method_sig , [";"] } , "}" ;
trait_method_sig= "fn" , identifier , "(" , [fn_params] , ")" , "->" , type_ref ;

impl_block      = "impl" , type_ref , ["for" , type_ref] , "{" , { fn_decl , [";"] } , "}" ;
```

### 2.4 Types

```ebnf
type_ref        = fn_type
                | tuple_type
                | array_type
                | map_type
                | set_type
                | typename_atom ;

fn_type         = "fn" , "(" , [type_ref , { "," , type_ref }] , ")" , "->" , type_ref ;
tuple_type      = "(" , type_ref , "," , type_ref , { "," , type_ref } , ")" ;
array_type      = "[" , type_ref , "]" ;
map_type        = "map" , "[" , type_ref , "]" , type_ref ;
set_type        = "set" , "[" , type_ref , "]" ;
```

### 2.5 Expressions and precedence

The parser is Pratt-based with precedence (low to high):
1. range (`..`, `..=`)
2. logical or (`||`)
3. logical and (`&&`)
4. bitwise or (`|`)
5. bitwise xor (`^`)
6. bitwise and (`&`)
7. equality (`==`, `!=`)
8. comparison (`<`, `<=`, `>`, `>=`)
9. shift (`<<`, `>>`)
10. sum (`+`, `-`)
11. product (`*`, `/`, `%`)
12. power (`**`, right-associative)
13. postfix/call/member/index/inc-dec/propagate

```ebnf
expr            = range_expr ;
range_expr      = logic_or_expr , [ (".." | "..=") , range_expr ] ;

logic_or_expr   = logic_and_expr , { "||" , logic_and_expr } ;
logic_and_expr  = bitor_expr , { "&&" , bitor_expr } ;
bitor_expr      = bitxor_expr , { "|" , bitxor_expr } ;
bitxor_expr     = bitand_expr , { "^" , bitand_expr } ;
bitand_expr     = eq_expr , { "&" , eq_expr } ;
eq_expr         = cmp_expr , { ("==" | "!=") , cmp_expr } ;
cmp_expr        = shift_expr , { ("<" | "<=" | ">" | ">=") , shift_expr } ;
shift_expr      = sum_expr , { ("<<" | ">>") , sum_expr } ;
sum_expr        = prod_expr , { ("+" | "-") , prod_expr } ;
prod_expr       = pow_expr , { ("*" | "/" | "%") , pow_expr } ;
pow_expr        = postfix_expr , [ "**" , pow_expr ] ;

postfix_expr    = prefix_expr , { postfix_op } ;
postfix_op      = "(" , [arg_list] , ")"
                | "." , (identifier | integer_lit)
                | "[" , expr , "]"
                | "++"
                | "--"
                | "?" ;

prefix_expr     = primary
                | "-" , prefix_expr
                | "!" , prefix_expr
                | "~" , prefix_expr
                | "++" , prefix_expr
                | "--" , prefix_expr ;

primary         = identifier
                | "self"
                | integer_lit
                | float_lit
                | string_lit
                | char_lit
                | bool_lit
                | null_lit
                | try_catch_expr
                | tuple_or_group
                | array_lit
                | map_lit
                | set_lit
                | struct_lit
                | static_method_call ;

arg_list        = expr , { "," , expr } ;

tuple_or_group  = "(" , expr , ( "," , expr , { "," , expr } , ")" | ")" ) ;

array_lit       = "[" , [ array_item , { "," , array_item } ] , "]" ;
array_item      = expr | "..." , expr ;

map_lit         = "{" , [ map_entry , { "," , map_entry } ] , "}" ;
map_entry       = expr , ":" , expr ;

set_lit         = "set" , "{" , [ expr , { "," , expr } ] , "}" ;

struct_lit      = identifier , "{" , [ struct_lit_field , { "," , struct_lit_field } ] , "}" ;
struct_lit_field= identifier , ":" , expr ;

static_method_call
                = identifier , "::" , identifier , "(" , [arg_list] , ")" ;

try_catch_expr  = "try" , expr , "catch" , "(" , identifier , ":" , type_ref , ")" , block_stmt ;
```

## 3. Formal Typing Rules

### 3.1 Core domains

Let:
- $\Gamma$ be the symbol environment.
- Types $T$ include primitives, tuples, arrays, maps, sets, functions, structs, traits, and special forms `Unknown`, `Any`, `Null`, `Err`.

### 3.2 Assignability

The checker uses a structural assignability relation:

$$
T_e \sqsubseteq T_a \iff
\left(
T_e = T_a
\right)
\lor
\left(
T_e \in \{\text{Unknown}, \text{Any}\}
\right)
\lor
\left(
T_a \in \{\text{Unknown}, \text{Null}\}
\right)
\lor
\text{structural}(T_e, T_a)
$$

where structural is recursive for tuple/array/map/set with same outer constructor and compatible inner positions.

### 3.3 Selected expression rules

Variable:
$$
\frac{\Gamma(x)=T}{\Gamma \vdash x : T}
$$

Assignment compatibility:
$$
\frac{\Gamma \vdash e : T_a \quad T_e = \Gamma(x) \quad T_e \sqsubseteq T_a}{\Gamma \vdash x=e : T_e}
$$

Binary numeric operators (`-`, `*`, `/`, `%`, `**`):
- both operands must be numeric and same numeric family/shape (`Int` same sign+width or `Float` same width).
- result type is operand type.

Add (`+`):
- if either operand is `str`, result is `str`.
- otherwise follows numeric rule above.

Logical (`&&`, `||`):
- both operands must be `bool`.
- result is `bool`.

Comparison (`<`, `<=`, `>`, `>=`):
- numeric compatibility required as above.
- result is `bool`.

Equality (`==`, `!=`):
- accepted when either side assignable to the other.
- result is `bool`.

### 3.4 Control flow typing constraints

- `if`, `while`, `for` conditions must be truthy/falsy-compatible at semantic level:
  `bool`, numeric, `str`, `char`.
- `break` and `continue` are valid only inside loop context.

### 3.5 Function and return constraints

- Non-`unit` functions must return on all paths.
- `return` outside function is invalid.
- Empty `return` requires declared return type `unit`.
- Multiple return values are valid only for functions returning tuple types.

### 3.6 Collections

- Empty array/map/set literals require explicit expected type context.
- Map/set key/item hashability constraint:
  allowed key-like types are numeric, bool, str, char, Unknown.

### 3.7 Error system typing

Special builtins:
- `error(str, [i32]) : err`
- `panic(str, [i32]) : unit`
- `wrap(err_like, str, [i32]) : err`

Err-like type predicate:
- `err`, or
- struct containing at least fields `message: str` and `code: i32`.

Propagation operator `?`:
- operand must be `(T, E)` with `E` err-like.
- enclosing function return type must be `(R, E_f)` with `E_f` err-like.
- propagated error type must be assignable to function error type.
- expression type of `e?` is `T`.

Try/catch expression:
- `try e catch(x: Ex) { ... return v }` expects `e : (T, E)` with err-like `E`.
- catch binding type must be err-like (or Unknown).
- catch value type must be assignable to `T`.
- whole expression type is `T`.

## 4. Order of Evaluation

Evaluation is deterministic and left-to-right in bytecode emission.

### 4.1 Expression order

- Binary: emit/evaluate left operand, then right operand, then operator.
- Call: evaluate callee first, then arguments in source order.
- Method call: evaluate receiver before explicit args.
- Index access: evaluate container, then index.
- Literals with multiple elements (`tuple`, `array`, `map`, `set`): evaluate elements/entries in source order.

### 4.2 Statement order

- Program statements run in source order.
- Block statements run in source order with explicit scope enter/exit.
- `if`: condition first, then selected branch.
- `while`: condition checked before each iteration.
- `for`: init, then condition, then body, then step, then repeat.

### 4.3 Boolean operators

Current implementation emits both operands for `&&` and `||` before applying operator opcode.
This means no short-circuit at emission level for logical binary expressions.

### 4.4 `?` runtime flow

For `e?`:
1. evaluate `e` to `(ok, err)` tuple,
2. if `err != null`, wrap error context and return early from current function,
3. else yield `ok`.

### 4.5 `try/catch` runtime flow

`try` sets a panic handler context.
Panics inside `try` are converted into a fallible tuple path and then processed by catch logic.
Catch path executes only when error slot is non-null.
Success path extracts tuple success value.

## 5. Edge Cases (Normative)

### 5.1 Syntax and parsing

- `import` without alias is invalid (`import requires alias`).
- Tuple type must have at least two elements.
- Missing block terminator `}` is a parse error.
- `else` without matching `if` is invalid.
- For-init in C-style for-loop must be typed declaration (`name: type = value`).

### 5.2 Typing and semantics

- Assignment to constant is invalid.
- Array index type must be integer.
- Tuple index access out of range is invalid.
- Tuple destructuring arity mismatch is invalid.
- Function argument count and type mismatches are invalid.
- Return arity/type mismatches are invalid.
- Internal intrinsics are not callable through public source API.

### 5.3 Runtime

- division by zero and modulo by zero produce runtime diagnostics.
- array index out-of-bounds produces runtime diagnostic.
- calling a non-function value produces runtime diagnostic.
- explicit `panic(...)` outside recovery path terminates execution with diagnostic.
- runtime symbol resolution in `Load` checks: current locals, then globals, then current function self-symbol (recursion case).
- lexical scopes remove local bindings on scope exit (`EnterScope` / `ExitScope`).
- closures capture a snapshot of current locals at creation time.
- closure invocation clones captured bindings into the callee frame before binding call arguments.
- captured mutable bindings are not shared across calls to the same closure value (snapshot/value semantics).

### 5.4 Type-literal boundaries

- integer literal must fit target integer type when expected context exists.
- float literal must fit expected float width (`f32`/`f64`).
- invalid char literal forms are rejected.
