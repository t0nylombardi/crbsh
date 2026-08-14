use std::env;

use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(_shell: &mut Shell, args: &[String]) -> BuiltinResult {
    let target = match args.first() {
        Some(path) => path.clone(),
        None => env::var("HOME").map_err(|_| BuiltinError::new("HOME is not set"))?,
    };

    env::set_current_dir(&target)
        .map_err(|err| BuiltinError::new(format!("cd: {target}: {err}")))?;

    Ok(BuiltinOutcome::Continue)
}
