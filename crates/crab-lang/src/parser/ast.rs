use crate::runtime::TypeName;

use super::{
    Expression, FunctionDefinition, IfBranch, Iterable, MatchArm, Pipeline, PipelineConnector,
};

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
