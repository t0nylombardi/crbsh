use crate::builtins::registry::BuiltinRegistry;

pub struct Shell {
    pub builtins: BuiltinRegistry,
    pub exit_code: i32,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            exit_code: 0,
        }
    }
}
