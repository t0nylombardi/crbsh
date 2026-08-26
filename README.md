# crbsh

`crbsh` is an experimental Unix shell with its own typed language, written in
Rust. It runs traditional Unix programs directly while adding native values,
functions, control flow, and structured pipelines.

The project is now a Cargo workspace with a deliberate language/runtime
boundary:

- `crab-lang` is the reusable language library.
- `crbsh` is the Unix shell host and executable.

`crbsh` is not a Bash clone. User input is never delegated to Bash, Zsh, Fish,
or `/bin/sh`; the project owns tokenization, parsing, evaluation, and process
execution itself.

> `crbsh` is early-stage software intended for development and language
> experimentation. It is not ready to replace a production login shell.

## Highlights

- Direct external command execution through `$PATH`.
- Unix text pipelines, redirection, conditional chains, and background jobs.
- Native `string`, `int`, `bool`, `list<T>`, and `record` values.
- Typed variables, expressions, lexical scopes, functions, and recursion.
- `if`/`else`, `while`, `for`, and `match` control flow.
- Native structured pipelines with explicit Unix text adapters.
- Separate `crab-lang` library with no third-party Rust dependencies.

## Requirements

- Rust 1.88 or newer.
- Cargo.
- A Unix-like environment.
- `make` only for the included convenience targets.

## Build and Run

Build the workspace and start the shell:

```sh
cargo build --workspace
cargo run -p crbsh
```

The debug binary is written to `target/debug/crbsh`.

Run a `.crb` script:

```sh
cargo run -p crbsh -- path/to/script.crb
```

Other file extensions are rejected.

Run the complete project gate:

```sh
make ci
```

Equivalent Cargo commands are:

```sh
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Workspace Architecture

```text
source text
    │
    ▼
┌──────────────────────── crab-lang ────────────────────────┐
│ lexer → tokens → parser → AST                             │
│ language values, types, scopes, functions, ValueStream    │
└───────────────────────────┬────────────────────────────────┘
                            │ parsed input and native values
                            ▼
┌────────────────────────── crbsh ───────────────────────────┐
│ evaluator and persistent shell state                      │
│ builtins, aliases, environment, history, startup config   │
│ Unix processes, Stdio, redirection, jobs, REPL, rendering │
└────────────────────────────────────────────────────────────┘
```

### `crab-lang`

The `crates/crab-lang` package owns:

- lexical analysis, quoting, escaping, literals, and operators;
- parsing and AST construction;
- language expressions and control-flow definitions;
- command, redirection, and pipeline syntax;
- `Value` and `TypeName`;
- lexical scope and function-definition state;
- ordered native `ValueStream` transformations.

The Cargo package is named `crab-lang`; Rust code imports it as `crab_lang`.
It remains in this repository so the language boundary can mature alongside its
first real host without cross-repository release choreography.

### `crbsh`

The root package owns host behavior:

- expression and statement orchestration involving shell capabilities;
- direct Unix process execution and `$PATH` lookup;
- byte-stream pipelines and `Stdio` wiring;
- native-value-to-Unix-text adapters;
- redirection, background jobs, and foreground control;
- builtins, aliases, environment integration, and exit status;
- history, startup configuration, prompt, scripts, and the REPL.

The evaluator intentionally remains in `crbsh` while it coordinates host-only
capabilities such as processes, builtins, globs, environment values, and status.
That coupling is explicit instead of being smuggled into the language crate.

## Unix Commands and Text Pipelines

External programs execute directly and resolve through `$PATH`:

```crb
ls -la
git status
printf 'crab\nfish\n' | grep crab
cat input.txt | grep crab > results.txt
```

Pipeline chains use the preceding pipeline's exit status:

```crb
cargo build && print "build passed"
false || print "command failed"
```

Supported process operators:

- `|` — connect pipeline stages.
- `&&` — run the next pipeline after success.
- `||` — run the next pipeline after failure.
- `<` — redirect standard input.
- `>` — redirect standard output.
- `>>` — append standard output.
- trailing `&` — run an external command or pipeline in the background.

## Native Values and Variables

Declare variables with inferred or explicit types:

```crb
let project: string = "crbsh"
let retries: int = 3
let ready: bool = retries < 5

retries = retries + 1
print project retries ready
```

Supported native types are `string`, `int`, `bool`, `list<T>`, and `record`.
Lists are homogeneous; mixed element types are rejected. Explicit annotations
preserve the intended type of an empty list:

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

## Structured Pipelines

A pipeline becomes structured when it contains a native structured command.
Inside native stages, data remains an ordered `ValueStream` instead of being
converted to text.

Available native stages:

- `values VALUE...` — produce native values; a top-level list expands once.
- `record KEY VALUE...` — produce one atomic record.
- `take N` — keep the first `N` values.
- `count` — replace the stream with its item count.
- `collect` — bundle all stream items into one list.

```crb
values [1, 2, 3] | take 2
# 1
# 2

