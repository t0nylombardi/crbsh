mod builtins;
mod executor;
mod history;
mod jobs;
mod parser;
mod prompt;
mod shell;
mod tokens;
mod value;

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use builtins::BuiltinOutcome;
use parser::{Expression, Iterable, ParsedCommand, ParsedInput, Pipeline, PipelineConnector};
use shell::{Shell, ShellError};
use value::TypeName;
use value::Value;

enum EvalError {
    Shell(ShellError),
    Function(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(error) => write!(formatter, "{error}"),
            Self::Function(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<ShellError> for EvalError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

enum ControlFlow {
    Continue,
    Break,
    LoopContinue,
    Return(Option<Value>),
    Exit(i32),
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let mut shell = Shell::new();

    match args.as_slice() {
        [_] => run_repl(&mut shell),
        [_, path] => std::process::exit(run_script(&mut shell, path)),
        _ => {
            eprintln!("crbsh: usage: crbsh [script.crb]");
            std::process::exit(2);
        }
    }
}

fn run_repl(shell: &mut Shell) {
    let history_path = history::default_history_path();

    if let Some(path) = history_path.as_ref() {
        match history::History::load(path, 1000) {
            Ok(history) => shell.history = history,
            Err(err) => {
                eprintln!("crbsh: {}: {err}", path.display());
                shell.exit_code = 1;
            }
        }
    }

    run_interactive_config(shell);

    loop {
        print!("{}", prompt::render());
        if io::stdout().flush().is_err() {
            shell.exit_code = 1;
            return;
        }

        let input = match read_input() {
            Ok(Some(input)) => input,
            Ok(None) => return,
            Err(()) => continue,
        };

        if input.trim().is_empty() {
            continue;
        }

        let parsed_input = match parser::parse(&input) {
            Ok(parsed_input) => parsed_input,

            Err(err) => {
                eprintln!("crbsh: {}", parser::format_error(&err));
                shell.exit_code = 2;
                continue;
            }
        };

        shell.history.add(&input);
        if let Some(path) = history_path.as_ref()
            && let Err(err) = shell.history.save(path)
        {
            eprintln!("crbsh: {}: {err}", path.display());
            shell.exit_code = 1;
        }

        let flow = execute_input(shell, parsed_input);
        handle_control_flow(shell, flow, true);
    }
}

fn run_script(shell: &mut Shell, path: &str) -> i32 {
    if Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("crb")
    {
        eprintln!("crbsh: script files must use the .crb extension");
        return 2;
    }

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("crbsh: {path}: {err}");
            return 1;
        }
    };

    execute_source(shell, &source)
}

fn run_interactive_config(shell: &mut Shell) {
    let Some(path) = interactive_config_path() else {
        return;
    };

    if let Some(code) = run_config_file(shell, &path)
        && code != 0
    {
        shell.exit_code = code;
    }
}

fn interactive_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".crbshrc"))
}

fn run_config_file(shell: &mut Shell, path: &Path) -> Option<i32> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            eprintln!("crbsh: {}: {err}", path.display());
            return Some(1);
        }
    };

    Some(execute_source(shell, &source))
}

fn execute_source(shell: &mut Shell, source: &str) -> i32 {
    let statements = collect_source_statements(source);

    for statement in statements {
        let parsed_input = match parser::parse(&statement) {
            Ok(parsed_input) => parsed_input,
            Err(err) => {
                eprintln!("crbsh: {}", parser::format_error(&err));
                return 2;
            }
        };

        let flow = execute_input(shell, parsed_input);
        if let Some(code) = handle_control_flow(shell, flow, false) {
            return code;
        }
    }

    shell.exit_code
}

fn handle_control_flow(shell: &mut Shell, flow: ControlFlow, interactive: bool) -> Option<i32> {
    match flow {
        ControlFlow::Exit(code) => {
            if interactive {
                std::process::exit(code);
            }

            return Some(code);
        }
        ControlFlow::Break => {
            eprintln!("crbsh: break outside loop");
            shell.exit_code = 2;
        }
        ControlFlow::LoopContinue => {
            eprintln!("crbsh: continue outside loop");
            shell.exit_code = 2;
        }
        ControlFlow::Return(_) => {
            eprintln!("crbsh: return outside function");
            shell.exit_code = 2;
        }
        ControlFlow::Continue => {}
    }

    if interactive {
        None
    } else if shell.exit_code == 2 {
        Some(2)
    } else {
        None
    }
}

