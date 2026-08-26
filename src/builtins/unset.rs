use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    let [name] = args else {
        return Err(BuiltinError::new("unset: expected one argument"));
    };

    if let Some(name) = name.strip_prefix('@') {
        shell.unset_environment(name);
        return Ok(BuiltinOutcome::Continue);
    }

    if let Some(name) = name.strip_prefix("env.") {
        shell.unset_environment(name);
        return Ok(BuiltinOutcome::Continue);
    }

    shell.unset_variable(name);

    Ok(BuiltinOutcome::Continue)
}

#[cfg(test)]
mod tests {
    use crate::runtime::Value;

    use super::*;

    #[test]
    fn removes_native_variable() {
        let mut shell = Shell::new();
        shell.set_variable("retries", Value::Int(3));

        assert!(matches!(
            run(&mut shell, &["retries".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert_eq!(shell.variable_value("retries"), None);
    }

    #[test]
    fn removes_environment_override_with_env_prefix() {
        let mut shell = Shell::new();
        shell.set_environment("RUST_LOG", "debug");

        assert!(matches!(
            run(&mut shell, &["env.RUST_LOG".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert!(shell.environment_overrides().next().is_none());
    }

    #[test]
    fn removes_environment_override_with_at_prefix() {
        let mut shell = Shell::new();
        shell.set_environment("RUST_LOG", "debug");

        assert!(matches!(
            run(&mut shell, &["@RUST_LOG".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert!(shell.environment_overrides().next().is_none());
    }
}
