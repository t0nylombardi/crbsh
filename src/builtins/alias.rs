use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    match args {
        [] => {
            for (name, value) in shell.aliases() {
                println!("{name} = {value}");
            }

            Ok(BuiltinOutcome::Continue)
        }
        [name] => {
            let Some(value) = shell.alias_value(name) else {
                return Err(BuiltinError::new(format!("alias: '{name}' is not defined")));
            };

            println!("{name} = {value}");

            Ok(BuiltinOutcome::Continue)
        }
        [name, operator, value] if operator == "=" => {
            shell
                .set_alias(name, value)
                .map_err(|err| BuiltinError::new(format!("alias: {err}")))?;

            Ok(BuiltinOutcome::Continue)
        }
        _ => Err(BuiltinError::new("alias: expected alias NAME = VALUE")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_alias_with_explicit_assignment_operator() {
        let mut shell = Shell::new();

        assert!(matches!(
            run(&mut shell, &["ll".into(), "=".into(), "ls -la".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert_eq!(shell.alias_value("ll"), Some("ls -la".into()));
    }

    #[test]
    fn rejects_invalid_replacement() {
        let mut shell = Shell::new();

        let error = run(&mut shell, &["bad".into(), "=".into(), "ls | wc".into()]).unwrap_err();

        assert_eq!(
            error.message,
            "alias: alias replacement must be a single command without redirection"
        );
    }
}
