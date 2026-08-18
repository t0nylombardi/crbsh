# AGENTS.md

## Project Overview

`crbsh` is a modern Unix shell written in Rust.

The goal is not to build a Bash clone. `crbsh` should remain compatible with traditional Unix programs while introducing a cleaner shell language, structured data, modern process execution, and developer-focused ergonomics.

Core design principles:

- Fast startup and low runtime overhead.
- Clean, testable Rust architecture.
- Strong Unix interoperability.
- Structured internal representations instead of stringly-typed shell behavior.
- Predictable parsing and execution semantics.
- First-class developer tooling.
- Extensible architecture without premature complexity.
- Safe defaults.
- Clear separation between parsing, shell state, builtins, and execution.

The project is currently early-stage. Prefer correctness, clarity, and architecture over feature count.

---

## Current Architecture

The source tree is expected to evolve roughly around these responsibilities:

```text
src/
├── main.rs
├── shell.rs
├── prompt.rs
├── parser.rs
├── tokens.rs
├── executor.rs
└── builtins/
    ├── mod.rs
    ├── registry.rs
    ├── cd.rs
    ├── exit.rs
    └── print.rs
```

Responsibilities:

- `main.rs`
  - Application entrypoint only.
  - Initializes the shell.
  - Avoid putting parsing, builtin dispatch, or process execution logic here.

- `shell.rs`
  - Owns long-lived shell state.
  - Examples:
    - builtin registry
    - last exit code
    - aliases
    - variables
    - jobs
    - configuration
    - history
  - New persistent shell state should generally belong here.

- `prompt.rs`
  - Handles rendering the interactive prompt.
  - Prompt formatting should not leak into command execution logic.

- `tokens.rs`
  - Lexical analysis.
  - Converts raw input into `Token` values.
  - Handles quotes, escaping, operators, and other lexical constructs.
  - Should not decide execution semantics.

- `parser.rs`
  - Consumes tokens.
  - Produces structured representations such as commands and pipelines.
  - Grammar decisions belong here.
  - Do not execute commands from the parser.

- `executor.rs`
  - Executes external programs.
  - Eventually owns pipeline wiring, stdin/stdout routing, redirection, and process management.
  - Should not contain parsing logic.

- `builtins/`
  - Contains commands implemented directly by `crbsh`.
  - Examples:
    - `cd`
    - `exit`
    - `print`
  - Builtins may mutate `Shell` state where appropriate.

---

## Builtin Architecture

Builtins should use a centralized registry rather than a large `match` statement in `main.rs`.

Use the existing function-pointer pattern.

Conceptually:

```rust
pub type BuiltinFn =
    fn(&mut Shell, &[String]) -> BuiltinResult;
```

Each builtin should expose a function shaped like:

```rust
pub fn run(
    shell: &mut Shell,
    args: &[String],
) -> BuiltinResult
```

Builtins should return structured outcomes rather than terminating the process directly.

Expected concepts:

```rust
pub enum BuiltinOutcome {
    Continue,
    Exit(i32),
}
```

Errors should be returned instead of printed deep inside builtin implementations whenever practical.

Prefer:

```rust
Err(BuiltinError::new("message"))
```

over:

```rust
eprintln!("message");
```

Central execution code should decide how errors are presented and how exit status is updated.

Do not add a giant command dispatch `match` to `main.rs`.

---

## Parsing Model

Input processing should follow this pipeline:

```text
raw input
    ↓
tokenizer
    ↓
Vec<Token>
    ↓
parser
    ↓
structured command / pipeline representation
    ↓
executor
```

The tokenizer and parser must remain separate.

### Current Token Direction

Tokens may include concepts such as:

```rust
pub enum Token {
    Word(String),
    Pipe,
    RedirectOut,
    RedirectAppend,
    RedirectIn,
    Background,
}
```

Additional tokens may be introduced as grammar expands.

Do not interpret shell operators as ordinary strings once a dedicated token exists.

For example:

```text
ls | grep rs
```

should conceptually become:

```rust
[
    Token::Word("ls".into()),
    Token::Pipe,
    Token::Word("grep".into()),
    Token::Word("rs".into()),
]
```

The parser then decides what that token stream means.

---

