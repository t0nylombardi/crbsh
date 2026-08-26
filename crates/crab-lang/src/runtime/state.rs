use crate::parser::FunctionDefinition;

use super::{FunctionRegistry, ScopeError, ScopeStack, TypeName, Value};

pub struct LanguageRuntime {
    scopes: ScopeStack,
    functions: FunctionRegistry,
}

impl Default for LanguageRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageRuntime {
    pub fn new() -> Self {
        Self {
            scopes: ScopeStack::new(),
            functions: FunctionRegistry::new(),
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push();
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn enter_function_scope(&mut self) -> ScopeStack {
        self.scopes.enter_function()
    }

    pub fn restore_caller_scopes(&mut self, scopes: ScopeStack) {
        self.scopes = scopes;
    }

    pub fn declare_variable(
        &mut self,
        name: String,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ScopeError> {
        self.scopes.declare(name, type_annotation, value)
    }

    pub fn assign_variable(&mut self, name: String, value: Value) -> Result<(), ScopeError> {
        self.scopes.assign(name, value)
    }

    pub fn set_variable(&mut self, name: String, value: Value) {
        self.scopes.set(name, value);
    }

    pub fn variables(&self) -> Vec<(String, Value)> {
        self.scopes.visible_values()
    }

    pub fn variable_value(&self, name: &str) -> Option<Value> {
        self.scopes.value(name)
    }

    pub fn remove_variable(&mut self, name: &str) -> bool {
        self.scopes.remove(name)
    }

    pub fn define_function(&mut self, name: String, definition: FunctionDefinition) {
        self.functions.define(name, definition);
    }

    pub fn function(&self, name: &str) -> Option<FunctionDefinition> {
        self.functions.get(name)
    }

    pub fn enter_function_call(&mut self) -> Result<(), usize> {
        self.functions.enter_call()
    }

    pub fn exit_function_call(&mut self) {
        self.functions.exit_call();
    }
}
