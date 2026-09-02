use std::fmt;

use super::{ParseError, ParsedInput, parse};

/// A one-based location in Crab source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

/// A parsed top-level input and the location where it begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedInput {
    pub input: ParsedInput,
    pub location: SourceLocation,
}

/// A syntax diagnostic produced while parsing a complete source buffer.
#[derive(Debug, PartialEq, Eq)]
pub struct SourceDiagnostic {
    pub location: SourceLocation,
    pub error: ParseError,
}

impl fmt::Display for SourceDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.location, self.error)
    }
}

/// Parses every independent top-level input in a complete source buffer.
///
/// Syntax errors are aggregated when statement recovery is safe. A multiline
/// block is treated as one input and retains the location of its opening line.
pub fn parse_source(source: &str) -> Result<Vec<LocatedInput>, Vec<SourceDiagnostic>> {
    let mut inputs = Vec::new();
    let mut diagnostics = Vec::new();

    for statement in source_statements(source) {
        match parse(&statement.text) {
            Ok(input) => inputs.push(LocatedInput {
                input,
                location: statement.location,
            }),
            Err(error) => diagnostics.push(SourceDiagnostic {
                location: statement.location,
                error,
            }),
        }
    }

    if diagnostics.is_empty() {
        Ok(inputs)
    } else {
        Err(diagnostics)
    }
}

struct SourceStatement {
    text: String,
    location: SourceLocation,
}

fn source_statements(source: &str) -> Vec<SourceStatement> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut location = None;

    for (index, line) in source.lines().enumerate() {
        if current.is_empty() && line.trim().is_empty() {
            continue;
        }

        location.get_or_insert_with(|| SourceLocation {
            line: source_coordinate(index + 1),
            column: source_coordinate(
                line.chars()
                    .take_while(|character| character.is_whitespace())
                    .count()
                    + 1,
            ),
        });

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);

        if brace_balance(&current) <= 0 && !current.trim().is_empty() {
            statements.push(SourceStatement {
                text: std::mem::take(&mut current),
                location: location.take().expect("non-empty statement has a location"),
            });
        }
    }

    if !current.trim().is_empty() {
        statements.push(SourceStatement {
            text: current,
            location: location.expect("non-empty statement has a location"),
        });
    }

    statements
}

fn source_coordinate(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn brace_balance(input: &str) -> i32 {
    let mut balance = 0;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for character in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match character {
            '\\' if !in_single_quotes => escaped = true,
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '{' if !in_single_quotes && !in_double_quotes => balance += 1,
            '}' if !in_single_quotes && !in_double_quotes => balance -= 1,
            _ => {}
        }
    }

    balance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_source_with_top_level_locations() {
        let inputs = parse_source("\n  let first = 1\n\nlet second = 2\n").unwrap();

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].location, SourceLocation { line: 2, column: 3 });
        assert_eq!(inputs[1].location, SourceLocation { line: 4, column: 1 });
    }

    #[test]
    fn aggregates_independent_syntax_diagnostics() {
        let diagnostics = parse_source("let = 1\nlet = 2\n").unwrap_err();

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].location.line, 1);
        assert_eq!(diagnostics[1].location.line, 2);
    }

    #[test]
    fn keeps_multiline_blocks_as_one_input() {
        let inputs = parse_source("if true {\n    print yes\n}\nlet done = true\n").unwrap();

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].location.line, 1);
        assert_eq!(inputs[1].location.line, 4);
    }
}
