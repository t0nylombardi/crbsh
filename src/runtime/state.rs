use crate::parser::FunctionDefinition;

use super::{FunctionRegistry, ScopeError, ScopeStack, TypeName, Value};

pub(crate) struct LanguageRuntime {
    scopes: ScopeStack,
    functions: FunctionRegistry,
}

impl LanguageRuntime {
    pub(crate) fn new() -> Self {
        Self {
            scopes: ScopeStack::new(),
            functions: FunctionRegistry::new(),
        }
    }

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push();
    }

    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(crate) fn enter_function_scope(&mut self) -> ScopeStack {
        self.scopes.enter_function()
    }

    pub(crate) fn restore_caller_scopes(&mut self, scopes: ScopeStack) {
        self.scopes = scopes;
    }

    pub(crate) fn declare_variable(
        &mut self,
        name: String,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ScopeError> {
        self.scopes.declare(name, type_annotation, value)
    }

    pub(crate) fn assign_variable(&mut self, name: String, value: Value) -> Result<(), ScopeError> {
        self.scopes.assign(name, value)
    }

    #[cfg(test)]
    pub(crate) fn set_variable(&mut self, name: String, value: Value) {
        self.scopes.set(name, value);
    }

    pub(crate) fn variables(&self) -> Vec<(String, Value)> {
        self.scopes.visible_values()
    }

    pub(crate) fn variable_value(&self, name: &str) -> Option<Value> {
        self.scopes.value(name)
    }

    pub(crate) fn remove_variable(&mut self, name: &str) -> bool {
        self.scopes.remove(name)
    }

    pub(crate) fn define_function(&mut self, name: String, definition: FunctionDefinition) {
        self.functions.define(name, definition);
    }

    pub(crate) fn function(&self, name: &str) -> Option<FunctionDefinition> {
        self.functions.get(name)
    }

    pub(crate) fn enter_function_call(&mut self) -> Result<(), usize> {
        self.functions.enter_call()
    }

    pub(crate) fn exit_function_call(&mut self) {
        self.functions.exit_call();
    }
}
