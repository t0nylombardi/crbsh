use crate::shell::Shell;

use super::{BuiltinOutcome, BuiltinResult};

pub fn run(_shell: &mut Shell, args: &[String]) -> BuiltinResult {
    print!("{}", output(args));

    Ok(BuiltinOutcome::Continue)
}

pub fn output(args: &[String]) -> String {
    format!("{}\n", args.join(" "))
}
