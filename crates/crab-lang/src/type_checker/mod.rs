mod context;
mod diagnostic;

use std::collections::{HashMap, HashSet};

pub use context::{ContextError, TypeContext};
pub use diagnostic::{TypeDiagnostic, TypeDiagnosticKind};

use crate::parser::{
    BinaryOperator, Expression, FunctionDefinition, Iterable, MatchPattern, ParsedInput, Pipeline,
};
use crate::runtime::TypeName;

#[derive(Debug, Clone)]
struct FunctionType {
    params: Vec<Option<TypeName>>,
    return_type: Option<TypeName>,
}

/// Traverses Crab AST nodes and reports type errors without evaluation.
#[derive(Debug)]
pub struct TypeChecker {
    context: TypeContext,
    unknowns: Vec<HashSet<String>>,
    functions: HashMap<String, FunctionType>,
    diagnostics: Vec<TypeDiagnostic>,
    expected_return: Option<TypeName>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    /// Creates a checker with an empty global type context.
    pub fn new() -> Self {
        Self {
            context: TypeContext::new(),
            unknowns: vec![HashSet::new()],
            functions: HashMap::new(),
            diagnostics: Vec::new(),
            expected_return: None,
        }
    }

    /// Creates a checker with caller-provided lexical type bindings.
    pub fn with_context(context: TypeContext) -> Self {
        Self {
            context,
            ..Self::new()
        }
    }

    /// Returns the lexical type context used for expression inference.
    pub fn context(&self) -> &TypeContext {
        &self.context
    }

