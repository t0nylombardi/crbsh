mod context;
mod diagnostic;
mod host;

use std::collections::{HashMap, HashSet};

pub use context::{ContextError, TypeContext};
pub use diagnostic::{TypeDiagnostic, TypeDiagnosticKind};
pub use host::{HostSymbol, HostTypeProvider, LanguageHostTypes, NativeStageSignature};

use crate::parser::{
    BinaryOperator, Expression, FunctionDefinition, Iterable, LocatedInput, MatchPattern,
    ParsedInput, Pipeline,
};
use crate::runtime::TypeName;

#[derive(Debug, Clone)]
struct FunctionType {
    params: Vec<Option<TypeName>>,
    return_type: Option<TypeName>,
}

/// Traverses Crab AST nodes and reports type errors without evaluation.
pub struct TypeChecker<'host> {
    context: TypeContext,
    host: &'host dyn HostTypeProvider,
    unknowns: Vec<HashSet<String>>,
    function_scopes: Vec<HashMap<String, FunctionType>>,
    diagnostics: Vec<TypeDiagnostic>,
    current_function: Option<FunctionContext>,
}

#[derive(Debug, Clone)]
struct FunctionContext {
    name: String,
    return_type: Option<TypeName>,
}

impl Default for TypeChecker<'static> {
    fn default() -> Self {
        Self::new()
    }
}

static LANGUAGE_HOST_TYPES: LanguageHostTypes = LanguageHostTypes;

impl TypeChecker<'static> {
    /// Creates a checker with an empty global type context.
    pub fn new() -> Self {
        Self {
            context: TypeContext::new(),
            host: &LANGUAGE_HOST_TYPES,
            unknowns: vec![HashSet::new()],
            function_scopes: vec![HashMap::new()],
            diagnostics: Vec::new(),
            current_function: None,
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
        checker.check_program(program)
    }

    /// Checks a program using host-owned symbol and native-stage types.
    pub fn check_with_host(
        program: &[ParsedInput],
        host: &dyn HostTypeProvider,
    ) -> Result<(), Vec<TypeDiagnostic>> {
        TypeChecker::with_host(host).check_program(program)
    }

    /// Checks located inputs and attaches their source locations to diagnostics.
    pub fn check_located_with_host(
        program: &[LocatedInput],
        host: &dyn HostTypeProvider,
    ) -> Result<(), Vec<TypeDiagnostic>> {
        TypeChecker::with_host(host).check_located_program(program)
    }
}

impl<'host> TypeChecker<'host> {
    /// Creates a checker using types supplied by the shell host.
    pub fn with_host(host: &'host dyn HostTypeProvider) -> Self {
        Self {
            context: TypeContext::new(),
            host,
            unknowns: vec![HashSet::new()],
            function_scopes: vec![HashMap::new()],
            diagnostics: Vec::new(),
            current_function: None,
        }
    }

