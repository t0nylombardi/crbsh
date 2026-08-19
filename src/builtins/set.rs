use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    match args {
        [] => {
            for (name, value) in shell.variables() {
                println!("{name} = {value}");
            }

            Ok(BuiltinOutcome::Continue)
        }
        [name] => {
            let Some(value) = shell.variable_value(name) else {
                return Err(BuiltinError::new(format!(
                    "set: variable '{name}' is not defined"
                )));
            };

            println!("{name} = {value}");

            Ok(BuiltinOutcome::Continue)
        }
        _ => Err(BuiltinError::new("set: expected zero or one argument")),
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;

    use super::*;

    #[test]
    fn rejects_unknown_variable() {
        let mut shell = Shell::new();

        let error = run(&mut shell, &["missing".into()]).unwrap_err();

        assert_eq!(error.message, "set: variable 'missing' is not defined");
    }

    #[test]
    fn inspects_known_variable() {
        let mut shell = Shell::new();
        shell.set_variable("project", Value::String("crbsh".into()));

        assert!(matches!(
            run(&mut shell, &["project".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
    }
}
