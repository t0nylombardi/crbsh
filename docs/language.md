# Crab Language Guide

`crab-lang` is the language library used by `crbsh`. It owns tokenization,
parsing, the AST, native values and types, lexical scope state, function
definitions, and native `ValueStream` transformations.

This guide documents the language currently accepted by `crbsh`. The syntax
and public Rust API are still evolving.

## Values and Types

The language currently supports:

- `string`
- `int`
- `bool`
- `list<T>`
- `record`

Declare variables with inferred or explicit types:

```crb
let project = "crbsh"
let retries: int = 3
let ready: bool = retries < 5

retries = retries + 1
```

A typed variable rejects assignments of a different type. Variables are
lexically scoped, and assignment updates the nearest visible binding.

### Lists

Lists are homogeneous. Mixed element types are rejected:

```crb
let names: list<string> = ["Tony", "Alice", "Bob"]
let first = names[0]
let count = names.len
let empty: list<int> = []
```

Indexes must be integers and are checked for negative and out-of-bounds values.
Index expressions compose with other expressions:

```crb
let answer = [20, 21, 22][1] * 2
```

Index assignment is not implemented.

### Records

Records are currently created through the `record` structured-pipeline stage:

```crb
record name "Tony" active true
# {active: true, name: Tony}
```

Records are atomic values when passed through structured pipelines.

## Expressions

Expressions include literals, identifiers, environment values, status,
function calls, lists, indexing, length access, match expressions, and binary
operators.

Supported arithmetic and comparison operators are:

```text
+  -  *  /
==  !=  <  <=  >  >=
```

Arithmetic operates on integers. Integer overflow and division by zero produce
errors instead of panics. Equality requires matching value types.

### Static Type Checking

`crab-lang::type_checker` can traverse parsed inputs without evaluating them.
Its `TypeChecker` validates lexical declarations and assignments, infers
expression types, applies operator rules, and returns diagnostics containing
expected and found types when both are known. `TypeContext` provides the
lexical type scopes used by the checker.

Function signatures are collected before bodies are checked, so forward and
recursive calls can be validated. The checker verifies typed parameters at
call sites and inside function bodies, enforces declared return types, and
rejects value-returning functions that can fall through. Nested function
signatures follow lexical block scope and do not escape their defining scope.

This is currently a library interface, not an execution gate in `crbsh`.
Ordinary Unix command arguments remain host syntax because a bare command word
and a language identifier currently share one AST representation. Function
calls in command position are checked when their signatures are known.

`status` evaluates to the exit code of the most recently executed command or
pipeline. Environment values use the `env.NAME` namespace:

```crb
let previous = status
let home = env.HOME
```

## Conditions and Loops

```crb
let retries = 0

while retries < 3 {
    print retries
    retries = retries + 1
}

if retries == 3 {
    print "done"
} else {
    print "not done"
}
```

`for` supports lists, integer ranges, and file globs containing one `*` in the
file-name component:

```crb
for name in ["Tony", "Alice", "Bob"] {
    print name
}

for number in 1..3 {
    print number
}

for number in 1..=3 {
    print number
}

for file in src/*.rs {
    print file
}
```

`break` and `continue` are supported inside loops.

## Match

Match patterns support integer, string, and boolean literals, identifiers,
`status`, and the `_` wildcard. Arms are checked in source order.

Statement matches may be non-exhaustive; no match is a successful no-op:

```crb
match status {
    0 => print "success"
    1 => print "failed"
    _ => print "unknown"
}
```

Match expressions must include `_` because they must produce a value:

```crb
let label = match status {
    0 => "success"
    1 => "failure"
    _ => "unknown"
}
```

Matches may be nested. A `return` inside a nested statement arm propagates out
of the function.

## Functions

Value-returning functions require typed parameters and an explicit return type:

```crb
fn add(a: int, b: int) -> int {
    return a + b
}

fn first(items: list<string>) -> string {
    return items[0]
}

let total = add(2, 3)
let name = first(["Tony", "Alice"])
```

Procedures may infer parameter types when they do not return a value:

```crb
fn show(value) {
    print value
}
```

Functions use a fresh local scope while retaining access to global bindings.
Caller-local bindings are not visible inside the function and function locals
do not leak after return. Nested calls and recursion are supported, with a
recursion-depth limit.

## Structured Pipelines

Native structured stages exchange ordered values without converting them to
text:

- `values VALUE...` produces values; a top-level list expands once.
- `record KEY VALUE...` produces one record.
- `take N` keeps the first `N` values.
- `count` replaces the stream with its item count.
- `collect` bundles the stream into one list.

```crb
values [1, 2, 3] | take 2
# 1
# 2

record name "Tony" active true | count
# 1

values ["crab", "fish"] | collect
# [crab, fish]
```

`crab-lang` owns `ValueStream` semantics. The `crbsh` host owns conversion at
Unix process boundaries. Values entering a Unix program render as
newline-delimited text; UTF-8 output entering a later native stage becomes one
string value per line:

```crb
values ["crab", "fish"] | grep crab | collect
# [crab]

printf "first
second
" | count
# 2
```

Invalid UTF-8 cannot be converted back into native values. Structured streams
are currently buffered, stateful builtins cannot participate, and structured
pipelines cannot run in the background.

## Command Syntax

The parser also recognizes the shell command grammar consumed by `crbsh`:

- `|` for pipelines
- `&&` and `||` for conditional pipeline chains
- `<`, `>`, and `>>` for redirection
- trailing `&` for background execution

Quoted or escaped operators remain word content instead of becoming operators:

```crb
print "hello | crab"
print hello\ world
```

Although this syntax is represented by `crab-lang`, process execution,
redirection, job control, environment inheritance, and rendering belong to the
`crbsh` host.

## Current Language Limitations

- Syntax may change while the language is young.
- Index assignment is not implemented.
- File globs support one wildcard in the file-name component.
- Structured streams are buffered rather than lazy or backpressured.
- Interactive completeness detection is currently handled by the shell host.
