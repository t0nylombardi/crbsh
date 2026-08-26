mod builtins;
mod execution;
mod history;
mod lexer;
mod parser;
mod prompt;
mod runtime;
mod shell;

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use runtime::{ControlFlow, execute_input};

#[cfg(test)]
use runtime::Value;
#[cfg(test)]
use runtime::{evaluate_expression, execute_function_call, glob_values};
use shell::Shell;

#[cfg(test)]
use shell::ShellError;

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
        ControlFlow::Error(error) => {
            eprintln!("crbsh: {error}");
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

#[cfg(test)]
mod tests {
    use crate::parser::Expression;

    use super::*;

    #[test]
    fn lists_work_across_declarations_functions_loops_indexing_and_len() {
        let mut shell = Shell::new();

        run(&mut shell, r#"let names = ["Tony", "Alice", "Bob"]"#);
        run(&mut shell, "let first = names[0]");
        run(&mut shell, "let count = names.len");
        run(
            &mut shell,
            r#"
fn last(items: list<string>) -> string {
    let result = ""
    for item in items {
        result = item
    }
    return result
}
"#,
        );
        run(&mut shell, r#"let final = last(["one", "two", "three"])"#);

        assert_eq!(
            shell.evaluate(&Expression::Identifier("first".into())),
            Ok(Value::String("Tony".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("count".into())),
            Ok(Value::Int(3))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("final".into())),
            Ok(Value::String("three".into()))
        );
    }

    #[test]
    fn indexing_composes_with_function_calls_and_larger_expressions() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn get_numbers() -> list<int> {
    return [20, 21, 22]
}
"#,
        );
        run(&mut shell, "let answer = get_numbers()[1] * 2");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("answer".into())),
            Ok(Value::Int(42))
        );
    }

    #[test]
    fn list_arguments_execute_in_procedures_and_for_loops() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn consume(items: list<string>) {
    for item in items {
        let copy = item
    }
}
"#,
        );
        run(&mut shell, r#"consume(["one", "two", "three"])"#);

        assert_eq!(shell.exit_code, 0);
    }

    #[test]
    fn typed_lists_accept_empty_values_and_reject_wrong_element_types() {
        let mut shell = Shell::new();

        run(&mut shell, "let empty: list<int> = []");
        assert_eq!(shell.exit_code, 0);
        let wrong = parser::parse(r#"let wrong: list<int> = ["nope"]"#).unwrap();
        assert!(matches!(
            execute_input(&mut shell, wrong),
            ControlFlow::Continue
        ));
        assert_eq!(shell.exit_code, 2);
    }

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
    fn nested_function_calls_compose_inside_larger_expressions() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#,
        );
        run(
            &mut shell,
            r#"
fn double(value: int) -> int {
    return value * 2
}
"#,
        );
        run(&mut shell, "let total = add(double(2), add(1, 2)) * 2");
        run(&mut shell, "let matches = add(2, 3) == 5");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("total".into())),
            Ok(Value::Int(14))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("matches".into())),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn nested_call_arguments_report_the_first_error() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#,
        );

        let expression = Expression::Call {
            name: "add".into(),
            args: vec![
                Expression::Call {
                    name: "first_missing".into(),
                    args: Vec::new(),
                },
                Expression::Call {
                    name: "second_missing".into(),
                    args: Vec::new(),
                },
            ],
        };

        let Err(error) = evaluate_expression(&mut shell, &expression) else {
            panic!("nested call should fail");
        };

        assert_eq!(error.to_string(), "undefined function 'first_missing'");
    }

    #[test]
    fn procedure_call_is_rejected_when_an_expression_requires_a_value() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn consume(value) {
    let copy = value
}
"#,
        );

        let expression = Expression::Call {
            name: "consume".into(),
            args: vec![Value::Int(7).into()],
        };
        let Err(error) = evaluate_expression(&mut shell, &expression) else {
            panic!("procedure call should not produce a value");
        };

        assert_eq!(
            error.to_string(),
            "function 'consume' did not return a value"
        );
    }

    #[test]
    fn recursive_function_calls_return_values() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn factorial(n: int) -> int {
    if n == 0 {
        return 1
    }

    return n * factorial(n - 1)
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "factorial", &[Value::Int(5).into()]),
            Ok(Some(Value::Int(120)))
        );
    }

    #[test]
    fn recursion_limit_returns_an_error_and_restores_call_state() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn recurse() -> int {
    return recurse()
}
"#,
        );
        run(
            &mut shell,
            r#"
fn identity(value: int) -> int {
    return value
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "recurse", &[]),
            Err("function recursion limit of 100 exceeded while calling 'recurse'".into())
        );
        assert_eq!(
            execute_function_call(&mut shell, "identity", &[Value::Int(7).into()]),
            Ok(Some(Value::Int(7)))
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
    fn function_scope_shadows_globals_and_does_not_leak_locals() {
        let mut shell = Shell::new();

        run(&mut shell, "let x = 10");
        run(
            &mut shell,
            r#"
fn example(x: int) {
    let local = 20
    let parameter_copy = x
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "example", &[Value::Int(5).into()]),
            Ok(None)
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(10))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("local".into())),
            Err(ShellError::UndefinedVariable("local".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("parameter_copy".into())),
            Err(ShellError::UndefinedVariable("parameter_copy".into()))
        );
    }

    #[test]
    fn nested_blocks_layer_on_function_scope() {
        let mut shell = Shell::new();

        run(&mut shell, "let global = 10");
        run(
            &mut shell,
            r#"
fn test(x: int) -> int {
    let a = 1

    if true {
        let b = 2
        let inside = global + x + a + b
    }

    return global + x + a
}
"#,
        );

        assert_eq!(
            execute_function_call(&mut shell, "test", &[Value::Int(5).into()]),
            Ok(Some(Value::Int(16)))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("a".into())),
            Err(ShellError::UndefinedVariable("a".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("b".into())),
            Err(ShellError::UndefinedVariable("b".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("inside".into())),
            Err(ShellError::UndefinedVariable("inside".into()))
        );
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
