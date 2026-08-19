use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    let entries = match args {
        [] => shell.history.entries(),
        [count] => {
            let count = count
                .parse::<usize>()
                .map_err(|_| BuiltinError::new("history: count must be a positive integer"))?;

            shell.history.recent(count)
        }
        _ => return Err(BuiltinError::new("history: expected zero or one argument")),
    };

    let offset = shell.history.entries().len().saturating_sub(entries.len());

    for (index, entry) in entries.iter().enumerate() {
        println!("{:>4}  {}", offset + index + 1, entry);
    }

    Ok(BuiltinOutcome::Continue)
}
