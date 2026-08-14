use std::collections::HashMap;

use crate::shell::Shell;

use super::BuiltinResult;

pub type BuiltinFn = fn(&mut Shell, &[String]) -> BuiltinResult;

pub struct BuiltinRegistry {
    commands: HashMap<&'static str, BuiltinFn>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };

        registry.register("cd", super::cd::run);
        registry.register("exit", super::exit::run);
        registry.register("print", super::print::run);

        registry
    }

    pub fn register(&mut self, name: &'static str, command: BuiltinFn) {
        self.commands.insert(name, command);
    }

    pub fn get(&self, name: &str) -> Option<BuiltinFn> {
        self.commands.get(name).copied()
    }
}
