use crate::runtime::TypeName;

/// A host-backed symbol whose type is not owned by lexical language scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSymbol<'a> {
    Status,
    Environment(&'a str),
}

/// Static behavior of a native stage exposed by the shell host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStageSignature {
    Values,
    Record,
    Take,
    Count,
    Collect,
}

/// Supplies host-owned symbol types and native structured-stage signatures.
///
/// Returning `None` for a command leaves it dynamically bounded. This is how
/// ordinary Unix commands remain valid without pretending that Crab knows their
/// argument or output types.
pub trait HostTypeProvider {
    fn symbol_type(&self, symbol: HostSymbol<'_>) -> Option<TypeName>;

    fn native_stage(&self, command: &str) -> Option<NativeStageSignature>;
}

/// Language defaults used when no richer shell host is supplied.
#[derive(Debug, Default)]
pub struct LanguageHostTypes;

impl HostTypeProvider for LanguageHostTypes {
    fn symbol_type(&self, symbol: HostSymbol<'_>) -> Option<TypeName> {
        match symbol {
            HostSymbol::Status => Some(TypeName::Int),
            HostSymbol::Environment(_) => Some(TypeName::String),
        }
    }

    fn native_stage(&self, _command: &str) -> Option<NativeStageSignature> {
        None
    }
}
