mod evaluator;

pub use crb_language::runtime::{
    LanguageRuntime, ScopeError, ScopeStack, TypeName, Value, ValueStream,
};
pub(crate) use evaluator::{ControlFlow, execute_input};
#[cfg(test)]
pub(crate) use evaluator::{evaluate_expression, execute_function_call, glob_values};