    fn check_program(&mut self, program: &[ParsedInput]) -> Result<(), Vec<TypeDiagnostic>> {
        self.check_statements(program);

        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.diagnostics))
        }
    }

    fn check_located_program(
        &mut self,
        program: &[LocatedInput],
    ) -> Result<(), Vec<TypeDiagnostic>> {
        let statements = program
            .iter()
            .map(|located| located.input.clone())
            .collect::<Vec<_>>();
        self.register_functions(&statements);

        for located in program {
            let diagnostic_start = self.diagnostics.len();
            self.check_statement(&located.input);
            for diagnostic in &mut self.diagnostics[diagnostic_start..] {
                diagnostic.locate(located.location);
            }
        }

        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.diagnostics))
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
                self.function_scopes
                    .last_mut()
                    .expect("checker always has a function scope")
                    .insert(
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
        self.register_functions(statements);
        for statement in statements {
            self.check_statement(statement);
        }
    }

    fn check_statement(&mut self, statement: &ParsedInput) {
        match statement {
            ParsedInput::Module { .. } | ParsedInput::Import { .. } => {}
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
            ParsedInput::FunctionDefinition { name, definition } => {
                self.check_function(name, definition)
            }
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
        match (self.current_function.clone(), value) {
            (
                Some(FunctionContext {
                    return_type: Some(expected),
                    ..
                }),
                Some(value),
            ) => {
                self.expect(
                    value,
                    expected,
                    TypeDiagnosticKind::Declaration("return".into()),
                );
            }
            (Some(function), None) if function.return_type.is_some() => {
                self.diagnostics
                    .push(TypeDiagnostic::new(TypeDiagnosticKind::MissingReturnValue(
                        function.name,
                    )))
            }
            (Some(_), Some(value)) => {
                self.infer(value);
                self.diagnostics.push(TypeDiagnostic::new(
                    TypeDiagnosticKind::UnexpectedReturnValue,
                ));
            }
            (Some(_), None) | (None, _) => {}
        }
    }

    fn check_function(&mut self, name: &str, definition: &FunctionDefinition) {
        let function_context = self.context.function_context();
        let caller_context = std::mem::replace(&mut self.context, function_context);
        let function_unknowns = vec![
            self.unknowns.first().cloned().unwrap_or_default(),
            HashSet::new(),
        ];
        let caller_unknowns = std::mem::replace(&mut self.unknowns, function_unknowns);
        let caller_function = self.current_function.replace(FunctionContext {
            name: name.into(),
            return_type: definition.return_type.clone(),
        });
        self.function_scopes.push(HashMap::new());

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
        if definition.return_type.is_some() && !statements_guarantee_return(&definition.body) {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::MissingReturnValue(
                    name.into(),
                )));
        }

        self.function_scopes.pop();
        self.context = caller_context;
        self.unknowns = caller_unknowns;
        self.current_function = caller_function;
    }

    fn check_for(&mut self, name: &str, iterable: &Iterable, body: &[ParsedInput]) {
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
        } else {
            self.declare_unknown(name.into());
        }
        self.check_statements(body);
        self.unknowns.pop();
        self.context.pop_scope();
    }

    fn check_block(&mut self, statements: &[ParsedInput]) {
        self.context.push_scope();
        self.unknowns.push(HashSet::new());
        self.function_scopes.push(HashMap::new());
        self.check_statements(statements);
        self.function_scopes.pop();
        self.unknowns.pop();
        self.context.pop_scope();
    }

    fn check_pipeline(&mut self, pipeline: &Pipeline) {
        let mut stream_type = None;
        for command in &pipeline.commands {
            if let Some(function) = self.resolve_function(&command.name).cloned() {
                self.check_call(&command.name, &command.args, &function);
                continue;
            }

            if let Some(signature) = self.host.native_stage(&command.name) {
                stream_type = self.check_native_stage(command, signature, stream_type);
            } else {
                // External commands remain dynamically bounded. Their arguments
                // are host syntax, while output crossing into Crab is text.
                stream_type = Some(TypeName::String);
            }
        }
    }

    fn check_native_stage(
        &mut self,
        command: &crate::parser::ParsedCommand,
        signature: NativeStageSignature,
        input: Option<TypeName>,
    ) -> Option<TypeName> {
        match signature {
            NativeStageSignature::Values => {
                self.expect_no_stage_input(&command.name, input.as_ref());
                self.infer_stream_arguments(&command.args)
            }
            NativeStageSignature::Record => {
                self.expect_no_stage_input(&command.name, input.as_ref());
                self.infer_record_stage(&command.name, &command.args)
            }
            NativeStageSignature::Take => {
                self.expect_stage_input(&command.name, input.as_ref());
                self.expect_stage_arguments(&command.name, &command.args, &[TypeName::Int]);
                input
            }
            NativeStageSignature::Count => {
                self.expect_stage_input(&command.name, input.as_ref());
                self.expect_stage_arguments(&command.name, &command.args, &[]);
                Some(TypeName::Int)
            }
            NativeStageSignature::Collect => {
                self.expect_stage_input(&command.name, input.as_ref());
                self.expect_stage_arguments(&command.name, &command.args, &[]);
                Some(TypeName::List(input.map(Box::new)))
            }
        }
    }

    fn infer_stream_arguments(&mut self, arguments: &[Expression]) -> Option<TypeName> {
        let mut item_type = None;
        for argument in arguments {
            let found = match self.infer(argument) {
                Some(TypeName::List(element)) => element.map(|element| *element),
                other => other,
            };
            item_type = merge_stream_type(item_type, found);
        }
        item_type
    }

    fn infer_record_stage(&mut self, name: &str, arguments: &[Expression]) -> Option<TypeName> {
        if !arguments.len().is_multiple_of(2) {
            self.diagnostics.push(TypeDiagnostic::new(
                TypeDiagnosticKind::StageArgumentCount {
                    stage: name.into(),
                    expected: arguments.len() + 1,
                    found: arguments.len(),
                },
            ));
            return Some(TypeName::Record(None));
        }

        let mut fields = std::collections::BTreeMap::new();
        for pair in arguments.chunks_exact(2) {
            let field = match &pair[0] {
                Expression::Identifier(name)
                | Expression::Literal(crate::runtime::Value::String(name)) => Some(name.clone()),
                _ => None,
            };
            let value_type = self.infer(&pair[1]);
            if let (Some(field), Some(value_type)) = (field, value_type) {
                fields.insert(field, value_type);
            }
        }
        Some(TypeName::Record(Some(fields)))
    }

    fn expect_stage_arguments(
        &mut self,
        name: &str,
        arguments: &[Expression],
        expected: &[TypeName],
    ) {
        if arguments.len() != expected.len() {
            self.diagnostics.push(TypeDiagnostic::new(
                TypeDiagnosticKind::StageArgumentCount {
                    stage: name.into(),
                    expected: expected.len(),
                    found: arguments.len(),
                },
            ));
        }
        for (index, argument) in arguments.iter().enumerate() {
            if let Some(expected) = expected.get(index) {
                self.expect(
                    argument,
                    expected.clone(),
                    TypeDiagnosticKind::StageArgument {
                        stage: name.into(),
                        index,
                    },
                );
            }
        }
    }

    fn expect_no_stage_input(&mut self, name: &str, input: Option<&TypeName>) {
        if input.is_some() {
            self.diagnostics.push(TypeDiagnostic::new(
                TypeDiagnosticKind::UnexpectedStageInput(name.into()),
            ));
        }
    }

    fn expect_stage_input(&mut self, name: &str, input: Option<&TypeName>) {
        if input.is_none() {
            self.diagnostics
                .push(TypeDiagnostic::new(TypeDiagnosticKind::MissingStageInput(
                    name.into(),
                )));
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
            Expression::EnvironmentVariable(name) => self
                .host
                .symbol_type(HostSymbol::Environment(name.as_str())),
            Expression::Status => self.host.symbol_type(HostSymbol::Status),
            Expression::Call { name, args } => self.infer_call(name, args),
            Expression::List(values) => self.infer_list(values),
            Expression::Index { target, index } => self.infer_index(target, index),
            Expression::Field { target, name } => self.infer_field(target, name),
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

    fn infer_field(&mut self, target: &Expression, name: &str) -> Option<TypeName> {
        match self.infer(target) {
            Some(TypeName::Record(Some(fields))) => fields.get(name).cloned().or_else(|| {
                self.diagnostics
                    .push(TypeDiagnostic::new(TypeDiagnosticKind::MissingRecordField(
                        name.into(),
                    )));
                None
            }),
            Some(TypeName::Record(None)) => None,
            Some(found) => {
                self.diagnostics.push(TypeDiagnostic::mismatch(
                    TypeDiagnosticKind::FieldTarget,
                    TypeName::Record(None),
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
        let Some(function) = self.resolve_function(name).cloned() else {
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

        for (index, argument) in args.iter().enumerate() {
            match function.params.get(index).and_then(Option::as_ref) {
                Some(expected) => self.expect(
                    argument,
                    expected.clone(),
                    TypeDiagnosticKind::FunctionArgument {
                        function: name.into(),
                        index,
                    },
                ),
                None => {
                    self.infer(argument);
                }
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

    fn resolve_function(&self, name: &str) -> Option<&FunctionType> {
        self.function_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }
}

fn statements_guarantee_return(statements: &[ParsedInput]) -> bool {
    statements.iter().any(statement_guarantees_return)
}

fn merge_stream_type(current: Option<TypeName>, found: Option<TypeName>) -> Option<TypeName> {
    match (current, found) {
        (None, found) => found,
        (Some(current), Some(found)) if current.accepts(&found) => Some(current),
        _ => None,
    }
}

fn statement_guarantees_return(statement: &ParsedInput) -> bool {
    match statement {
        ParsedInput::Return { .. } => true,
        ParsedInput::If {
            branches,
            else_body: Some(else_body),
        } => {
            branches
                .iter()
                .all(|branch| statements_guarantee_return(&branch.body))
                && statements_guarantee_return(else_body)
        }
        ParsedInput::Match { arms, .. } => {
            arms.iter()
                .any(|arm| matches!(arm.pattern, MatchPattern::Wildcard))
                && arms
                    .iter()
                    .all(|arm| statement_guarantees_return(&arm.body))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::parser::{Expression, parse};
    use crate::runtime::Value;

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
    fn infers_list_literal_element_types() {
        let expression = Expression::List(vec![Value::Int(1).into(), Value::Int(2).into()]);

        assert_eq!(
            TypeChecker::new().infer_expression(&expression),
            Ok(TypeName::List(Some(Box::new(TypeName::Int))))
        );
    }

    #[test]
    fn rejects_heterogeneous_list_literals() {
        let program = [parsed("let values = [1, \"two\"]")];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::ListElement
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
    }

    #[test]
    fn uses_annotations_to_type_empty_lists() {
        let program = [
            parsed("let values: list<int> = []"),
            parsed("let first: int = values[0]"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn checks_index_types_and_targets() {
        let program = [
            parsed("let values = [1, 2]"),
            parsed("let bad_index = values[\"zero\"]"),
            parsed("let bad_target = true[0]"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::Index
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::IndexTarget
                && diagnostic.found == Some(TypeName::Bool)
        }));
    }

    #[test]
    fn tracks_record_field_types() {
        let record = Value::Record(BTreeMap::from([
            ("active".into(), Value::Bool(true)),
            ("name".into(), Value::String("Tony".into())),
        ]));
        let program = [
            ParsedInput::Let {
                name: "user".into(),
                type_annotation: None,
                value: record.into(),
            },
            parsed("let name: string = user.name"),
            parsed("let active: bool = user.active"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn reports_missing_record_fields() {
        let record = Value::Record(BTreeMap::from([(
            "name".into(),
            Value::String("Tony".into()),
        )]));
        let program = [
            ParsedInput::Let {
                name: "user".into(),
                type_annotation: None,
                value: record.into(),
            },
            parsed("let missing = user.email"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MissingRecordField("email".into())
        }));
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
    fn collects_function_signatures_before_checking_calls() {
        let program = [
            parsed("let total: int = add_one(2)"),
            parsed("fn add_one(value: int) -> int {\n    return value + 1\n}"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn checks_parameter_types_inside_function_bodies() {
        let program = [parsed(
            "fn invalid(value: string) -> int {\n    return value + 1\n}",
        )];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::BinaryOperands("+".into())
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
    }

    #[test]
    fn checks_function_call_arity() {
        let program = [
            parsed("fn add(left: int, right: int) -> int {\n    return left + right\n}"),
            parsed("let total = add(1)"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind
                == TypeDiagnosticKind::ArgumentCount {
                    function: "add".into(),
                    expected: 2,
                    found: 1,
                }
        }));
    }

    #[test]
    fn checks_extra_function_argument_expressions() {
        let program = [
            parsed("fn identity(value: int) -> int {\n    return value\n}"),
            parsed("let result = identity(1, missing)"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::UndefinedVariable("missing".into())
        }));
    }

    #[test]
    fn checks_return_expressions_against_the_declared_type() {
        let program = [parsed(
            "fn invalid(value: int) -> int {\n    return \"wrong\"\n}",
        )];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::Declaration("return".into())
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
    }

    #[test]
    fn rejects_typed_functions_that_can_fall_through() {
        let program = [parsed(
            "fn classify(value: int) -> string {\n    if value == 0 {\n        return \"zero\"\n    }\n}",
        )];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MissingReturnValue("classify".into())
        }));
    }

    #[test]
    fn accepts_typed_functions_when_every_branch_returns() {
        let program = [parsed(
            "fn classify(value: int) -> string {\n    if value == 0 {\n        return \"zero\"\n    } else {\n        return \"other\"\n    }\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn checks_recursive_function_calls() {
        let program = [parsed(
            "fn factorial(value: int) -> int {\n    if value == 0 {\n        return 1\n    } else {\n        return value * factorial(value - 1)\n    }\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn collects_nested_function_signatures_in_their_lexical_scope() {
        let program = [parsed(
            "fn outer(value: int) -> int {\n    let result = helper(value)\n    fn helper(input: int) -> int {\n        return input + 1\n    }\n    return result\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn nested_function_signatures_do_not_escape_their_scope() {
        let program = [
            parsed(
                "fn outer(value: int) -> int {\n    fn helper(input: int) -> int {\n        return input + 1\n    }\n    return helper(value)\n}",
            ),
            parsed("let leaked = helper(1)"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::UnknownFunction("helper".into())
        }));
    }

    #[test]
    fn checks_loop_iterables_and_loop_variable_scope() {
        let program = [parsed(
            "for number in [1, 2] {\n    let doubled: int = number * 2\n}",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn requires_boolean_if_and_while_conditions() {
        let program = [
            parsed(
                "if 1 {\n    print \"never\"\n} else if \"also wrong\" {\n    print \"never\"\n}",
            ),
            parsed("while [true] {\n    break\n}"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == TypeDiagnosticKind::Condition)
                .count(),
            3
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::Condition
                && diagnostic.expected == Some(TypeName::Bool)
                && diagnostic.found == Some(TypeName::Int)
        }));
    }

    #[test]
    fn accepts_boolean_if_and_while_conditions() {
        let program = [
            parsed("if true {\n    print \"yes\"\n}"),
            parsed("while 1 < 2 {\n    break\n}"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn validates_range_bounds_and_iterable_expressions() {
        let program = [
            parsed("for number in \"start\"..3 {\n    print number\n}"),
            parsed("for item in true {\n    print item\n}"),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::RangeBound
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::Iterable
                && diagnostic.expected == Some(TypeName::List(None))
                && diagnostic.found == Some(TypeName::Bool)
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::UndefinedVariable("item".into())
        }));
    }

    #[test]
    fn assigns_string_and_integer_types_to_host_and_range_iterators() {
        let program = [
            parsed("for file in src/*.rs {\n    let path: string = file\n}"),
            parsed("for number in 0..=3 {\n    let value: int = number\n}"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[test]
    fn default_host_types_status_and_environment_as_int_and_string() {
        let program = [
            parsed("let code: int = status"),
            parsed("let home: string = env.HOME"),
        ];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }

    #[derive(Debug)]
    struct TestHostTypes;

    impl HostTypeProvider for TestHostTypes {
        fn symbol_type(&self, symbol: HostSymbol<'_>) -> Option<TypeName> {
            match symbol {
                HostSymbol::Status => Some(TypeName::Int),
                HostSymbol::Environment(_) => Some(TypeName::String),
            }
        }

        fn native_stage(&self, command: &str) -> Option<NativeStageSignature> {
            match command {
                "emit" => Some(NativeStageSignature::Values),
                "limit" => Some(NativeStageSignature::Take),
                "size" => Some(NativeStageSignature::Count),
                "bundle" => Some(NativeStageSignature::Collect),
                _ => None,
            }
        }
    }

    #[test]
    fn checks_host_provided_native_stage_signatures() {
        let host = TestHostTypes;
        let program = [parsed("emit [1, 2] | limit \"two\" | bundle")];

        let diagnostics = TypeChecker::check_with_host(&program, &host).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind
                == TypeDiagnosticKind::StageArgument {
                    stage: "limit".into(),
                    index: 0,
                }
                && diagnostic.expected == Some(TypeName::Int)
                && diagnostic.found == Some(TypeName::String)
        }));
    }

    #[test]
    fn reports_native_stage_stream_contract_errors() {
        let host = TestHostTypes;
        let program = [parsed("limit 1"), parsed("emit 1 | emit 2")];

        let diagnostics = TypeChecker::check_with_host(&program, &host).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MissingStageInput("limit".into())
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::UnexpectedStageInput("emit".into())
        }));
    }

    #[test]
    fn leaves_unknown_unix_commands_dynamically_bounded() {
        let host = TestHostTypes;
        let program = [parsed("printf missing | size")];

        assert_eq!(TypeChecker::check_with_host(&program, &host), Ok(()));
    }

    #[test]
    fn rejects_incompatible_statement_match_patterns() {
        let program = [parsed(
            "match 1 {\n    \"one\" => print \"wrong\"\n    true => print \"also wrong\"\n    _ => print \"fallback\"\n}",
        )];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == TypeDiagnosticKind::MatchPattern)
                .count(),
            2
        );
    }

    #[test]
    fn checks_identifier_and_status_match_pattern_types() {
        let program = [
            parsed("let label = \"ready\""),
            parsed(
                "match true {\n    label => print \"wrong\"\n    status => print \"wrong\"\n    _ => print \"fallback\"\n}",
            ),
        ];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MatchPattern
                && diagnostic.found == Some(TypeName::String)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MatchPattern
                && diagnostic.found == Some(TypeName::Int)
        }));
    }

    #[test]
    fn rejects_incompatible_match_expression_patterns_and_results() {
        let program = [parsed(
            "let result = match true { 1 => \"wrong pattern\", true => \"yes\", _ => 0 }",
        )];

        let diagnostics = TypeChecker::check(&program).unwrap_err();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MatchPattern
                && diagnostic.expected == Some(TypeName::Bool)
                && diagnostic.found == Some(TypeName::Int)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TypeDiagnosticKind::MatchArm
                && diagnostic.expected == Some(TypeName::String)
                && diagnostic.found == Some(TypeName::Int)
        }));
    }

    #[test]
    fn unifies_compatible_match_expression_result_types() {
        let program = [parsed(
            "let result: list<int> = match true { true => [], _ => [1, 2] }",
        )];

        assert_eq!(TypeChecker::check(&program), Ok(()));
    }
}