    /// Checks a complete sequence of parsed inputs and returns every safe diagnostic.
    pub fn check(program: &[ParsedInput]) -> Result<(), Vec<TypeDiagnostic>> {
        let mut checker = Self::new();
        checker.register_functions(program);
        checker.check_statements(program);

        if checker.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(checker.diagnostics)
        }
    }

    /// Infers the type of one expression using this checker's current context.
    pub fn infer_expression(
        &mut self,
        expression: &Expression,
    ) -> Result<TypeName, TypeDiagnostic> {
        let diagnostic_start = self.diagnostics.len();
        let inferred = self.infer(expression);

        match inferred {
            Some(type_name) if self.diagnostics.len() == diagnostic_start => Ok(type_name),
            _ if self.diagnostics.len() > diagnostic_start => {
                Err(self.diagnostics.remove(diagnostic_start))
            }
            _ => Err(TypeDiagnostic::new(TypeDiagnosticKind::NonValueFunction(
                "expression".into(),
            ))),
        }
    }

    fn register_functions(&mut self, statements: &[ParsedInput]) {
        for statement in statements {
            if let ParsedInput::FunctionDefinition { name, definition } = statement {
                self.functions.insert(
                    name.clone(),
                    FunctionType {
                        params: definition
                            .params
                            .iter()
                            .map(|param| param.type_annotation.clone())
                            .collect(),
                        return_type: definition.return_type.clone(),
                    },
                );
            }
        }
    }

    fn check_statements(&mut self, statements: &[ParsedInput]) {
        for statement in statements {
            self.check_statement(statement);
        }
    }

    fn check_statement(&mut self, statement: &ParsedInput) {
        match statement {
            ParsedInput::Let {
                name,
                type_annotation,
                value,
            } => self.check_declaration(name, type_annotation.as_ref(), value),
            ParsedInput::Assignment { name, value } => self.check_assignment(name, value),
            ParsedInput::EnvironmentAssignment { value, .. } => {
                self.infer(value);
            }
            ParsedInput::Return { value } => self.check_return(value.as_ref()),
            ParsedInput::If {
                branches,
                else_body,
            } => {
                for branch in branches {
                    self.expect(
                        &branch.condition,
                        TypeName::Bool,
                        TypeDiagnosticKind::Condition,
                    );
                    self.check_block(&branch.body);
                }
                if let Some(body) = else_body {
                    self.check_block(body);
                }
            }
            ParsedInput::Match { value, arms } => {
                let matched = self.infer(value);
                for arm in arms {
                    self.check_pattern(&arm.pattern, matched.as_ref());
                    self.check_block(std::slice::from_ref(&arm.body));
                }
            }
            ParsedInput::While { condition, body } => {
                self.expect(condition, TypeName::Bool, TypeDiagnosticKind::Condition);
                self.check_block(body);
            }
            ParsedInput::For {
                name,
                iterable,
                body,
            } => self.check_for(name, iterable, body),
            ParsedInput::FunctionDefinition { definition, .. } => self.check_function(definition),
            ParsedInput::Pipeline(pipeline) | ParsedInput::BackgroundPipeline { pipeline, .. } => {
                self.check_pipeline(pipeline)
            }
            ParsedInput::PipelineChain { first, rest } => {
                self.check_pipeline(first);
                for (_, pipeline) in rest {
                    self.check_pipeline(pipeline);
                }
            }
            ParsedInput::Break | ParsedInput::Continue => {}
        }
    }

    fn check_declaration(&mut self, name: &str, annotation: Option<&TypeName>, value: &Expression) {
        let diagnostic_start = self.diagnostics.len();
        let Some(found) = self.infer(value) else {
            if self.diagnostics.len() == diagnostic_start {
                self.declare_unknown(name.into());
            }
            return;
        };
        let declared = annotation.cloned().unwrap_or_else(|| found.clone());

        if !declared.accepts(&found) {
            self.diagnostics.push(TypeDiagnostic::mismatch(
                TypeDiagnosticKind::Declaration(name.into()),
                declared,
                found,
            ));
            return;
        }

        if self.context.declare(name.into(), declared).is_err() {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::AlreadyDefined(
                    name.into(),
                )));
        }
    }

    fn check_assignment(&mut self, name: &str, value: &Expression) {
        let expected = self.context.resolve(name).cloned();
        let found = self.infer(value);

        match (expected, found) {
            (None, _) if !self.is_unknown(name) => {
                self.diagnostics
                    .push(TypeDiagnostic::new(TypeDiagnosticKind::UndefinedVariable(
                        name.into(),
                    )))
            }
            (Some(expected), Some(found)) if !expected.accepts(&found) => {
                self.diagnostics.push(TypeDiagnostic::mismatch(
                    TypeDiagnosticKind::Assignment(name.into()),
                    expected,
                    found,
                ));
            }
            _ => {}
        }
    }

    fn check_return(&mut self, value: Option<&Expression>) {
        match (self.expected_return.clone(), value) {
            (Some(expected), Some(value)) => {
                self.expect(
                    value,
                    expected,
                    TypeDiagnosticKind::Declaration("return".into()),
                );
            }
            (Some(_), None) => {
                self.diagnostics
                    .push(TypeDiagnostic::new(TypeDiagnosticKind::MissingReturnValue(
                        "function".into(),
                    )))
            }
            (None, Some(value)) => {
                self.infer(value);
                self.diagnostics.push(TypeDiagnostic::new(
                    TypeDiagnosticKind::UnexpectedReturnValue,
                ));
            }
            (None, None) => {}
        }
    }

    fn check_function(&mut self, definition: &FunctionDefinition) {
        let function_context = self.context.function_context();
        let caller_context = std::mem::replace(&mut self.context, function_context);
        let function_unknowns = vec![
            self.unknowns.first().cloned().unwrap_or_default(),
            HashSet::new(),
        ];
        let caller_unknowns = std::mem::replace(&mut self.unknowns, function_unknowns);
        let caller_return = self
            .expected_return
            .replace_with(definition.return_type.clone());

        for param in &definition.params {
            if let Some(type_name) = &param.type_annotation {
                if self
                    .context
                    .declare(param.name.clone(), type_name.clone())
                    .is_err()
                {
                    self.diagnostics
                        .push(TypeDiagnostic::new(TypeDiagnosticKind::AlreadyDefined(
                            param.name.clone(),
                        )));
                }
            } else {
                self.declare_unknown(param.name.clone());
            }
        }
        self.check_statements(&definition.body);

        self.context = caller_context;
        self.unknowns = caller_unknowns;
        self.expected_return = caller_return;
    }

    fn check_for(&mut self, name: &str, iterable: &Iterable, body: &[ParsedInput]) {
        let diagnostic_start = self.diagnostics.len();
        let element_type = match iterable {
            Iterable::Range { start, end, .. } => {
                self.expect(start, TypeName::Int, TypeDiagnosticKind::RangeBound);
                self.expect(end, TypeName::Int, TypeDiagnosticKind::RangeBound);
                Some(TypeName::Int)
            }
            Iterable::Glob(_) => Some(TypeName::String),
            Iterable::Expression(expression) => match self.infer(expression) {
                Some(TypeName::List(element)) => element.map(|element| *element),
                Some(found) => {
                    self.diagnostics.push(TypeDiagnostic::mismatch(
                        TypeDiagnosticKind::Iterable,
                        TypeName::List(None),
                        found,
                    ));
                    None
                }
                None => None,
            },
        };

        self.context.push_scope();
        self.unknowns.push(HashSet::new());
        if let Some(element_type) = element_type {
            let _ = self.context.declare(name.into(), element_type);
        } else if self.diagnostics.len() == diagnostic_start {
            self.declare_unknown(name.into());
        }
        self.check_statements(body);
        self.unknowns.pop();
        self.context.pop_scope();
    }

    fn check_block(&mut self, statements: &[ParsedInput]) {
        self.context.push_scope();
        self.unknowns.push(HashSet::new());
        self.check_statements(statements);
        self.unknowns.pop();
        self.context.pop_scope();
    }

    fn check_pipeline(&mut self, pipeline: &Pipeline) {
        for command in &pipeline.commands {
            if let Some(function) = self.functions.get(&command.name).cloned() {
                self.check_call(&command.name, &command.args, &function);
            }
        }
    }

    fn infer(&mut self, expression: &Expression) -> Option<TypeName> {
        match expression {
            Expression::Literal(value) => Some(value.type_name()),
            Expression::Identifier(name) => self.context.resolve(name).cloned().or_else(|| {
                if !self.is_unknown(name) {
                    self.diagnostics.push(TypeDiagnostic::new(
                        TypeDiagnosticKind::UndefinedVariable(name.clone()),
                    ));
                }
                None
            }),
            Expression::EnvironmentVariable(_) => Some(TypeName::String),
            Expression::Status => Some(TypeName::Int),
            Expression::Call { name, args } => self.infer_call(name, args),
            Expression::List(values) => self.infer_list(values),
            Expression::Index { target, index } => self.infer_index(target, index),
            Expression::Match { value, arms } => self.infer_match(value, arms),
            Expression::Len(target) => self.infer_len(target),
            Expression::Binary {
                left,
                operator,
                right,
            } => self.infer_binary(left, *operator, right),
        }
    }

    fn infer_list(&mut self, values: &[Expression]) -> Option<TypeName> {
        let mut element_type: Option<TypeName> = None;
        for value in values {
            let Some(found) = self.infer(value) else {
                continue;
            };
            if let Some(expected) = &element_type {
                if !expected.accepts(&found) {
                    self.diagnostics.push(TypeDiagnostic::mismatch(
                        TypeDiagnosticKind::ListElement,
                        expected.clone(),
                        found,
                    ));
                }
            } else {
                element_type = Some(found);
            }
        }
        Some(TypeName::List(element_type.map(Box::new)))
    }

    fn infer_index(&mut self, target: &Expression, index: &Expression) -> Option<TypeName> {
        self.expect(index, TypeName::Int, TypeDiagnosticKind::Index);
        match self.infer(target) {
            Some(TypeName::List(Some(element))) => Some(*element),
            Some(TypeName::List(None)) => None,
            Some(found) => {
                self.diagnostics.push(TypeDiagnostic::mismatch(
                    TypeDiagnosticKind::IndexTarget,
                    TypeName::List(None),
                    found,
                ));
                None
            }
            None => None,
        }
    }

    fn infer_len(&mut self, target: &Expression) -> Option<TypeName> {
        match self.infer(target) {
            Some(TypeName::List(_)) | Some(TypeName::String) => Some(TypeName::Int),
            Some(found) => {
                self.diagnostics
                    .push(TypeDiagnostic::new(TypeDiagnosticKind::LengthTarget));
                self.diagnostics.last_mut().expect("diagnostic added").found = Some(found);
                None
            }
            None => None,
        }
    }

    fn infer_match(
        &mut self,
        value: &Expression,
        arms: &[crate::parser::MatchExpressionArm],
    ) -> Option<TypeName> {
        let matched = self.infer(value);
        let mut result: Option<TypeName> = None;

        for arm in arms {
            self.check_pattern(&arm.pattern, matched.as_ref());
            let Some(found) = self.infer(&arm.value) else {
                continue;
            };
            if let Some(expected) = &result {
                if !expected.accepts(&found) {
                    self.diagnostics.push(TypeDiagnostic::mismatch(
                        TypeDiagnosticKind::MatchArm,
                        expected.clone(),
                        found,
                    ));
                }
            } else {
                result = Some(found);
            }
        }
        result
    }

    fn check_pattern(&mut self, pattern: &MatchPattern, expected: Option<&TypeName>) {
        let found = match pattern {
            MatchPattern::Literal(value) => Some(value.type_name()),
            MatchPattern::Identifier(name) => self.context.resolve(name).cloned().or_else(|| {
                if !self.is_unknown(name) {
                    self.diagnostics.push(TypeDiagnostic::new(
                        TypeDiagnosticKind::UndefinedVariable(name.clone()),
                    ));
                }
                None
            }),
            MatchPattern::Status => Some(TypeName::Int),
            MatchPattern::Wildcard => None,
        };
        if let (Some(expected), Some(found)) = (expected, found)
            && !expected.accepts(&found)
        {
            self.diagnostics.push(TypeDiagnostic::mismatch(
                TypeDiagnosticKind::MatchPattern,
                expected.clone(),
                found,
            ));
        }
    }

    fn infer_binary(
        &mut self,
        left: &Expression,
        operator: BinaryOperator,
        right: &Expression,
    ) -> Option<TypeName> {
        match operator {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual => {
                let start = self.diagnostics.len();
                let kind = TypeDiagnosticKind::BinaryOperands(operator.symbol().into());
                self.expect(left, TypeName::Int, kind.clone());
                self.expect(right, TypeName::Int, kind);
                if self.diagnostics.len() > start {
                    return None;
                }
            }
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                let left_type = self.infer(left)?;
                let right_type = self.infer(right)?;
                if !left_type.accepts(&right_type) {
                    self.diagnostics.push(TypeDiagnostic::mismatch(
                        TypeDiagnosticKind::BinaryOperands(operator.symbol().into()),
                        left_type,
                        right_type,
                    ));
                    return None;
                }
            }
        }

        match operator {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide => Some(TypeName::Int),
            _ => Some(TypeName::Bool),
        }
    }

    fn infer_call(&mut self, name: &str, args: &[Expression]) -> Option<TypeName> {
        let Some(function) = self.functions.get(name).cloned() else {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::UnknownFunction(
                    name.into(),
                )));
            return None;
        };
        self.check_call(name, args, &function);
        function.return_type.or_else(|| {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::NonValueFunction(
                    name.into(),
                )));
            None
        })
    }

    fn check_call(&mut self, name: &str, args: &[Expression], function: &FunctionType) {
        if args.len() != function.params.len() {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::ArgumentCount {
                    function: name.into(),
                    expected: function.params.len(),
                    found: args.len(),
                }));
        }

        for (index, (argument, expected)) in args.iter().zip(&function.params).enumerate() {
            if let Some(expected) = expected {
                self.expect(
                    argument,
                    expected.clone(),
                    TypeDiagnosticKind::FunctionArgument {
                        function: name.into(),
                        index,
                    },
                );
            }
        }
    }

    fn expect(&mut self, expression: &Expression, expected: TypeName, kind: TypeDiagnosticKind) {
        if let Some(found) = self.infer(expression)
            && !expected.accepts(&found)
        {
            self.diagnostics
                .push(TypeDiagnostic::mismatch(kind, expected, found));
        }
    }

    fn declare_unknown(&mut self, name: String) {
        self.unknowns
            .last_mut()
            .expect("checker always has a global unknown-type scope")
            .insert(name);
    }

    fn is_unknown(&self, name: &str) -> bool {
        self.unknowns.iter().rev().any(|scope| scope.contains(name))
    }
}

