use std::env;

pub fn run(args: &[&str]) {
    let target = match args.first() {
        Some(path) => *path,
        None => match env::var("HOME") {
            Ok(home) => {
                if let Err(err) = env::set_current_dir(home) {
                    eprintln!("crbsh: cd: {err}");
                }
                return;
            }
            Err(_) => {
                eprintln!("crbsh: cd: HOME is not set");
                return;
            }
        },
    };

    if let Err(err) = env::set_current_dir(target) {
        eprintln!("crbsh: cd: {target}: {err}");
    }
}
