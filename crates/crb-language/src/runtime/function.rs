use std::collections::HashMap;

use crate::parser::FunctionDefinition;

const MAX_FUNCTION_CALL_DEPTH: usize = 100;

pub(crate) struct FunctionRegistry {
    definitions: HashMap<String, FunctionDefinition>,
    call_depth: usize,
}

impl FunctionRegistry {
    pub(crate) fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            call_depth: 0,
        }
    }

    pub(crate) fn define(&mut self, name: String, definition: FunctionDefinition) {
        self.definitions.insert(name, definition);
    }

    pub(crate) fn get(&self, name: &str) -> Option<FunctionDefinition> {
        self.definitions.get(name).cloned()
    }

    pub(crate) fn enter_call(&mut self) -> Result<(), usize> {
        if self.call_depth >= MAX_FUNCTION_CALL_DEPTH {
            return Err(MAX_FUNCTION_CALL_DEPTH);
        }

        self.call_depth += 1;
        Ok(())
    }

    pub(crate) fn exit_call(&mut self) {
        debug_assert!(self.call_depth > 0);
        self.call_depth = self.call_depth.saturating_sub(1);
    }
}