record name "Tony" active true | count
# 1

values ["crab", "fish"] | collect
# [crab, fish]
```

Lists passed to `values` expand by one level. Records remain atomic stream
items, and `collect` deliberately reconstructs a list.

### Unix boundaries

`crab-lang` owns native stream semantics. `crbsh` owns every Unix boundary.
When values enter an external command, `crbsh` renders one newline-delimited
item per value. When external output later enters a native stage, `crbsh`
decodes UTF-8 text and converts each line into a native string:

```crb
values ["crab", "fish"] | grep crab | collect
# [crab]

printf "first
second
" | count
# 2
```

Invalid UTF-8 cannot be adapted back into native values and produces a
stage-specific error. Final structured output renders one value per line and
supports final-stage output redirection:

```crb
values [1, 2, 3] | take 2 > numbers.txt
```

Structured streams are currently buffered in memory. Stateful builtins and
background execution are intentionally unsupported inside structured
pipelines.

## Control Flow

### Conditions and loops

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

`for` supports native lists, integer ranges, and one-wildcard file globs:

```crb
for name in ["Tony", "Alice", "Bob"] {
    print name
}

for number in 1..=3 {
    print number
}

for file in src/*.rs {
    print file
}
```

`break` and `continue` are supported inside loops.

### Match statements and expressions

Patterns support integer, string, and boolean literals plus `_`. Arms are
checked in source order.

Statement matches may be non-exhaustive:

```crb
match status {
    0 => print "success"
    1 => print "failed"
    _ => print "unknown"
}
```

Match expressions require a wildcard arm because they must produce a value:

```crb
let label = match status {
    0 => "success"
    1 => "failure"
    _ => "unknown"
}
```

Matches may be nested, and `return` propagates through nested statement arms.

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

Functions use isolated call scopes, support nested and recursive calls, and
enforce a recursion-depth limit.

## Shell State

Environment overrides are inherited by child processes:

```crb
env.RUST_LOG = "debug"
export RUST_LOG = "trace"
unset env.RUST_LOG
```

Aliases expand only in command position:

```crb
alias p = "print alias"
p tail
unalias p
```

Background jobs can be inspected and foregrounded:

```crb
sleep 10 &
jobs
fg 1
```

Interactive history is stored at `$XDG_STATE_HOME/crbsh/history` when
`XDG_STATE_HOME` is set, or `~/.local/state/crbsh/history` otherwise. Interactive
startup configuration loads from `~/.crbshrc` when present.

Registered builtins are `alias`, `cd`, `exit`, `export`, `fg`, `history`,
`jobs`, `print`, `set`, `unalias`, and `unset`.

## Source Layout

```text
.
├── Cargo.toml                     # workspace and crbsh package
├── crates/
│   └── crab-lang/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── lexer/
│           │   ├── mod.rs
│           │   ├── token.rs
│           │   └── error.rs
│           ├── parser/
│           │   ├── mod.rs
│           │   ├── ast.rs
│           │   ├── command.rs
│           │   ├── language.rs
│           │   ├── expression.rs
│           │   ├── statement.rs
│           │   └── error.rs
│           └── runtime/
│               ├── mod.rs
│               ├── value.rs
│               ├── stream.rs
│               ├── scope.rs
│               ├── state.rs
│               └── function.rs
└── src/
    ├── main.rs
    ├── shell.rs
    ├── prompt.rs
    ├── history.rs
    ├── parser/mod.rs              # crab_lang parser facade
    ├── runtime/
    │   ├── mod.rs                 # crab_lang runtime facade
    │   └── evaluator.rs           # shell-host orchestration
    ├── execution/
    │   ├── command.rs
    │   ├── pipeline.rs
    │   ├── structured.rs          # native/Unix boundary
    │   ├── render.rs
    │   ├── redirect.rs
    │   ├── jobs.rs
    │   └── error.rs
    └── builtins/
        ├── registry.rs
        ├── alias.rs
        ├── cd.rs
        ├── exit.rs
        ├── export.rs
        ├── fg.rs
        ├── history.rs
        ├── jobs.rs
        ├── print.rs
        ├── set.rs
        ├── unalias.rs
        └── unset.rs
```

## Install

```sh
make install
```

The default destination is `/usr/local/bin/crbsh`. Override `PREFIX` when
needed:

```sh
make install PREFIX="$HOME/.local"
make uninstall PREFIX="$HOME/.local"
```

## Current Limitations

- `crbsh` is not POSIX-compatible or a drop-in replacement for existing shells.
- Native syntax may change while the language is young.
- The `crab-lang` public API is still evolving with its first host.
- Index assignment is not implemented.
- Structured streams are buffered rather than lazy or backpressured.
- Structured pipelines cannot run as background jobs.
- Stateful builtins do not participate in pipelines.
- Interactive continuation primarily uses brace balance; a richer
  complete/incomplete/invalid input model remains future work.
