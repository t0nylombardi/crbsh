use std::collections::HashMap;

use crate::runtime::TypeName;

/// Lexical type bindings visible while checking a program.
#[derive(Debug, Clone)]
pub struct TypeContext {
    scopes: Vec<HashMap<String, TypeName>>,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    /// Creates a context containing one global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Opens a nested lexical scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Closes the innermost lexical scope, preserving the global scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Declares a type in the current scope.
    pub fn declare(&mut self, name: String, type_name: TypeName) -> Result<(), ContextError> {
        let scope = self
            .scopes
            .last_mut()
            .expect("type context always has a global scope");

        if scope.contains_key(&name) {
            return Err(ContextError::AlreadyDefined(name));
        }

        scope.insert(name, type_name);
        Ok(())
    }

    /// Resolves a name from the innermost lexical scope outward.
    pub fn resolve(&self, name: &str) -> Option<&TypeName> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// Returns an isolated function context containing the current globals.
    pub(crate) fn function_context(&self) -> Self {
        let global = self.scopes.first().cloned().unwrap_or_default();
        Self {
            scopes: vec![global, HashMap::new()],
        }
    }
}

/// Failure to modify a lexical type context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    AlreadyDefined(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_nearest_lexical_binding() {
        let mut context = TypeContext::new();
        context.declare("value".into(), TypeName::Int).unwrap();
        context.push_scope();
        context.declare("value".into(), TypeName::String).unwrap();

        assert_eq!(context.resolve("value"), Some(&TypeName::String));

        context.pop_scope();
        assert_eq!(context.resolve("value"), Some(&TypeName::Int));
    }
}