fn collect_source_statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();

    for line in source.lines() {
        if current.is_empty() && line.trim().is_empty() {
            continue;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);

        if brace_balance(&current) <= 0 && !current.trim().is_empty() {
            statements.push(std::mem::take(&mut current));
        }
    }

    if !current.trim().is_empty() {
        statements.push(current);
    }

    statements
}

fn read_input() -> Result<Option<String>, ()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    read_input_from(&mut stdin)
}

fn read_input_from(reader: &mut impl BufRead) -> Result<Option<String>, ()> {
    let mut input = String::new();

    match reader.read_line(&mut input) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(_) => return Err(()),
    }

    while brace_balance(&input) > 0 {
        let mut next_line = String::new();

        match reader.read_line(&mut next_line) {
            Ok(0) => return Ok(Some(input)),
            Ok(_) => {}
            Err(_) => return Err(()),
        }

        if next_line.is_empty() {
            break;
        }

        input.push_str(&next_line);
    }

    Ok(Some(input))
}

fn brace_balance(input: &str) -> i32 {
    let mut balance = 0;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quotes => escaped = true,
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '{' if !in_single_quotes && !in_double_quotes => balance += 1,
            '}' if !in_single_quotes && !in_double_quotes => balance -= 1,
            _ => {}
        }
    }

    balance
}

