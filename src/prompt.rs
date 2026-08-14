use std::env;

pub fn render() -> String {
    let cwd = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

    format!("crbsh:{cwd}> ")
}
