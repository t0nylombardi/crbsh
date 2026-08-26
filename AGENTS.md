# AGENTS.md

## Project Overview

`crbsh` is an experimental Unix shell with its own typed language, written in Rust.

The repository is a Cargo workspace with a deliberate separation between:

- `crab-lang`: the reusable language library.
- `crbsh`: the Unix shell host and executable.

`crbsh` is not a Bash clone and must not delegate user input to Bash, Zsh, Fish, `/bin/sh`, or another shell. The project owns lexing, parsing, evaluation, structured values, Unix process execution, pipelines, redirection, jobs, and host integration directly.

The project is early-stage. Prefer correctness, explicit semantics, clean architecture, regression coverage, and small focused changes over feature count.

---

## Core Design Principles

- Preserve a real boundary between language semantics and Unix host behavior.
- Keep `crab-lang` reusable and independent of `crbsh`.
- Maintain strong Unix interoperability through direct process execution.
- Prefer typed native values over stringly-typed shell behavior.
- Keep parsing, evaluation, host execution, and shell state separate.
- Treat shell input as untrusted user input.
- Preserve predictable semantics across interactive and script execution.
- Avoid speculative abstractions and premature optimization.
- Keep startup and runtime overhead low.
- Do not add dependencies without a concrete justification.
- Prefer explicit architecture over hidden coupling.

---

## Workspace Architecture

The intended dependency direction is:

```text
crab-lang
    ↑
    │ dependency
    │
  crbsh
```

`crab-lang` must never depend on `crbsh`.

Conceptually:

```text
source text
    │
    ▼
┌──────────────────────── crab-lang ────────────────────────┐
│ lexer → tokens → parser → AST                             │
│ language values, types, scopes, functions, ValueStream    │
└───────────────────────────┬────────────────────────────────┘
                            │ parsed input / native values
                            ▼
┌────────────────────────── crbsh ───────────────────────────┐
│ evaluator + persistent shell state                        │
│ builtins, aliases, environment, history, startup config   │
│ Unix processes, Stdio, redirection, jobs, REPL, rendering │
└────────────────────────────────────────────────────────────┘
```

Do not make Cargo appear cleaner while leaving conceptual dependencies tangled.

---

## Current Source Layout

```text
.
├── Cargo.toml
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
    ├── parser/mod.rs
    ├── runtime/
    │   ├── mod.rs
    │   └── evaluator.rs
    ├── execution/
    │   ├── command.rs
    │   ├── pipeline.rs
    │   ├── structured.rs
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

If the actual tree differs, inspect the repository before changing architecture. Do not assume this document is newer than the code.

---

## `crab-lang` Responsibilities

`crab-lang` owns reusable language semantics.

It may own:

- lexical analysis;
- token definitions;
- quoting and escaping rules;
- literals and language operators;
- parser and AST construction;
- language expressions;
- language control-flow definitions;
- `Value`;
- `TypeName`;
- lexical scope state;
- function definitions and signatures;
- native `ValueStream`;
- native value transformations.

It must not own:

- Unix process spawning;
- `std::process::Command` orchestration for external commands;
- `Stdio` wiring;
- terminal control;
- jobs;
- shell aliases;
- shell history;
- startup configuration;
- shell builtins;
- host environment mutation policy;
- Unix byte-stream adapters.

Avoid importing host types such as `Shell`, `JobManager`, builtin registries, or execution-layer structs into `crab-lang`.

---

## `crbsh` Responsibilities

`crbsh` is the Unix host.

It owns:

- REPL behavior;
- script entrypoint behavior;
- prompt rendering;
- startup configuration;
- persistent shell session state;
- aliases;
- history;
- builtins;
- environment integration;
- exit status;
- process spawning;
- `$PATH` lookup;
- Unix text pipelines;
- `Stdio` wiring;
- redirection;
- background jobs;
- foreground job control;
- text ↔ native-value adaptation;
- final output rendering;
- mixed native/Unix pipeline orchestration.

Host behavior may consume language AST and `Value` objects from `crab-lang`.

---

## Evaluator Boundary

The evaluator currently remains in `crbsh` because evaluation can involve host-only capabilities such as:

- environment values;
- shell status;
- builtins;
- external processes;
- filesystem globs;
- mixed pipelines.

Do not force the evaluator into `crab-lang` unless its host dependencies have first been reduced to a narrow interface.

If host-backed language values such as:

```text
status
env.HOME
```

are moved behind an abstraction, prefer a minimal host capability interface rather than passing `Shell` into the language runtime.

Example direction:

```rust
pub trait HostContext {
    fn status(&self) -> i32;
    fn env_get(&self, name: &str) -> Option<String>;
}
```

Do not recreate `Shell` behind a giant trait.

---

## Parsing and AST Model

The project owns the full syntax pipeline.

Conceptually:

```text
source
    ↓
