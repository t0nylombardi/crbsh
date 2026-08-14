use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(_shell: &mut Shell, args: &[String]) -> BuiltinResult {
    let code = match args.first() {
        Some(value) => value
            .parse::<i32>()
            .map_err(|_| BuiltinError::new("exit: expected numeric exit code"))?,
        None => 0,
    };

    Ok(BuiltinOutcome::Exit(code))
}
