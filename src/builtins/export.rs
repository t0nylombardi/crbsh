use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    match args {
        [name] => {
            shell
                .export_variable(name)
                .map_err(|err| BuiltinError::new(format!("export: {err}")))?;

            Ok(BuiltinOutcome::Continue)
        }
        [name, operator, value] if operator == "=" => {
            shell.set_environment(name, value);

            Ok(BuiltinOutcome::Continue)
        }
        [] => Err(BuiltinError::new("export: expected variable name")),
        _ => Err(BuiltinError::new("export: expected NAME or NAME = VALUE")),
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::Value;

    use super::*;

    #[test]
    fn exports_existing_native_variable() {
        let mut shell = Shell::new();
        shell.set_variable("project", Value::String("crbsh".into()));

        assert!(matches!(
            run(&mut shell, &["project".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert_eq!(shell.environment_value("project").as_deref(), Some("crbsh"));
    }

    #[test]
    fn sets_environment_override() {
        let mut shell = Shell::new();

        assert!(matches!(
            run(&mut shell, &["RUST_LOG".into(), "=".into(), "debug".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert_eq!(
            shell.environment_value("RUST_LOG").as_deref(),
            Some("debug")
        );
    }

    #[test]
    fn rejects_unknown_native_variable() {
        let mut shell = Shell::new();

        let error = run(&mut shell, &["missing".into()]).unwrap_err();

        assert_eq!(error.message, "export: variable 'missing' is not defined");
    }
}
