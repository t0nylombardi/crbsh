use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    let [name] = args else {
        return Err(BuiltinError::new("unalias: expected one alias name"));
    };

    if shell.unset_alias(name) {
        Ok(BuiltinOutcome::Continue)
    } else {
        Err(BuiltinError::new(format!(
            "unalias: '{name}' is not defined"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_alias() {
        let mut shell = Shell::new();

        shell.set_alias("ll", "ls -la").unwrap();

        assert!(matches!(
            run(&mut shell, &["ll".into()]),
            Ok(BuiltinOutcome::Continue)
        ));
        assert_eq!(shell.alias_value("ll"), None);
    }
}