lexer
    ↓
Vec<Token>
    ↓
parser
    ↓
ParsedInput / AST
    ↓
runtime + host execution
```

The lexer recognizes syntax.

The parser decides what syntax means.

The evaluator executes language semantics.

The execution layer owns Unix behavior.

Do not mix these responsibilities.

### AST Layers

Current semantic layers include concepts such as:

```text
PipelineChain
    ↓
Pipeline
    ↓
Command
    ↓
Expression
    ↓
Value
```

Language AST and host-command syntax should remain distinguishable even when they are parsed by the same parser.

Redirection and background execution are host-oriented syntax.

Native expressions, functions, typed values, match expressions, and lexical scopes are language semantics.

---

## Supported Language Values

Current native types include:

```text
string
int
bool
list<T>
record
```

`Value` currently supports corresponding runtime forms.

Lists are homogeneous.

Examples:

```crb
let project: string = "crbsh"
let retries: int = 3
let ready: bool = retries < 5

let names: list<string> = ["Tony", "Alice", "Bob"]
let empty: list<int> = []
```

Quoted literals stay literal.

Examples:

```text
"true" → string
true   → bool

"3"    → string
3      → int
```

Do not silently coerce unrelated native types unless semantics explicitly require it.

---

## Variables and Assignment

Variables may be inferred:

```crb
let project = "crbsh"
let retries = 3
let ready = true
```

or explicitly typed:

```crb
let project: string = "crbsh"
let retries: int = 3
let ready: bool = true
```

Bindings are type-stable.

Valid:

```crb
let retries = 3
retries = 5
```

Invalid:

```crb
let retries = 3
retries = "five"
```

`let` declares in the current scope.

Assignment should update the nearest existing binding according to current scope rules.

Do not make assignment semantics dynamically change types.

---

## Environment Semantics

Native variables and environment variables are distinct.

Supported forms include:

```crb
print @HOME
print env.HOME
env.RUST_LOG = "debug"
```

Native variable names must not implicitly collide with environment variables.

For example:

```crb
let HOME = "local"

print HOME
print @HOME
print env.HOME
```

must remain unambiguous.

Environment overrides used by child processes should be shell-owned state rather than process-global mutation when practical.

Unset inherited environment values must remain removed for child processes, including values inherited before `crbsh` started.

---

## Status Semantics

`status` resolves from the shell's previous exit code.

Example:

```crb
false
print status
```

prints:

```text
1
```

Pipeline status policy is:

> The overall pipeline status is the final stage exit code.

Logical chaining relies on this policy.

Do not change it without updating documentation and regression tests.

---

## Functions

Current function behavior includes:

- typed parameters;
- optional inferred parameters for procedures;
- typed return values;
- `return`;
- nested return propagation;
- isolated function-local scope;
- parameter binding;
- shadowing;
- recursive calls;
- recursion-depth guard;
- function calls in expressions;
- nested calls.

Examples:

```crb
fn add(a: int, b: int) -> int {
    return a + b
}

