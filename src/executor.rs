use std::io;
use std::process::Command;

pub fn execute_external(command: &str, args: &[String]) -> io::Result<i32> {
    let status = Command::new(command).args(args).status()?;

    Ok(status.code().unwrap_or(1))
}
