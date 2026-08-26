# crbsh

`crbsh` is an experimental Unix shell written in Rust. It executes traditional
Unix programs directly while providing a native language for typed values,
functions, control flow, and structured pipelines.

It is not a Bash clone and does not pass user input through Bash, Zsh, Fish, or
`/bin/sh`. The lexer, parser, runtime, and process executor own the language from
source text to child processes.

> `crbsh` is early-stage software. It is useful for development and language
> experimentation, but it is not ready to replace a production login shell.

## Highlights

- Direct external command execution through `$PATH`.
- Text pipelines, redirection, conditional chains, and background jobs.
- Native `string`, `int`, `bool`, `list<T>`, and `record` values.
- Variables, expressions, lexical scopes, functions, loops, and matching.
- List literals, indexing, length inspection, and typed function boundaries.
- Native structured pipelines with automatic Unix text adaptation.
- No third-party Rust dependencies.

## Requirements

- Rust 1.88 or newer.
- Cargo.
- A Unix-like environment.
- `make` only if you want to use the included Makefile targets.

## Build and Run

```sh
cargo build
cargo run
```

After building, the binary is available at `target/debug/crbsh`.

Run a `.crb` script:

```sh
cargo run -- path/to/script.crb
```

Other file extensions are rejected.

Run the complete project gate:

```sh
make ci
```

This runs formatting, compilation, tests, and Clippy with warnings denied. The
equivalent commands are:

```sh
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Workspace Architecture

The repository is a Cargo workspace with two packages:

- `crab-lang` owns tokenization, parsing, language and command syntax,
  native values, lexical scope state, function definitions, and native value
  streams.
- `crbsh` owns the executable shell host: Unix processes, byte-stream adapters,
  redirection, jobs, builtins, environment integration, history, and the REPL.

Structured pipelines cross that boundary explicitly. Native stages transform a
`ValueStream`; the shell converts values to bytes before a Unix process and
decodes process output when a later native stage needs values.

## Install

```sh
make install
```

The default destination is `/usr/local/bin/crbsh`. Set `PREFIX` to install
elsewhere:

```sh
make install PREFIX="$HOME/.local"
make uninstall PREFIX="$HOME/.local"
```

## Unix Commands and Text Pipelines

External programs execute directly and resolve through `$PATH`:

```crb
ls -la
git status
printf 'crab\nfish\n' | grep crab
cat input.txt | grep crab > results.txt
```

Pipeline chains use the previous pipeline's exit status:

```crb
cargo build && print "build passed"
false || print "command failed"
```

Supported process operators are:

- `|` — pipeline
- `&&` — run the next pipeline after success
- `||` — run the next pipeline after failure
- `<` — input redirection
- `>` — output redirection
- `>>` — append redirection
- trailing `&` — background external command or pipeline

## Native Values and Variables

Declare variables with inferred or explicit types:

```crb
let project: string = "crbsh"
let retries: int = 3
let ready: bool = retries < 5

retries = retries + 1
print project retries ready
```

Currently supported native types are `string`, `int`, `bool`, `list<T>`, and
`record`. Typed lists include `list<string>`, `list<int>`, and `list<bool>`.

Lists are homogeneous. Mixed element types are rejected. Empty lists are
accepted, and an explicit annotation preserves their intended element type:

```crb
let names: list<string> = ["Tony", "Alice", "Bob"]
let first = names[0]
let count = names.len
let empty: list<int> = []
```

Indexing requires an integer and checks negative and out-of-bounds indexes.
Index expressions compose normally:

```crb
let answer = [20, 21, 22][1] * 2
```

Index assignment is not implemented.

## Structured Pipelines

A pipeline becomes structured when it contains a native structured command.
Native stages exchange ordered `Value` items without converting them to text.

Available structured commands:

- `values VALUE...` — produce native values. A top-level list expands into
  individual stream items.
- `record KEY VALUE...` — produce one atomic record from key/value pairs.
- `take N` — keep the first `N` items.
- `count` — consume the stream and produce its item count.
- `collect` — consume the stream and produce one list containing all items.

Examples:

```crb
values [1, 2, 3] | take 2
# 1
# 2