let total = add(2, 3)
```

Procedures may infer parameter types per invocation:

```crb
fn show(value) {
    print value
}
```

Value-returning functions require typed parameters and an explicit return type.

Return types must be enforced.

Recursive calls must respect the runtime recursion-depth limit.

Do not weaken these contracts without an explicit language-design decision.

---

## Scope Rules

Function calls use isolated local scopes.

Nested blocks use lexical scopes.

Lookup proceeds from the innermost scope outward.

A local declaration may shadow an outer binding.

Example:

```crb
let value = 5

fn test(value: int) {
    let local = 10
    print value
    print local
}
```

Function parameters are value bindings, not accidental aliases to caller variables.

Block-local variables must not leak outside their scope.

---

## Control Flow

Current control flow includes:

```text
if / else if / else
while
for
break
continue
match
return
```

Blocks use braces.

Examples:

```crb
if condition {
    print "yes"
} else {
    print "no"
}
```

```crb
while retries < 3 {
    retries = retries + 1
}
```

`for` supports:

- native lists;
- integer ranges;
- inclusive integer ranges;
- single-wildcard filesystem globs.

Examples:

```crb
for i in 0..10 {
    print i
}

for i in 0..=10 {
    print i
}

for file in src/*.rs {
    print file
}
```

`break` and `continue` must propagate correctly through nested blocks.

---

## Match Semantics

Match supports:

- integer literal patterns;
- string literal patterns;
- boolean literal patterns;
- `_` wildcard;
- first-match-wins semantics;
- nested matches;
- match statements;
- match expressions.

Statement matches may be non-exhaustive.

Match expressions must produce a value and therefore require a wildcard arm under current semantics.

Example:

```crb
let label = match status {
    0 => "success"
    1 => "failure"
    _ => "unknown"
}
```

Return propagation must work through nested match arms.

---

## Lists and Indexing

Lists are homogeneous.

Mixed element types are rejected.

Examples:

```crb
let names = ["Tony", "Alice"]
let first = names[0]
let count = names.len
```

Indexing:

- requires an integer;
- rejects negative indexes;
- rejects out-of-bounds indexes;
- composes inside expressions.

Example:

```crb
let answer = [20, 21, 22][1] * 2
```

Index assignment is not currently implemented.

Do not silently introduce mutable list indexing without an explicit feature decision.

---

## Records

Records are native structured values.

They may participate in:

- variables;
- function arguments;
- function returns;
- structured streams.

Record behavior must remain deterministic and testable.

Missing-field behavior should produce explicit errors rather than silently returning null-like values unless the language specification changes.

---

## Unix Commands

External programs execute directly through `$PATH`.

Examples:

```text
ls
git status
cargo test
nvim file.rs
```

Do not rewrite standard Unix programs unless there is a specific native-language reason.

Do not delegate command parsing or execution to another shell.

Never use:

```rust
Command::new("sh").arg("-c").arg(user_input)
```

as a shortcut for executing user syntax.

Use direct process construction.

---

## Text Pipelines

Traditional Unix pipelines are supported.

Example:

```crb
ls -la | grep rs | sort
```

Pipeline stages should use direct `std::process::Command` and `Stdio` wiring.

Do not shell out to Bash/Zsh/Fish to implement pipelines.

The final pipeline stage determines overall status.

---

## Logical Chaining

Supported:

```text
&&
||
```

Examples:

```crb
cargo build && print "build passed"
false || print "command failed"
```

Semantics:

- `&&` runs the next pipeline only when the previous status is `0`.
- `||` runs the next pipeline only when the previous status is non-zero.
- evaluation short-circuits;
- chaining is left-associative.

Background `&` is intentionally rejected anywhere inside a logical chain under current semantics.

Examples that must remain invalid:

```text
foo & && bar
foo && & bar
foo && bar &
```

Do not change this without explicit design and tests.

---

## Redirection

Supported:

```text
<
>
>>
```

Examples:

```crb
cat < input.txt
print hello > output.txt
print hello >> output.txt
```

Structured pipeline redirection is valid only at supported final boundaries.

Redirection belongs to host execution, not native language semantics.

---

## Background Jobs

Trailing `&` runs an external command or external pipeline in the background.

Valid:

```text
sleep 10 &
ls | grep rs &
```

Invalid:

```text
& sleep 10
sleep & 10
sleep & ls
```

Logical chains containing `&` are currently rejected.

`JobManager` owns:

- job IDs;
- running/done state;
- process polling;
- shell-owned job state.

Do not block the REPL when starting a background job.

Background children must be reaped.

---

## Job Control

Current builtins include:

```text
jobs
fg
```

Examples:

```crb
sleep 10 &
jobs
fg 1
```

Do not introduce full POSIX suspend/resume semantics casually.

Proper stopped-job support may require:

- process groups;
- terminal ownership;
- signals;
- `SIGTSTP`;
- `SIGCONT`;
- `SIGCHLD`;
- `tcsetpgrp()`.

Treat that as a deliberate future subsystem.

---

## Aliases

Aliases expand only in command position.

Example:

```crb
alias p = "print alias"
p tail
unalias p
```

Do not expand alias names inside ordinary expression arguments.

Alias expansion must guard against cycles.

---

## History

Interactive history is persisted at:

```text
$XDG_STATE_HOME/crbsh/history
```

when `XDG_STATE_HOME` exists, otherwise:

```text
~/.local/state/crbsh/history
```

Multiline input should be stored as one logical history entry.

Consecutive duplicate suppression is preferred over global deduplication.

History semantics should remain independent from parser semantics.

---

## Startup Configuration

Interactive startup configuration is loaded from:

```text
~/.crbshrc
```

when present.

Configuration is executable shell-language input.

Treat startup configuration as user-controlled code.

Do not introduce network access or surprising external execution during startup.

---

## Builtin Architecture

Builtins are shell-host features.

Current registered builtins include:

```text
alias
cd
exit
export
fg
history
jobs
print
set
unalias
unset
```

Use the existing centralized registry.

Do not reintroduce a giant builtin dispatch `match` in `main.rs`.

Builtin errors should return structured errors when practical rather than printing deep inside implementation code.

The shell host should decide presentation and exit status.

---

## Structured Pipelines

Structured pipelines are a major crbsh differentiator.

A pipeline becomes structured when native structured stages participate.

`crab-lang` owns native value-stream semantics.

`crbsh` owns Unix boundaries.

### ValueStream

Keep the current implementation simple unless profiling proves a need for more.

Conceptually:

```rust
pub struct ValueStream {
    values: Vec<Value>,
}
```

Do not prematurely replace it with:

- async streams;
- channels;
- complex iterator traits;
- backpressure infrastructure;
- speculative streaming frameworks.

Current behavior is buffered in memory. Preserve semantics before optimizing.

### Native Structured Stages

Current examples include:

```text
values
record
take
count
collect
```

Native stages exchange `Value` items.

Lists passed through `values` expand by one level.

Records remain atomic stream items.

`collect` bundles stream items into a list.

### Unix Boundary

The host should explicitly cross native/Unix boundaries.

Conceptually:

```rust
enum PipelinePayload {
    Values(ValueStream),
    Bytes(Vec<u8>),
}
```

Flow:

```text
native stage
    ↓
ValueStream
    ↓
Unix adapter
    ↓
bytes
    ↓
external process
    ↓
bytes
    ↓
native adapter
    ↓
ValueStream
```

External programs operate on bytes/text.

Native stages operate on `ValueStream`.

Do not make `crab-lang` own a generic pipeline executor that knows about external processes.

### Structured ↔ Unix Adaptation

When values enter an external Unix command:

- render one value per newline-delimited text item.

When external output enters a later native stage:

- decode UTF-8;
- convert each line into a native string value.

Invalid UTF-8 must produce a stage-specific error.

Final structured output renders one value per line.

Structured output may support final-stage redirection according to current execution rules.

### Current Structured Pipeline Limitations

- streams are buffered in memory;
- background structured pipelines are not supported;
- stateful builtins are rejected inside structured pipelines.

Do not silently remove these restrictions without defining semantics and adding regression tests.

---

## Multiline Input

The parser and REPL must leave room for complete/incomplete/invalid input states.

Current continuation behavior primarily relies on brace balance.

Do not assume every physical newline always means execution.

For scripts or complete source buffers, newline may be meaningful to grammar and formatting.

A future richer completeness model may distinguish:

```text
Complete
Incomplete
Invalid
```

Do not break multiline blocks while modifying REPL behavior.

---

## `.crb` Scripts

Crab scripts use:

```text
.crb
```

Example:

```sh
cargo run -p crbsh -- path/to/script.crb
```

Other file extensions are currently rejected.

Do not use `.csh`; that extension is associated with the historical C shell.

---

## Error Handling

Avoid `unwrap()` and `expect()` in production paths where failure is reasonably possible.

Use `Result` for recoverable failures.

Malformed user input must not panic the shell.

Errors should:

- identify the relevant command, stage, parser location, or subsystem;
- remain concise;
- preserve useful underlying OS errors;
- map to meaningful exit codes;
- distinguish parser/runtime/host failures where useful.

Do not suppress errors merely to keep execution moving.

---

## Exit Codes

Preserve external process exit status.

Current shell status semantics include conventional codes where appropriate:

```text
0   success
1   general failure
2   parse/usage failure
126 found but cannot execute
127 command not found
```

Do not scatter inconsistent hard-coded exit codes across modules.

Pipeline status is the final stage's status.

---

## Rust Style

Prefer idiomatic Rust.

Use slices for read-only sequence APIs:

```rust
&[T]
```

instead of unnecessarily requiring:

```rust
&Vec<T>
```

Avoid unnecessary cloning.

Prefer ownership transfer where clean.

Useful tools may include:

```rust
into_iter()
std::mem::take(...)
```

Do not optimize prematurely at the expense of clarity.

Prefer explicit types at architecture boundaries and inference inside straightforward local code.

---

## Module Visibility

Keep implementation details private unless another module genuinely requires them.

Expose stable abstractions rather than implementation modules.

Do not widen visibility simply to work around poor boundaries.

If cross-crate access requires exposing a type, consider whether that type actually belongs in the public API first.

---

## Dependencies

The project currently intentionally uses no third-party Rust dependencies.

Do not add a dependency casually.

Before adding one, answer:

1. Is this difficult or risky to implement correctly with the standard library?
2. Is the dependency mature and actively maintained?
3. Does it materially improve correctness or maintainability?
4. What does it add to compile time and dependency surface?
5. Does it fit the project architecture?
6. Can the same goal be achieved without coupling core language semantics to a framework?

If a dependency is justified, explain why.

---

## Security

Treat all shell input, scripts, config, aliases, and external command arguments as untrusted input.

Never construct a raw command string and hand it to another shell as a shortcut.

Prefer direct process construction:

```rust
Command::new(command).args(args)
```

Do not introduce implicit remote code execution.

Do not fetch or execute network content during startup.

Future plugin systems should prefer explicit permissions and isolation.

---

## Performance

Shell startup matters.

Avoid:

- expensive eager initialization;
- unnecessary filesystem scans;
- network access during startup;
- heavy runtime frameworks;
- loading optional subsystems eagerly.

Measure before optimizing.

Structured streams are currently buffered. Do not implement async/backpressure/JIT/VM infrastructure without a measured or architectural need.

Correctness and clean boundaries come first.

---

## Future Compiler Direction

Do not implement this unless explicitly requested, but preserve an architecture that could support:

```text
source
  ↓
lexer
  ↓
parser
  ↓
AST
  ↓
static type checker
  ↓
typed AST
  ↓
IR / bytecode
  ↓
interpreter / VM
  ↓
optional JIT or AOT
```

The next serious compiler milestone is likely static type checking before any JIT work.

Do not add a JIT directly on top of an unstable AST/runtime architecture.

---

## Documentation Boundaries

The README is the project introduction and quick reference.

As the language grows, prefer dedicated documentation for:

```text
LANGUAGE.md
docs/shell.md
docs/architecture.md
```

Language semantics should be documented separately from shell-host behavior when practical.

Any behavior change affecting documented semantics must update the appropriate docs.

---

## Testing

Every behavior change should include focused tests.

Parser, evaluator, structured pipeline, function, scope, job, environment, and execution changes should have regression coverage.

Before considering work complete, run the full workspace gate:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The project Makefile exposes:

```bash
make ci
```

which should run the equivalent project gate.

Do not silence warnings to achieve green CI unless the warning is intentionally unavoidable and documented.

---

## Test Philosophy

Test semantics, not implementation accidents.

Important categories include:

- successful parsing;
- malformed syntax;
- quoting and escaping;
- variable inference;
- strict assignment;
- scope behavior;
- function parameter and return validation;
- recursion guard;
- nested return propagation;
- lists and indexing;
- match behavior;
- pipeline exit status;
- logical short-circuiting;
- left associativity;
- background grammar;
- job state;
- environment inheritance;
- structured native stages;
- text ↔ value adaptation;
- invalid UTF-8 boundaries;
- alias command-position behavior;
- history persistence;
- multiline parsing.

When fixing a bug, add a regression test reproducing it.

---

## Formatting

Use `cargo fmt`.

Do not manually fight `rustfmt` without a strong reason.

Prefer readable, focused functions over compressed clever code.

---

## Scope Discipline

Do not implement unrelated features while completing a focused task.

Prefer small, coherent feature branches.

Keep each branch green before merging.

For refactors:

- preserve semantics;
- separate structure changes from behavior changes;
- migrate tests with the owning subsystem;
- avoid unrelated cleanup unless necessary.

Large architecture work should be decomposed into explicit seams rather than broad rewrites.

---

## Commit Discipline

Commit after each clean, coherent code change.

Prefer commits that represent one meaningful architectural or behavioral step.

Examples:

```text
feat: add typed function return validation
feat: add structured ValueStream transformation
refactor: isolate language runtime state
test: cover recursive scope shadowing
```

Avoid giant commits such as:

```text
update stuff
functions work now
cleanup
```

Branches should be short-lived and merged after the full project gate passes.

---

## Codex Working Rules

When modifying this repository:

- inspect the relevant code before proposing architecture changes;
- preserve existing semantics unless explicitly asked to change them;
- do not rewrite unrelated modules;
- prefer small focused diffs;
- add regression tests for behavior changes;
- run the complete workspace gate;
- commit after each clean coherent change;
- do not silently change syntax;
- do not delegate parsing/execution to another shell;
- do not move host behavior into `crab-lang`;
- do not move language semantics into `crbsh` merely for convenience;
- keep structured pipeline native semantics separate from Unix adaptation;
- treat compiler warnings seriously;
- favor maintainable code over clever code;
- avoid speculative abstractions;
- do not introduce async/JIT/plugin systems unless explicitly requested;
- update docs when user-visible semantics change.

If architectural intent is unclear, preserve the current boundary and make the smallest safe change.

---

## Definition of Done

A change is complete when:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

all pass, or any remaining issue is explicitly documented and justified.

Additionally:

- new functionality has tests;
- existing behavior has not regressed;
- workspace boundaries remain clean;
- `crab-lang` does not depend on `crbsh`;
- host-only behavior remains in the shell host;
- documented semantics are updated when behavior changes;
- the result remains understandable to an experienced engineer opening the repository for the first time.
