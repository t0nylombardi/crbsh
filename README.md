# crbsh

`crbsh` is a modern Unix shell written in Rust. It is not intended to be a Bash
clone. The current implementation focuses on a small, testable shell core that
can run traditional Unix programs while experimenting with cleaner shell syntax,
typed values, structured parsing, and explicit shell state.

The project is early-stage. The source currently contains an interactive REPL,
script execution for `.crb` files, a tokenizer/parser, builtin dispatch, native
values, aliases, history, jobs, pipelines, redirection, and external process
execution.

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
- Native `string`, `int`, and `bool` values.
- Integer arithmetic and comparisons in expressions.
- Environment overrides through `env.NAME = value`, `export`, and `unset`.
- Control-flow blocks for `if`, `match`, `while`, and `for`.
- Function definitions, typed parameters, optional return types, and `return`.
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

Run a background job and inspect it:

```crb
sleep 10 &
jobs
fg 1
```

## Project Structure

```text
src/
├── main.rs              # Entry point, REPL, script/config loading, evaluation
├── shell.rs             # Long-lived shell state, variables, aliases, functions
├── prompt.rs            # Interactive prompt rendering
├── tokens.rs            # Tokenizer for words, literals, operators, and quotes
├── parser.rs            # Parser and AST types for commands, pipelines, blocks
├── executor.rs          # External process execution, pipelines, redirection
├── history.rs           # Persistent interactive history
├── jobs.rs              # Background job tracking and foregrounding
├── value.rs             # Native value and type definitions
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
replacement for an existing login shell. Builtins are only supported as normal
single commands, with special executor support for `print` at the start of a
pipeline. Other builtins are intentionally rejected in pipeline positions and
as background jobs.

The parser already supports multiline block forms, but the interactive input
reader currently continues input based on brace balance.