record name "Tony" active true | count
# 1

values ["crab", "fish"] | collect
# [crab, fish]
```

Records are atomic stream items. Lists passed to `values` expand by one level;
`collect` deliberately bundles stream items back into a list.

### Unix adapters

External commands are text boundaries. When a structured stream enters an
external Unix command, `crbsh` renders each value as one newline-delimited text
item. If a later native consumer follows, external output is decoded as UTF-8
and each line becomes a native string value:

```crb
values ["crab", "fish"] | grep crab | collect
# [crab]

printf "first
second
" | count
# 2
```

Invalid UTF-8 cannot be adapted back into native values and produces a
stage-specific structured pipeline error.

### Rendering and validation

Final structured output renders one value per line. Final-stage output and
append redirection are supported:

```crb
values [1, 2, 3] | take 2 > numbers.txt
```

Structured pipeline errors name the failing command and its one-based stage.
Producers must start a structured stream, consumers require input, arguments
are validated, and output redirection is only valid on the final stage.

Structured streams are currently ordered and buffered in memory. Lazy,
backpressured value streaming and structured background pipelines are future
work. Stateful builtins are intentionally rejected inside structured pipelines.

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

`for` supports native lists, integer ranges, and single-wildcard file globs:

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

Match patterns support integer, string, and boolean literals plus the `_`
wildcard. Arms are checked in source order, so the first matching arm wins.

Statement matches may be non-exhaustive; no matching arm is a successful no-op:

```crb
match status {
    0 => print "success"
    1 => print "failed"
    _ => print "unknown"
}
```

Match expressions must include a wildcard arm because they must produce a
value:

```crb
let label = match status {
    0 => "success"
    1 => "failure"
    _ => "unknown"
}
```

Matches can be nested, and `return` propagates through nested statement arms.

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

Procedures may use inferred parameters when they do not return a value:

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
startup configuration is loaded from `~/.crbshrc` when that file exists.

Registered builtins are `alias`, `cd`, `exit`, `export`, `fg`, `history`,
`jobs`, `print`, `set`, `unalias`, and `unset`.

## Architecture

```text
source text
    ↓
lexer → Vec<Token>
    ↓
parser → ParsedInput AST
    ↓
runtime evaluator
    ├── values, scopes, functions, and control flow
    └── execution
        ├── direct Unix commands and text pipelines
        ├── structured value pipelines and Unix adapters
        ├── final rendering and redirection
        └── background jobs
```

Responsibilities remain deliberately separate:

- `lexer` recognizes words, literals, operators, quoting, and escaping.
- `parser` owns grammar and AST construction without executing commands.
- `runtime` evaluates native values, lexical scope, functions, and control flow.
- `execution` owns processes, text and structured pipelines, adapters,
  redirection, rendering, and jobs.
- `shell.rs` owns persistent session state.
- `main.rs` wires together the REPL, scripts, configuration, and runtime.

## Source Layout

```text
src/
├── main.rs
├── shell.rs
├── prompt.rs
├── history.rs
├── lexer/
│   ├── mod.rs
│   ├── token.rs
│   └── error.rs
├── parser/
│   ├── mod.rs
│   ├── ast.rs
│   ├── expression.rs
│   ├── statement.rs
│   └── error.rs
├── runtime/
│   ├── mod.rs
│   ├── value.rs
│   ├── scope.rs
│   ├── function.rs
│   └── evaluator.rs
├── execution/
│   ├── mod.rs
│   ├── command.rs
│   ├── pipeline.rs
│   ├── structured.rs
│   ├── render.rs
│   ├── redirect.rs
│   ├── jobs.rs
│   └── error.rs
└── builtins/
    ├── mod.rs
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

## Current Limitations

- `crbsh` is not POSIX-compatible or a drop-in replacement for existing shells.
- Native syntax may change while the language is young.
- Index assignment is not implemented.
- Structured streams are buffered rather than lazy or backpressured.
- Structured pipelines do not run as background jobs.
- Stateful builtins do not participate in pipelines.
- Interactive continuation currently relies primarily on brace balance; a
  richer complete/incomplete/invalid input model remains future work.
