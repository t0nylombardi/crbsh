use std::collections::HashMap;
use std::fmt;

use crate::builtins::registry::BuiltinRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeValue {
    String(String),
    Int(i64),
}

impl NativeValue {
    pub fn parse(input: &str) -> Self {
        input
            .parse::<i64>()
            .map(Self::Int)
            .unwrap_or_else(|_| Self::String(input.into()))
    }
}

impl fmt::Display for NativeValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => write!(formatter, "{value}"),
            Self::Int(value) => write!(formatter, "{value}"),
        }
    }
}

pub struct Shell {
    pub builtins: BuiltinRegistry,
    pub exit_code: i32,
    variables: HashMap<String, NativeValue>,
    environment: HashMap<String, String>,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            exit_code: 0,
            variables: HashMap::new(),
            environment: HashMap::new(),
        }
    }

    pub fn set_variable(&mut self, name: impl Into<String>, value: NativeValue) {
        self.variables.insert(name.into(), value);
    }

    pub fn set_environment(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(name.into(), value.into());
    }

    pub fn environment_overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn resolve_word(&self, word: &str) -> String {
        if word == "status" {
            return self.exit_code.to_string();
        }

        if let Some(name) = word.strip_prefix('@') {
            return self.environment_value(name).unwrap_or_default();
        }

        if let Some(name) = word.strip_prefix("env.") {
            if name.is_empty() {
                return word.into();
            }

            return self.environment_value(name).unwrap_or_default();
        }

        self.variables
            .get(word)
            .map(ToString::to_string)
            .unwrap_or_else(|| word.into())
    }

    // Reserved for future line-editor completion; `env.` is a namespace prefix,
    // not an executable command.
    #[allow(dead_code)]
    pub fn complete_environment_names(&self, prefix: &str) -> Vec<String> {
        let mut names = std::env::vars()
            .map(|(name, _)| name)
            .chain(self.environment.keys().cloned())
            .filter(|name| name.starts_with(prefix))
            .collect::<Vec<_>>();

        names.sort();
        names.dedup();
        names
    }

    fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeValue, Shell};

    #[test]
    fn resolves_native_variables() {
        let mut shell = Shell::new();

        shell.set_variable("project", NativeValue::String("crbsh".into()));
        shell.set_variable("retries", NativeValue::Int(3));

        assert_eq!(shell.resolve_word("project"), "crbsh");
        assert_eq!(shell.resolve_word("retries"), "3");
    }

    #[test]
    fn resolves_status() {
        let mut shell = Shell::new();

        shell.exit_code = 127;

        assert_eq!(shell.resolve_word("status"), "127");
    }

    #[test]
    fn resolves_environment_overrides() {
        let mut shell = Shell::new();

        shell.set_environment("CRBSH_TEST_ENV", "debug");

        assert_eq!(shell.resolve_word("@CRBSH_TEST_ENV"), "debug");
        assert_eq!(shell.resolve_word("env.CRBSH_TEST_ENV"), "debug");
    }

    #[test]
    fn leaves_environment_namespace_prefix_as_literal_word() {
        let shell = Shell::new();

        assert_eq!(shell.resolve_word("env."), "env.");
    }
}