## Quoting and Escaping

The tokenizer should preserve expected behavior for:

```text
print "hello world"
print 'hello world'
print hello\ world
```

Expected parsed arguments:

```text
hello world
```

Operators inside quotes should be treated as word content.

Example:

```text
print "hello | crab"
```

The `|` must not become a pipeline operator.

Escaped operators should also remain word content where supported.

---

## Pipeline Direction

Pipeline parsing is a current architectural priority.

The desired model is something like:

```rust
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
}
```

Example:

```text
ls -la | grep rs | sort
```

should parse into three structured commands.

Pipeline execution should eventually use Rust process primitives such as:

```rust
std::process::Command
std::process::Stdio
```

Do not implement pipelines by invoking another shell such as:

```rust
sh -c
bash -c
zsh -c
```

`crbsh` must own its parsing and execution behavior.

---

## Multiline Design

The architecture should leave room for multiline input.

Examples that should be supportable in the future:

```text
ls -la |
    grep rs
```

and scripting constructs such as:

```text
if condition {
    print "hello"
}
```

Interactive input may eventually distinguish between:

- complete input
- incomplete input requiring continuation
- invalid input

Do not hard-code assumptions that every newline always means immediate execution.

For complete source buffers or scripts, newline information may later be meaningful to the grammar.

---

## Unix Compatibility

External commands should continue to work through `$PATH`.

Examples:

```text
ls
git status
cargo test
nvim file.rs
docker ps
```

Do not rewrite standard Unix utilities unless there is a specific `crbsh` reason to do so.

`crbsh` may provide native alternatives, but Unix interoperability is a core requirement.

Commands affecting the shell process itself must generally be builtins.

Examples:

```text
cd
exit
export
unset
alias
jobs
fg
bg
```

A child process cannot modify the state of its parent shell.

---

## Shell Language Direction

`crbsh` should not blindly inherit POSIX shell syntax.

The project may introduce cleaner syntax where it improves usability.

Current example:

```text
print "hello"
```

is preferred as the native output builtin instead of making `echo` part of the language design.

External `echo` may still execute if available through `$PATH`.

Future language areas may include:

- variables
- typed values
- structured values
- pipelines
- functions
- conditions
- loops
- async/background jobs
- project-local configuration
- native HTTP functionality
- structured command output
- extensibility/plugins

Do not implement speculative syntax without considering how it affects the grammar as a whole.

---

## Structured Data Long-Term Goal

A major long-term differentiator for `crbsh` is structured data.

Traditional shells primarily pass bytes/text between processes.

`crbsh` may eventually support native values such as:

```rust
enum Value {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Record(/* structured fields */),
}
```

Do not prematurely implement this system unless required by the current task, but avoid architecture that would make structured pipelines impossible later.

Unix text streams and native structured values should eventually be able to interoperate.

---

## Error Handling

Avoid `unwrap()` and `expect()` in production paths where failure is reasonably possible.

Acceptable exceptions may include cases where an invariant is genuinely guaranteed, but prefer explicit handling.

Use `Result` for recoverable failures.

Errors should:

- identify the command or subsystem when useful
- remain concise
- preserve useful underlying OS errors
- produce meaningful exit codes

Example:

```text
crbsh: cd: /missing: No such file or directory
```

Do not panic because a user typed invalid shell input.

Malformed input is user input, not an application invariant violation.

---

## Exit Codes

Preserve external process exit status.

`Shell` should track the last command's exit code.

Common conventions are acceptable where useful:

- `0` — success
- `1` — general command failure
- `2` — parse/usage failure
- `126` — found but cannot execute
- `127` — command not found

Avoid hard-coding codes inconsistently across modules.

---

## Rust Style

Prefer idiomatic Rust.

Use:

```rust
&[T]
```

for read-only sequence parameters when ownership or resizing is not required.

Prefer:

```rust
&[String]
```

over:

```rust
&Vec<String>
```

unless the implementation specifically requires a `Vec`.

Avoid unnecessary cloning.

Prefer moving values when ownership can be transferred cleanly.

Examples:

```rust
std::mem::take(...)
into_iter()
```

may be preferable to repeated allocation or cloning where appropriate.

Do not optimize prematurely at the expense of readability.