fn execute_input(shell: &mut Shell, parsed_input: ParsedInput) -> ControlFlow {
    match parsed_input {
        ParsedInput::FunctionDefinition { name, definition } => {
            shell.define_function(name, definition);
            shell.exit_code = 0;
        }

        ParsedInput::Let {
            name,
            type_annotation,
            value,
        } => {
            let value = match evaluate_expression(shell, &value) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            match shell.declare_variable(name, type_annotation, value) {
                Ok(()) => shell.exit_code = 0,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::Break => return ControlFlow::Break,

        ParsedInput::Continue => return ControlFlow::LoopContinue,

        ParsedInput::Return { value } => {
            let value = match value {
                Some(value) => match evaluate_expression(shell, &value) {
                    Ok(value) => Some(value),
                    Err(err) => {
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                        return ControlFlow::Continue;
                    }
                },
                None => None,
            };

            return ControlFlow::Return(value);
        }

        ParsedInput::Assignment { name, value } => {
            let value = match evaluate_expression(shell, &value) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            match shell.assign_variable(name, value) {
                Ok(()) => shell.exit_code = 0,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::EnvironmentAssignment { name, value } => {
            match evaluate_expression(shell, &value) {
                Ok(value) => {
                    shell.set_environment(name, value.to_string());
                    shell.exit_code = 0;
                }
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::If {
            branches,
            else_body,
        } => {
            let flow = execute_if(shell, branches, else_body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::Match { value, arms } => {
            let flow = execute_match(shell, value, arms);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::While { condition, body } => {
            let flow = execute_while(shell, condition, body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::For {
            name,
            iterable,
            body,
        } => {
            let flow = execute_for(shell, name, iterable, body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::Pipeline(pipeline) => return execute_pipeline_input(shell, pipeline),

        ParsedInput::PipelineChain { first, rest } => {
            let flow = execute_pipeline_input(shell, first);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }

            for (connector, pipeline) in rest {
                let should_execute = match connector {
                    PipelineConnector::And => shell.exit_code == 0,
                    PipelineConnector::Or => shell.exit_code != 0,
                };

                if !should_execute {
                    continue;
                }

                let flow = execute_pipeline_input(shell, pipeline);

                if !matches!(flow, ControlFlow::Continue) {
                    return flow;
                }
            }
        }

        ParsedInput::BackgroundPipeline { pipeline, command } => {
            let pipeline = match expand_pipeline_aliases(shell, pipeline) {
                Ok(pipeline) => pipeline,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            if pipeline.commands.is_empty() {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }

            match executor::execute_background_pipeline(shell, &pipeline, command) {
                Ok((id, pid)) => {
                    println!("[{id}] {pid}");
                    shell.exit_code = 0;
                }

                Err(err) => {
                    eprintln!("crbsh: {}: {}", err.command, err.message);
                    shell.exit_code = 127;
                }
            }
        }
    }

    ControlFlow::Continue
}

fn execute_pipeline_input(shell: &mut Shell, pipeline: Pipeline) -> ControlFlow {
    let pipeline = match expand_pipeline_aliases(shell, pipeline) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    let parsed = match pipeline.commands.first() {
        Some(command) => command,
        None => {
            shell.exit_code = 0;
            return ControlFlow::Continue;
        }
    };

    let command = &parsed.name;
    let args = &parsed.args;

    if pipeline.commands.len() == 1
        && parsed.redirections.is_empty()
        && shell.function(command).is_some()
    {
        let result = execute_function_call(shell, command, args);

        match result {
            Ok(_) => shell.exit_code = 0,
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
            }
        }

        return ControlFlow::Continue;
    }

    if pipeline.commands.len() == 1
        && parsed.redirections.is_empty()
        && let Some(builtin) = shell.builtins.get(command)
    {
        let resolved_args = if uses_raw_builtin_args(command) {
            args.iter().map(raw_builtin_arg).collect()
        } else {
            args.iter()
                .map(|arg| shell.resolve_argument(arg))
                .collect::<Result<Vec<_>, _>>()
        };

        let resolved_args = match resolved_args {
            Ok(args) => args,
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 1;
                return ControlFlow::Continue;
            }
        };

        match builtin(shell, &resolved_args) {
            Ok(BuiltinOutcome::Continue) => {
                shell.exit_code = 0;
            }

            Ok(BuiltinOutcome::ContinueWithStatus(code)) => {
                shell.exit_code = code;
            }

            Ok(BuiltinOutcome::Exit(code)) => {
                return ControlFlow::Exit(code);
            }

            Err(err) => {
                eprintln!("crbsh: {}", err.message);
                shell.exit_code = 1;
            }
        }

        return ControlFlow::Continue;
    }

    match executor::execute_pipeline(shell, &pipeline) {
        Ok(code) => {
            shell.exit_code = code;
        }

        Err(err) => {
            eprintln!("crbsh: {}: {}", err.command, err.message);
            shell.exit_code = 127;
        }
    }

    ControlFlow::Continue
}

fn uses_raw_builtin_args(command: &str) -> bool {
    matches!(command, "alias" | "export" | "set" | "unalias" | "unset")
}

fn expand_pipeline_aliases(
    shell: &Shell,
    pipeline: Pipeline,
) -> Result<Pipeline, shell::AliasError> {
    pipeline
        .commands
        .into_iter()
        .map(|command| expand_command_aliases(shell, command))
        .collect::<Result<Vec<_>, _>>()
        .map(|commands| Pipeline { commands })
}

fn expand_command_aliases(
    shell: &Shell,
    mut command: ParsedCommand,
) -> Result<ParsedCommand, shell::AliasError> {
    let mut seen = Vec::new();

    loop {
        let Some(replacement) = shell.alias_command(&command.name)? else {
            return Ok(command);
        };

        if let Some(index) = seen.iter().position(|name| name == &command.name) {
            seen.push(command.name);
            return Err(shell::AliasError::Cycle(seen[index..].to_vec()));
        }

        seen.push(command.name);

        let mut args = replacement.args;
        args.extend(command.args);

        command.name = replacement.name;
        command.args = args;
    }
}

fn raw_builtin_arg(argument: &Expression) -> Result<String, ShellError> {
    Ok(match argument {
        Expression::Identifier(name) => name.clone(),
        Expression::EnvironmentVariable(name) => format!("env.{name}"),
        Expression::Status => "status".into(),
        Expression::Literal(value) => value.to_string(),
        Expression::Binary {
            left,
            operator,
            right,
        } => format!(
            "{} {} {}",
            raw_builtin_arg(left)?,
            operator.symbol(),
            raw_builtin_arg(right)?
        ),
        Expression::Call { name, .. } => name.clone(),
    })
}

fn execute_if(
    shell: &mut Shell,
    branches: Vec<parser::IfBranch>,
    else_body: Option<Vec<ParsedInput>>,
) -> ControlFlow {
    for branch in branches {
        let condition = match evaluate_expression(shell, &branch.condition) {
            Ok(Value::Bool(value)) => value,
            Ok(value) => {
                eprintln!(
                    "crbsh: type mismatch: expected bool, found {}",
                    value.type_name()
                );
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
        };

        if condition {
            return execute_block(shell, branch.body);
        }
    }

    match else_body {
        Some(body) => execute_block(shell, body),
        None => {
            shell.exit_code = 0;
            ControlFlow::Continue
        }
    }
}

fn execute_match(
    shell: &mut Shell,
    value: parser::Expression,
    arms: Vec<parser::MatchArm>,
) -> ControlFlow {
    let value = match evaluate_expression(shell, &value) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    for arm in arms {
        if pattern_matches(shell, &value, &arm.pattern) {
            return execute_input(shell, arm.body);
        }
    }

    shell.exit_code = 0;
    ControlFlow::Continue
}

fn execute_while(
    shell: &mut Shell,
    condition: parser::Expression,
    body: Vec<ParsedInput>,
) -> ControlFlow {
    loop {
        let condition = match evaluate_expression(shell, &condition) {
            Ok(Value::Bool(value)) => value,
            Ok(value) => {
                eprintln!(
                    "crbsh: type mismatch: expected bool, found {}",
                    value.type_name()
                );
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
        };

        if !condition {
            shell.exit_code = 0;
            return ControlFlow::Continue;
        }

        match execute_block(shell, body.clone()) {
            ControlFlow::Continue => {}
            ControlFlow::LoopContinue => continue,
            ControlFlow::Break => {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }
            flow @ ControlFlow::Return(_) => return flow,
            flow @ ControlFlow::Exit(_) => return flow,
        }
    }
}

fn execute_for(
    shell: &mut Shell,
    name: String,
    iterable: Iterable,
    body: Vec<ParsedInput>,
) -> ControlFlow {
    let values = match iterable_values(shell, iterable) {
        Ok(values) => values,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    for value in values {
        if let Err(err) = set_loop_variable(shell, &name, value) {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }

        match execute_block(shell, body.clone()) {
            ControlFlow::Continue => {}
            ControlFlow::LoopContinue => continue,
            ControlFlow::Break => {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }
            flow @ ControlFlow::Return(_) => return flow,
            flow @ ControlFlow::Exit(_) => return flow,
        }
    }

    shell.exit_code = 0;
    ControlFlow::Continue
}

fn execute_block(shell: &mut Shell, body: Vec<ParsedInput>) -> ControlFlow {
    shell.push_scope();
    let mut result = ControlFlow::Continue;

    for statement in body {
        let flow = execute_input(shell, statement);

        if !matches!(flow, ControlFlow::Continue) {
            result = flow;
            break;
        }
    }

    shell.pop_scope();
    result
}

fn iterable_values(shell: &mut Shell, iterable: Iterable) -> Result<Vec<Value>, EvalError> {
    match iterable {
        Iterable::Range {
            start,
            end,
            inclusive,
        } => {
            let start = expect_int(evaluate_expression(shell, &start)?)?;
            let end = expect_int(evaluate_expression(shell, &end)?)?;
            let upper = if inclusive {
                end.saturating_add(1)
            } else {
                end
            };

            Ok((start..upper).map(Value::Int).collect())
        }
        Iterable::Glob(pattern) => Ok(glob_values(&pattern)
            .into_iter()
            .map(Value::String)
            .collect()),
    }
}

fn expect_int(value: Value) -> Result<i64, EvalError> {
    match value {
        Value::Int(value) => Ok(value),
        value => Err(shell::ShellError::TypeMismatch {
            expected: value::TypeName::Int,
            found: value.type_name(),
        }
        .into()),
    }
}

fn set_loop_variable(shell: &mut Shell, name: &str, value: Value) -> Result<(), shell::ShellError> {
    match shell.assign_variable(name, value.clone()) {
        Ok(()) => Ok(()),
        Err(shell::ShellError::VariableNotDefined(_)) => shell.declare_variable(name, None, value),
        Err(err) => Err(err),
    }
}

fn glob_values(pattern: &str) -> Vec<String> {
    if pattern.matches('*').count() != 1 {
        return Vec::new();
    }

    let path = Path::new(pattern);
    let Some(file_pattern) = path.file_name().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Some((prefix, suffix)) = file_pattern.split_once('*') else {
        return Vec::new();
    };
    let directory = path.parent().filter(|path| !path.as_os_str().is_empty());
    let read_directory = directory.unwrap_or_else(|| Path::new("."));

    // v1 supports a single '*' in the file-name component, for example
    // '*.rs' or 'src/*.rs'. Recursive globs and multiple wildcards are ignored.
    let Ok(entries) = fs::read_dir(read_directory) else {
        return Vec::new();
    };

    let mut values = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
        .map(|name| {
            directory
                .map(|directory| directory.join(&name).to_string_lossy().into_owned())
                .unwrap_or(name)
        })
        .collect::<Vec<_>>();

    values.sort();
    values
}

fn pattern_matches(shell: &Shell, value: &Value, pattern: &parser::MatchPattern) -> bool {
    match pattern {
        parser::MatchPattern::Wildcard => true,
        parser::MatchPattern::Literal(pattern) => value == pattern,
        parser::MatchPattern::Status => value == &Value::Int(i64::from(shell.exit_code)),
        parser::MatchPattern::Identifier(name) => shell
            .evaluate(&parser::Expression::Identifier(name.clone()))
            .is_ok_and(|pattern| value == &pattern),
    }
}

fn evaluate_expression(
    shell: &mut Shell,
    expression: &parser::Expression,
) -> Result<Value, EvalError> {
    match expression {
        parser::Expression::Literal(value) => Ok(value.clone()),
        parser::Expression::Identifier(name) => shell
            .evaluate(&parser::Expression::Identifier(name.clone()))
            .map_err(Into::into),
        parser::Expression::EnvironmentVariable(name) => shell
            .environment_value(name)
            .map(Value::String)
            .ok_or_else(|| ShellError::UndefinedEnvironmentVariable(name.clone()).into()),
        parser::Expression::Status => Ok(Value::Int(i64::from(shell.exit_code))),
        parser::Expression::Binary {
            left,
            operator,
            right,
        } => {
            let left = evaluate_expression(shell, left)?;
            let right = evaluate_expression(shell, right)?;

            shell::evaluate_binary(*operator, left, right).map_err(Into::into)
        }
        parser::Expression::Call { name, args } => execute_function_call(shell, name, args)
            .and_then(|value| {
                value.ok_or_else(|| format!("function '{name}' did not return a value"))
            })
            .map_err(EvalError::Function),
    }
}

fn execute_function_call(
    shell: &mut Shell,
    name: &str,
    args: &[parser::Expression],
) -> Result<Option<Value>, String> {
    let Some(function) = shell.function(name) else {
        return Err(format!("undefined function '{name}'"));
    };

    if args.len() != function.params.len() {
        return Err(format!(
            "function '{name}' expected {} arguments, found {}",
            function.params.len(),
            args.len()
        ));
    }

    let values = args
        .iter()
        .map(|arg| evaluate_expression(shell, arg).map_err(|err| err.to_string()))
        .collect::<Result<Vec<_>, _>>()?;

    let caller_scopes = shell.isolate_function_scopes();

    let mut setup_error = None;
    for (param, value) in function.params.iter().zip(values) {
        if let Some(expected) = param.type_annotation
            && value.type_name() != expected
        {
            setup_error = Some(format!(
                "type mismatch: expected {expected}, found {}",
                value.type_name()
            ));
            break;
        }

        if let Err(err) = shell.declare_variable(&param.name, param.type_annotation, value) {
            setup_error = Some(err.to_string());
            break;
        }
    }

    if let Some(err) = setup_error {
        shell.restore_scopes(caller_scopes);
        return Err(err);
    }

    let flow = execute_block(shell, function.body);
    shell.restore_scopes(caller_scopes);

    match flow {
        ControlFlow::Return(value) => enforce_return_type(name, function.return_type, value),
        ControlFlow::Continue => {
            if let Some(return_type) = function.return_type {
                Err(format!("function '{name}' expected return {return_type}"))
            } else {
                Ok(None)
            }
        }
        ControlFlow::Break => Err("break outside loop".into()),
        ControlFlow::LoopContinue => Err("continue outside loop".into()),
        ControlFlow::Exit(code) => {
            shell.exit_code = code;
            Ok(None)
        }
    }
}

fn enforce_return_type(
    name: &str,
    return_type: Option<TypeName>,
    value: Option<Value>,
) -> Result<Option<Value>, String> {
    match (return_type, value) {
        (Some(expected), Some(value)) if value.type_name() == expected => Ok(Some(value)),
        (Some(expected), Some(value)) => Err(format!(
            "type mismatch: expected {expected}, found {}",
            value.type_name()
        )),
        (Some(expected), None) => Err(format!("function '{name}' expected return {expected}")),
        (None, value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::Expression;

    use super::*;

    #[test]
    fn function_call_expression_returns_value() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#,
        );
        run(&mut shell, "let total = add(2, 3)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("total".into())),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn inferred_parameter_accepts_the_argument_value_type() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn consume(value) {
    let copy = value
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "consume", &[Value::Int(7).into()]),
            Ok(None)
        );
        assert_eq!(
            execute_function_call(
                &mut shell,
                "consume",
                &[Expression::Literal(Value::String("crab".into()))],
            ),
            Ok(None)
        );
    }

    #[test]
    fn typed_parameter_rejects_the_wrong_argument_type() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn identity(value: int) -> int {
    return value
}
"#,
        );

        assert_eq!(
            execute_function_call(
                &mut shell,
                "identity",
                &[Expression::Literal(Value::String("crab".into()))],
            ),
            Err("type mismatch: expected int, found string".into())
        );
    }

    #[test]
    fn typed_function_rejects_the_wrong_return_type() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn wrong_type() -> int {
    return "crab"
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "wrong_type", &[]),
            Err("type mismatch: expected int, found string".into())
        );
    }

    #[test]
    fn typed_function_rejects_return_without_a_value() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn no_value() -> int {
    return
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "no_value", &[]),
            Err("function 'no_value' expected return int".into())
        );
    }

    #[test]
    fn typed_function_rejects_falling_through_without_a_return() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn falls_through() -> int {
    let value = 1
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "falls_through", &[]),
            Err("function 'falls_through' expected return int".into())
        );
    }

    #[test]
    fn function_invocation_uses_fresh_scope() {
        let mut shell = Shell::new();

        run(&mut shell, "let x = 10");
        run(
            &mut shell,
            r#"
fn test(x: int) -> int {
    let y = 5
    return x
}
"#,
        );
        run(&mut shell, "let result = test(20)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(10))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("result".into())),
            Ok(Value::Int(20))
        );
        assert!(matches!(
            shell.evaluate(&Expression::Identifier("y".into())),
            Err(ShellError::UndefinedVariable(_))
        ));
    }

    #[test]
    fn function_cannot_mutate_caller_local_scope() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn mutate() {
    x = 99
}
"#,
        );
        shell.push_scope();
        shell.declare_variable("x", None, Value::Int(1)).unwrap();

        let result = execute_function_call(&mut shell, "mutate", &[]);

        assert_eq!(result, Ok(None));
        assert_eq!(shell.exit_code, 2);
        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(1))
        );

        shell.pop_scope();
    }

    #[test]
    fn function_returns_from_nested_block() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn find_positive(x: int) -> int {
    if x > 0 {
        return x
    }

    return 0
}
"#,
        );
        run(&mut shell, "let positive = find_positive(3)");
        run(&mut shell, "let fallback = find_positive(0)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("positive".into())),
            Ok(Value::Int(3))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("fallback".into())),
            Ok(Value::Int(0))
        );
    }

    #[test]
    fn source_file_statements_preserve_block_units() {
        let statements = collect_source_statements(
            r#"
let x = 1

fn add(a: int, b: int) -> int {
    return a + b
}

let total = add(2, 3)
"#,
        );

        assert_eq!(statements.len(), 3);
        assert_eq!(statements[0].trim(), "let x = 1");
        assert!(statements[1].contains("fn add"));
        assert_eq!(statements[2].trim(), "let total = add(2, 3)");
    }

    #[test]
    fn runs_crb_script_file_in_single_shell_state() {
        let path = temp_script_path("runs_crb_script_file_in_single_shell_state");
        fs::write(
            &path,
            r#"
fn add(a: int, b: int) -> int {
    return a + b
}

let total = add(2, 3)
"#,
        )
        .unwrap();

        let mut shell = Shell::new();
        let code = run_script(&mut shell, path.to_str().unwrap());

        fs::remove_file(path).unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            shell.evaluate(&Expression::Identifier("total".into())),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn missing_config_file_is_ignored() {
        let mut shell = Shell::new();
        let path = temp_script_path("missing_config_file_is_ignored");

        assert_eq!(run_config_file(&mut shell, &path), None);
        assert_eq!(shell.exit_code, 0);
    }

    #[test]
    fn config_file_runs_in_interactive_shell_state() {
        let path = temp_config_path("config_file_runs_in_interactive_shell_state");
        fs::write(
            &path,
            r#"
let project = "crbsh"

fn add(a: int, b: int) -> int {
    return a + b
}

let total = add(2, 3)
"#,
        )
        .unwrap();

        let mut shell = Shell::new();
        let code = run_config_file(&mut shell, &path);

        fs::remove_file(path).unwrap();

        assert_eq!(code, Some(0));
        assert_eq!(
            shell.evaluate(&Expression::Identifier("project".into())),
            Ok(Value::String("crbsh".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("total".into())),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn rejects_non_crb_script_extension() {
        let mut shell = Shell::new();

        assert_eq!(run_script(&mut shell, "script.txt"), 2);
    }

    #[test]
    fn read_input_reports_end_of_input() {
        let mut input = io::Cursor::new(Vec::<u8>::new());

        assert_eq!(read_input_from(&mut input), Ok(None));
    }

    #[test]
    fn read_input_returns_partial_continuation_at_end_of_input() {
        let mut input = io::Cursor::new(b"if true {\n".as_slice());

        assert_eq!(read_input_from(&mut input), Ok(Some("if true {\n".into())));
    }

    #[test]
    fn glob_values_honor_directory_component() {
        let values = glob_values("src/*.rs");

        assert!(values.contains(&"src/main.rs".into()));
    }

    #[test]
    fn glob_values_reject_multiple_wildcards() {
        assert!(glob_values("src/*/*.rs").is_empty());
    }

    #[test]
    fn export_uses_native_variable_name() {
        let mut shell = Shell::new();

        run(&mut shell, r#"let project = "crbsh""#);
        run(&mut shell, "export project");

        assert_eq!(shell.environment_value("project").as_deref(), Some("crbsh"));
    }

    #[test]
    fn export_sets_environment_override() {
        let mut shell = Shell::new();

        run(&mut shell, r#"export CRBSH_TEST_EXPORT = "debug""#);

        assert_eq!(
            shell.environment_value("CRBSH_TEST_EXPORT").as_deref(),
            Some("debug")
        );
    }

    #[test]
    fn unset_removes_native_variable_and_environment_override() {
        let mut shell = Shell::new();

        run(&mut shell, "let retries = 3");
        run(&mut shell, r#"env.CRBSH_TEST_UNSET = "debug""#);
        run(&mut shell, "unset retries");
        run(&mut shell, "unset env.CRBSH_TEST_UNSET");

        assert_eq!(shell.variable_value("retries"), None);
        assert!(shell.environment_overrides().next().is_none());
    }

    #[test]
    fn and_connector_runs_next_pipeline_after_success() {
        let output = temp_output_path("and_connector_runs_next_pipeline_after_success", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(r#"true && print "build passed" > {}"#, output.display()),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "build passed\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn and_connector_skips_next_pipeline_after_failure() {
        let output = temp_output_path("and_connector_skips_next_pipeline_after_failure", "out");
        let mut shell = Shell::new();
        let parsed = parser::parse(&format!(
            r#"false && print "skipped" > {}"#,
            output.display()
        ))
        .unwrap();

        assert!(matches!(
            execute_input(&mut shell, parsed),
            ControlFlow::Continue
        ));

        assert_eq!(shell.exit_code, 1);
        assert!(!output.exists());
    }

    #[test]
    fn or_connector_runs_next_pipeline_after_failure() {
        let output = temp_output_path("or_connector_runs_next_pipeline_after_failure", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(r#"false || print "command failed" > {}"#, output.display()),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "command failed\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn conditional_connector_uses_previous_pipeline_exit_status() {
        let input = temp_output_path(
            "conditional_connector_uses_previous_pipeline_exit_status",
            "txt",
        );
        let output = temp_output_path(
            "conditional_connector_uses_previous_pipeline_exit_status",
            "out",
        );
        fs::write(&input, "blue\ncrab\n").unwrap();

        let mut shell = Shell::new();
        run(
            &mut shell,
            &format!(
                r#"cat {} | grep -q crab && print "found it" > {}"#,
                input.display(),
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "found it\n");
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn or_connector_skips_next_pipeline_after_success() {
        let output = temp_output_path("or_connector_skips_next_pipeline_after_success", "out");
        let mut shell = Shell::new();
        let parsed =
            parser::parse(&format!(r#"true || print "nope" > {}"#, output.display())).unwrap();

        assert!(matches!(
            execute_input(&mut shell, parsed),
            ControlFlow::Continue
        ));

        assert_eq!(shell.exit_code, 0);
        assert!(!output.exists());
    }

    #[test]
    fn and_chain_runs_left_to_right_after_successes() {
        let output = temp_output_path("and_chain_runs_left_to_right_after_successes", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(
                r#"true && print A > {} && print B >> {}"#,
                output.display(),
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "A\nB\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn or_chain_stops_after_first_successful_fallback() {
        let output = temp_output_path("or_chain_stops_after_first_successful_fallback", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(
                r#"false || print A > {} || print B >> {}"#,
                output.display(),
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "A\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn mixed_chain_false_and_then_or_runs_fallback() {
        let output = temp_output_path("mixed_chain_false_and_then_or_runs_fallback", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(
                r#"false && print A > {} || print B > {}"#,
                output.display(),
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "B\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn mixed_chain_true_or_then_and_runs_final_pipeline() {
        let output = temp_output_path("mixed_chain_true_or_then_and_runs_final_pipeline", "out");
        let mut shell = Shell::new();

        run(
            &mut shell,
            &format!(
                r#"true || print A > {} && print B > {}"#,
                output.display(),
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "B\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn multiline_pipeline_conditional_uses_previous_pipeline_status() {
        let output = temp_output_path(
            "multiline_pipeline_conditional_uses_previous_pipeline_status",
            "out",
        );

        let mut shell = Shell::new();
        run(
            &mut shell,
            &format!(
                "printf crab | grep -q crab &&\n    print found > {}",
                output.display()
            ),
        );

        assert_eq!(fs::read_to_string(&output).unwrap(), "found\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn alias_expands_command_position_and_preserves_arguments() {
        let output = temp_output_path(
            "alias_expands_command_position_and_preserves_arguments",
            "out",
        );
        let mut shell = Shell::new();

        run(&mut shell, r#"alias p = "print alias""#);
        run(&mut shell, &format!("p tail > {}", output.display()));

        assert_eq!(fs::read_to_string(&output).unwrap(), "alias tail\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn chained_aliases_expand_until_real_command() {
        let output = temp_output_path("chained_aliases_expand_until_real_command", "out");
        let mut shell = Shell::new();

        run(&mut shell, r#"alias l = "ll""#);
        run(&mut shell, r#"alias ll = "print long""#);
        run(&mut shell, &format!("l form > {}", output.display()));

        assert_eq!(fs::read_to_string(&output).unwrap(), "long form\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn alias_does_not_expand_non_command_arguments() {
        let output = temp_output_path("alias_does_not_expand_non_command_arguments", "out");
        let mut shell = Shell::new();

        run(&mut shell, r#"alias ll = "print expanded""#);
        run(&mut shell, &format!("print ll > {}", output.display()));

        assert_eq!(fs::read_to_string(&output).unwrap(), "ll\n");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn alias_cycle_sets_parse_style_failure_status() {
        let mut shell = Shell::new();

        run(&mut shell, r#"alias a = "b""#);
        run(&mut shell, r#"alias b = "a""#);

        let parsed = parser::parse("a").unwrap();

        assert!(matches!(
            execute_input(&mut shell, parsed),
            ControlFlow::Continue
        ));
        assert_eq!(shell.exit_code, 2);
    }

    fn run(shell: &mut Shell, input: &str) {
        let parsed = parser::parse(input).unwrap();
        assert!(matches!(
            execute_input(shell, parsed),
            ControlFlow::Continue
        ));
        assert_eq!(shell.exit_code, 0);
    }

    fn temp_script_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        path.push(format!("crbsh-{name}-{unique}.crb"));
        path
    }

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        path.push(format!("crbsh-{name}-{unique}.crbshrc"));
        path
    }

    fn temp_output_path(name: &str, extension: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        path.push(format!("crbsh-{name}-{unique}.{extension}"));
        path
    }
}
