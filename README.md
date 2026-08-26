# crbsh

`crbsh` is an early-stage modern Unix shell written in Rust. It runs traditional
Unix programs without delegating command interpretation to Bash, Zsh, Fish, or
`/bin/sh`, while providing a cleaner native language for typed values, control
flow, functions, and structured expressions.

The implementation is organized around explicit boundaries: the lexer produces
tokens, the parser produces an AST, the runtime evaluates native language
constructs, and the execution layer owns operating-system processes and I/O.
Long-lived interactive state remains in the shell instead of leaking into those
subsystems.

## Current Features

- Interactive REPL with a `crbsh:<cwd>>` prompt.
- `.crb` script execution through `crbsh path/to/script.crb`.
- Optional interactive startup file at `~/.crbshrc`.
- Persistent interactive history stored under:
  - `$XDG_STATE_HOME/crbsh/history`, when `XDG_STATE_HOME` is set.
  - `~/.local/state/crbsh/history`, otherwise.
- External command execution through `$PATH`.
- Native `print` builtin.
- Builtin command registry for `alias`, `cd`, `exit`, `export`, `fg`,
  `history`, `jobs`, `print`, `set`, `unalias`, and `unset`.
- Pipelines with `|`.
- Conditional pipeline chains with `&&` and `||`.
- Input, output, and append redirection with `<`, `>`, and `>>`.
- Background external commands and pipelines with trailing `&`.
- Job inspection and foregrounding with `jobs` and `fg`.
- Aliases for command-position expansion.
- Native variables with `let`, reassignment, and optional type annotations.
- Native `string`, `int`, `bool`, and typed list values.
- List literals, indexing, `.len`, function arguments, and `for` iteration.
- Integer arithmetic and comparisons in expressions.
- Environment overrides through `env.NAME = value`, `export`, and `unset`.
- Control-flow blocks for `if`, `match`, `while`, and `for`.
- Function definitions, typed parameters, return types, nested calls, and
  recursion-depth protection.
- Inferred parameters for procedures; value-returning functions require typed
  parameters and an explicit return type.
- Simple `for` iteration over integer ranges and one-wildcard file globs.

## Prerequisites

- Rust `1.88` or newer.
- Cargo.
- `make`, if using the provided Makefile targets.
- A Unix-like environment for process execution, signals, and job handling.

The crate currently has no third-party Rust dependencies.

## Build, Check, and Test

```sh
cargo build
cargo test
cargo check
cargo clippy --all-targets --all-features
```

The Makefile provides equivalent project targets:

```sh
make build
make test
make check
make lint
make ci
```

`make ci` runs:

```sh
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Run

Start the interactive shell:

```sh
cargo run
```

or:

```sh
make run
```

After building, run the binary directly:

```sh
target/debug/crbsh
```

Run a script file:

```sh
cargo run -- path/to/script.crb
```

Script paths must end in `.crb`; other extensions are rejected.

## Install

Build and install the release binary with the Makefile:

```sh
make install
```

By default this installs to `/usr/local/bin/crbsh`. Override `PREFIX` to choose
another install root:

```sh
make install PREFIX="$HOME/.local"
```

Uninstall from the same prefix:

```sh
make uninstall PREFIX="$HOME/.local"
```

## Usage Examples

Run external commands and pipelines:

```crb
ls -la | grep rs | sort
cat input.txt | grep crab > results.txt
```

Use conditional pipeline chains:

```crb
cargo build && print "build passed"
false || print "command failed"
```

Use native variables and expressions:

```crb
let project: string = "crbsh"
let retries: int = 3
let ready = retries < 5
print project retries ready
```

Set environment overrides for child processes:

```crb
env.RUST_LOG = "debug"
export RUST_LOG = "trace"
unset env.RUST_LOG
```

Define aliases:

```crb
alias p = "print alias"
p tail
unalias p
```

Use control flow:

```crb
let retries = 0

while retries < 3 {
    print retries
    retries = retries + 1
}

match status {
    0 => print "success"
    1 => print "failed"
    _ => print "unknown"
}
```

Define and call functions:

```crb
fn add(a: int, b: int) -> int {
    return a + b
}

let total = add(2, 3)
print total
```

Use typed lists:

```crb
let names: list<string> = ["Tony", "Alice", "Bob"]
let first = names[0]
let count = names.len

for name in names {
    print name
}
```

Run a background job and inspect it:

```crb
sleep 10 &
jobs
fg 1
```

## Architecture

Source moves through a one-way language pipeline:

```text
source text
    ↓
lexer: Vec<Token>
    ↓
parser: ParsedInput AST
    ↓
runtime evaluator
    ├── native values, variables, functions, and control flow
    └── execution layer
          ├── commands and pipelines
          ├── redirection
          └── background jobs
```

The boundaries are intentionally concrete rather than trait-heavy:

- `lexer` knows characters and tokens, but nothing about execution semantics.
- `parser` owns grammar and AST construction, but never executes input.
- `runtime` owns native values, lexical scope, functions, and evaluation.
- `execution` owns child processes, Unix pipes, redirection, and jobs.
- `shell.rs` owns persistent session state such as aliases, environment
  overrides, history, builtin registration, and the last exit code.
- `main.rs` wires together the REPL, scripts, startup configuration, parsing,
  and runtime entrypoint.

## Project Structure

```text
src/
├── main.rs                  # Program entrypoint, REPL, and script/config loading
├── shell.rs                 # Persistent shell session state
├── prompt.rs                # Interactive prompt rendering
├── history.rs               # Persistent interactive history
├── lexer/
│   ├── mod.rs               # Tokenization entrypoint
│   ├── token.rs             # Token definitions
│   └── error.rs             # Tokenization errors
├── parser/
│   ├── mod.rs               # Parser facade
│   ├── ast.rs               # Commands, expressions, statements, and pipelines
│   ├── expression.rs        # Expression precedence parser
│   ├── statement.rs         # Statements, blocks, functions, and pipelines
│   └── error.rs             # Parse errors and formatting
├── runtime/
│   ├── mod.rs               # Runtime facade
│   ├── value.rs             # Native values and type names
│   ├── scope.rs             # Lexical scope stack
│   ├── function.rs          # Function registry and call-depth state
│   └── evaluator.rs         # AST evaluation and native control flow
├── execution/
│   ├── mod.rs               # Execution facade
│   ├── command.rs           # External command construction and builtin print
│   ├── pipeline.rs          # Foreground/background pipeline coordination
│   ├── redirect.rs          # Redirection file handling
│   ├── jobs.rs              # Background job tracking and foregrounding
│   └── error.rs             # Structured execution errors
└── builtins/
    ├── mod.rs           # Builtin result/outcome types
    ├── registry.rs      # Central builtin registry
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

## Status Notes

`crbsh` is not POSIX-compatible and should not be treated as a drop-in
replacement for an existing login shell. The native grammar is intentionally
its own language rather than a partial Bash clone.

Builtins are supported as normal single commands. `print` also has explicit
execution support at the beginning of a pipeline. Other builtins are
intentionally rejected inside pipelines and as background jobs because their
shell-state and streaming semantics have not been defined yet.

The parser already supports multiline block forms, but the interactive input
reader currently continues input based on brace balance. Pipeline continuations
and a richer complete/incomplete/invalid input model remain future work.
