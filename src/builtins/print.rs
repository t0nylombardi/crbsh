use crate::shell::Shell;

use super::{BuiltinOutcome, BuiltinResult};

pub fn run(_shell: &mut Shell, args: &[String]) -> BuiltinResult {
    println!("{}", args.join(" "));

    Ok(BuiltinOutcome::Continue)
}