---

## Module Visibility

Keep implementation details private unless another module genuinely needs access.

Prefer:

```rust
mod cd;
```

over:

```rust
pub mod cd;
```

when the module only needs to be accessed by the builtin subsystem.

Expose stable abstractions rather than every implementation module.

---

## Testing

New parser, tokenizer, and execution behavior should include tests.

Before considering work complete, run:

```bash
cargo fmt
cargo test
cargo check
```

All should pass.

For broader Rust hygiene, also prefer:

```bash
cargo clippy --all-targets --all-features
```

Do not silence warnings merely to make the build green unless the warning is intentionally unavoidable and documented.

### Parser/Tokenizer Tests

Test both successful and malformed input.

Examples:

```text
print hello
print "hello crab"
print 'hello crab'
print hello\ crab
ls | grep rs
```

Also test failures:

```text
|
ls |
ls || grep
print "unterminated
print trailing\
```

When adding new grammar, add regression tests for existing behavior.

---

## Formatting

Use `cargo fmt`.

Do not manually fight `rustfmt` without a compelling reason.

Prefer readable functions over dense one-liners.

Keep functions focused on one responsibility.

---

## Dependencies

Avoid adding crates for functionality that is straightforward with the Rust standard library.

Before adding a dependency, consider:

1. Is this difficult or risky to implement correctly ourselves?
2. Is the crate mature and actively maintained?
3. Does it materially improve correctness or maintainability?
4. Does it significantly increase compile time or dependency surface?
5. Is the feature core enough to justify the dependency?

Parser and process execution functionality should not automatically introduce a heavy framework.

Small, well-justified dependencies are fine.

---

## Security

Treat shell input as untrusted input.

Never construct command strings and pass them through another shell just to simplify execution.

Prefer:

```rust
Command::new(command)
    .args(args)
```

over:

```rust
Command::new("sh")
    .arg("-c")
    .arg(user_input)
```

unless explicitly implementing a carefully designed compatibility feature.

Do not introduce implicit code execution from configuration files without considering trust boundaries.

Future plugin systems should favor sandboxing and explicit permissions.

---

## Performance

Startup performance matters for a shell.

Avoid:

- expensive initialization
- unnecessary filesystem scans
- network access during startup
- eagerly loading optional subsystems
- heavy runtime frameworks without justification

Measure before optimizing.

Correctness and architecture come first.

---

## UX Philosophy

`crbsh` should feel modern without becoming magical or unpredictable.

Prefer:

- explicit behavior
- useful errors
- discoverability
- sensible defaults
- minimal ceremony
- Unix interoperability

Avoid surprising implicit transformations.

The shell should be pleasant for interactive use and reliable for scripting.

---

## Scope Discipline

Do not implement ten features when the task requires one.

Current development should proceed incrementally:

1. Core REPL
2. Builtin registry
3. Tokenization
4. Parsing
5. Pipelines
6. Redirection
7. Background jobs
8. Variables and shell state
9. More advanced scripting grammar
10. Structured data and higher-level developer features

Keep each milestone green before moving forward.

---

## Codex Working Rules

When modifying this repository:

- Read the relevant existing files before changing architecture.
- Preserve working behavior unless the task explicitly changes it.
- Do not rewrite unrelated modules.
- Prefer small, focused diffs.
- Add tests for behavior changes.
- Run formatting and tests after changes.
- Commit after every clean code change.
- Explain architectural changes in commit summaries or final output.
- Do not introduce speculative abstractions without a concrete current use case.
- Do not replace existing project conventions without a clear improvement.
- Do not bypass `crbsh` parsing by delegating command interpretation to Bash, Zsh, Fish, or `/bin/sh`.
- Do not silently change shell syntax.
- Treat compiler warnings seriously.
- Favor maintainable code over clever code.

If architectural intent is unclear, preserve the existing design and make the smallest safe change.

---

## Definition of Done

A change is complete when:

```bash
cargo fmt --check
cargo test
cargo check
cargo clippy --all-targets --all-features
```

are clean, or any remaining issue is explicitly documented and justified.

New functionality should have appropriate tests and should not regress existing shell behavior.

The result should remain understandable to another experienced engineer opening the repository for the first time.