trait ReplaceOption<T> {
    fn replace_with(&mut self, value: Option<T>) -> Option<T>;
}

impl<T> ReplaceOption<T> for Option<T> {
    fn replace_with(&mut self, value: Option<T>) -> Option<T> {
        std::mem::replace(self, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn parsed(source: &str) -> ParsedInput {
        parse(source).unwrap()
    }

    #[test]
    fn checks_declarations_assignments_and_operator_types() {
        let program = [
            parsed("let retries: int = 3"),
            parsed("retries = retries + 1"),
            parsed("retries = \"many\""),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].expected, Some(TypeName::Int));
        assert_eq!(diagnostics[0].found, Some(TypeName::String));
        assert_eq!(
            diagnostics[0].kind,
            TypeDiagnosticKind::Assignment("retries".into())
        );
    }

    #[test]
    fn nested_declarations_do_not_escape_their_lexical_scope() {
        let program = [
            parsed("if true {\n    let local = 1\n}"),
            parsed("local = 2"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::UndefinedVariable("local".into())
        }));
    }

    #[test]
    fn infers_lists_indexes_and_match_expression_results() {
        let program = [
            parsed("let values: list<int> = [1, 2, 3]"),
            parsed("let first: int = values[0]"),
            parsed("let label: string = match first { 1 => \"one\", _ => \"other\" }"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn reports_incompatible_operator_operands() {
        let program = [parsed("let broken = 1 + \"two\"")];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert_eq!(diagnostics[0].expected, Some(TypeName::Int));
        assert_eq!(diagnostics[0].found, Some(TypeName::String));
        assert_eq!(
            diagnostics[0].kind,
            TypeDiagnosticKind::BinaryOperands("+".into())
        );
    }

    #[test]
    fn rejects_procedure_calls_in_value_position() {
        let program = [
            parsed("fn show(value) {\n    print value\n}"),
            parsed("let result = show(1)"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::NonValueFunction("show".into())
        }));
    }

    #[test]
    fn permits_untyped_procedure_parameters_without_guessing_their_type() {
        let program = [parsed(
            "fn show(value) {\n    let copy = value\n    copy = value\n    print copy\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn infers_expressions_from_a_caller_provided_context() {
        let mut context = TypeContext::new();
        context.declare("count".into(), TypeName::Int).unwrap();
        let mut checker = TypeChecker::with_context(context);
        let expression = match parsed("let doubled = count * 2") {
            ParsedInput::Let { value, .. } => value,
            _ => unreachable!("test source is a declaration"),
        };

        assert_eq!(checker.infer_expression(&expression), Ok(TypeName::Int));
    }

    #[test]
    fn checks_function_arguments_and_return_values() {
        let program = [
            parsed("fn add(value: int) -> int {\n    return value + 1\n}"),
            parsed("let total = add(\"one\")"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].expected, Some(TypeName::Int));
        assert_eq!(diagnostics[0].found, Some(TypeName::String));
    }

    #[test]
    fn checks_loop_iterables_and_loop_variable_scope() {
        let program = [parsed(
            "for number in [1, 2] {\n    let doubled: int = number * 2\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }
}
