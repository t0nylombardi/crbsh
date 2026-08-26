use crate::value::{TypeName, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<Expression>,
    pub redirections: Redirections,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Literal(Value),
    Identifier(String),
    EnvironmentVariable(String),
    Status,
    Call {
        name: String,
        args: Vec<Expression>,
    },
    List(Vec<Expression>),
    Index {
        target: Box<Expression>,
        index: Box<Expression>,
    },
    Len(Box<Expression>),
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
}

impl From<Value> for Expression {
    fn from(value: Value) -> Self {
        Self::Literal(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl BinaryOperator {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redirections {
    pub stdin: Option<String>,
    pub stdout: Option<OutputRedirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRedirection {
    pub target: String,
    pub append: bool,
}

impl Redirections {
    pub fn is_empty(&self) -> bool {
        self.stdin.is_none() && self.stdout.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineConnector {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInput {
    Pipeline(Pipeline),
    PipelineChain {
        first: Pipeline,
        rest: Vec<(PipelineConnector, Pipeline)>,
    },
    BackgroundPipeline {
        pipeline: Pipeline,
        command: String,
    },
    FunctionDefinition {
        name: String,
        definition: FunctionDefinition,
    },
    Let {
        name: String,
        type_annotation: Option<TypeName>,
        value: Expression,
    },
    Assignment {
        name: String,
        value: Expression,
    },
    EnvironmentAssignment {
        name: String,
        value: Expression,
    },
    Return {
        value: Option<Expression>,
    },
    Break,
    Continue,
    If {
        branches: Vec<IfBranch>,
        else_body: Option<Vec<ParsedInput>>,
    },
    Match {
        value: Expression,
        arms: Vec<MatchArm>,
    },
    While {
        condition: Expression,
        body: Vec<ParsedInput>,
    },
    For {
        name: String,
        iterable: Iterable,
        body: Vec<ParsedInput>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDefinition {
    pub params: Vec<FunctionParam>,
    pub return_type: Option<TypeName>,
    pub body: Vec<ParsedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParam {
    pub name: String,
    pub type_annotation: Option<TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Iterable {
    Range {
        start: Expression,
        end: Expression,
        inclusive: bool,
    },
    Glob(String),
    Expression(Expression),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfBranch {
    pub condition: Expression,
    pub body: Vec<ParsedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: ParsedInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchPattern {
    Literal(Value),
    Identifier(String),
    Status,
    Wildcard,
}
