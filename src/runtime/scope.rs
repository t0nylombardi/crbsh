use std::collections::HashMap;

use super::{TypeName, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeError {
    AlreadyDefined(String),
    NotDefined(String),
    TypeMismatch { expected: TypeName, found: TypeName },
}

#[derive(Clone)]
struct Variable {
    value: Value,
    type_annotation: Option<TypeName>,
}

pub(crate) struct ScopeStack {
    frames: Vec<HashMap<String, Variable>>,
}

impl ScopeStack {
    pub(crate) fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
        }
    }

    pub(crate) fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    pub(crate) fn enter_function(&mut self) -> Self {
        let global = self.frames.first().cloned().unwrap_or_default();
        std::mem::replace(
            self,
            Self {
                frames: vec![global, HashMap::new()],
            },
        )
    }

    pub(crate) fn declare(
        &mut self,
        name: String,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ScopeError> {
        let frame = self
            .frames
            .last_mut()
            .expect("scope stack always has a global frame");

        if frame.contains_key(&name) {
            return Err(ScopeError::AlreadyDefined(name));
        }

        if let Some(expected) = &type_annotation {
            let found = value.type_name();
            if !expected.accepts(&found) {
                return Err(ScopeError::TypeMismatch {
                    expected: expected.clone(),
                    found,
                });
            }
        }

        frame.insert(
            name,
            Variable {
                value,
                type_annotation,
            },
        );
        Ok(())
    }

    pub(crate) fn assign(&mut self, name: String, value: Value) -> Result<(), ScopeError> {
        let Some(variable) = self
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.get_mut(&name))
        else {
            return Err(ScopeError::NotDefined(name));
        };

        let expected = variable
            .type_annotation
            .clone()
            .unwrap_or_else(|| variable.value.type_name());
        let found = value.type_name();
        if !expected.accepts(&found) {
            return Err(ScopeError::TypeMismatch { expected, found });
        }

        variable.value = value;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set(&mut self, name: String, value: Value) {
        self.frames
            .last_mut()
            .expect("scope stack always has a global frame")
            .insert(
                name,
                Variable {
                    value,
                    type_annotation: None,
                },
            );
    }

    pub(crate) fn value(&self, name: &str) -> Option<Value> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).map(|variable| variable.value.clone()))
    }

    pub(crate) fn visible_values(&self) -> Vec<(String, Value)> {
        let mut values = HashMap::new();
        for frame in &self.frames {
            for (name, variable) in frame {
                values.insert(name.clone(), variable.value.clone());
            }
        }

        let mut values = values.into_iter().collect::<Vec<_>>();
        values.sort_by(|(left, _), (right, _)| left.cmp(right));
        values
    }

    pub(crate) fn remove(&mut self, name: &str) -> bool {
        self.frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.remove(name))
            .is_some()
    }
}
