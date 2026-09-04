use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crab_lang::parser::{Expression, FunctionDefinition, LocatedInput, ParsedInput, parse_source};

use crate::static_check;

/// Loads a root script and its transitive `.crb` imports as one checked program.
pub(crate) fn load_program(path: &Path) -> Result<Vec<LocatedInput>, Vec<ModuleDiagnostic>> {
    let root = canonical_crb_path(path).map_err(|diagnostic| vec![diagnostic])?;
    let mut loader = ModuleLoader::default();
    loader.load(&root, true)?;

    static_check::check_program(&loader.program).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| ModuleDiagnostic::Static {
                path: root.clone(),
                diagnostic: Box::new(diagnostic),
            })
            .collect::<Vec<_>>()
    })?;

    Ok(loader.program)
}

#[derive(Debug)]
pub(crate) enum ModuleDiagnostic {
    Io {
        path: PathBuf,
        message: String,
    },
    Syntax {
        path: PathBuf,
        message: String,
    },
    MissingDeclaration(PathBuf),
    DuplicateDeclaration(PathBuf),
    DuplicateNamespace {
        name: String,
        paths: Vec<PathBuf>,
    },
    Cycle(Vec<PathBuf>),
    Static {
        path: PathBuf,
        diagnostic: Box<static_check::StaticDiagnostic>,
    },
}

impl fmt::Display for ModuleDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } | Self::Syntax { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::MissingDeclaration(path) => {
                write!(
                    formatter,
                    "{}: imported file must declare a module",
                    path.display()
                )
            }
            Self::DuplicateDeclaration(path) => {
                write!(
                    formatter,
                    "{}: file declares more than one module",
                    path.display()
                )
            }
            Self::DuplicateNamespace { name, paths } => write!(
                formatter,
                "module namespace '{name}' is declared by {} and {}",
                paths[0].display(),
                paths[1].display()
            ),
            Self::Cycle(paths) => {
                let chain = paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(formatter, "cyclic import: {chain}")
            }
            Self::Static { path, diagnostic } => match diagnostic.location() {
                Some(location) => write!(formatter, "{}:{location}: {diagnostic}", path.display()),
                None => write!(formatter, "{}: {diagnostic}", path.display()),
            },
        }
    }
}

#[derive(Default)]
struct ModuleLoader {
    program: Vec<LocatedInput>,
    loaded: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    namespaces: HashMap<String, PathBuf>,
}

impl ModuleLoader {
    fn load(&mut self, path: &Path, root: bool) -> Result<(), Vec<ModuleDiagnostic>> {
        if self.loaded.contains(path) {
            return Ok(());
        }
        if let Some(index) = self.stack.iter().position(|candidate| candidate == path) {
            let mut cycle = self.stack[index..].to_vec();
            cycle.push(path.to_path_buf());
            return Err(vec![ModuleDiagnostic::Cycle(cycle)]);
        }

        self.stack.push(path.to_path_buf());
        let source = fs::read_to_string(path).map_err(|error| {
            vec![ModuleDiagnostic::Io {
                path: path.to_path_buf(),
                message: error.to_string(),
            }]
        })?;
        let mut parsed = parse_source(&source).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| ModuleDiagnostic::Syntax {
                    path: path.to_path_buf(),
                    message: diagnostic.to_string(),
                })
                .collect::<Vec<_>>()
        })?;

        let declarations = parsed
            .iter()
            .filter_map(|located| match &located.input {
                ParsedInput::Module { name } => Some(name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if declarations.len() > 1 {
            return Err(vec![ModuleDiagnostic::DuplicateDeclaration(
                path.to_path_buf(),
            )]);
        }
        if !root && declarations.is_empty() {
            return Err(vec![ModuleDiagnostic::MissingDeclaration(
                path.to_path_buf(),
            )]);
        }

        let namespace = declarations.first().cloned();
        if let Some(name) = namespace.as_ref()
            && let Some(existing) = self.namespaces.insert(name.clone(), path.to_path_buf())
            && existing != path
        {
            return Err(vec![ModuleDiagnostic::DuplicateNamespace {
                name: name.clone(),
                paths: vec![existing, path.to_path_buf()],
            }]);
        }

        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        let imports = parsed
            .iter()
            .filter_map(|located| match &located.input {
                ParsedInput::Import { path } => Some(path.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for import in imports {
            let imported = canonical_crb_path(&directory.join(import))
                .map_err(|diagnostic| vec![diagnostic])?;
            self.load(&imported, false)?;
        }

        parsed.retain(|located| {
            !matches!(
                located.input,
                ParsedInput::Module { .. } | ParsedInput::Import { .. }
            )
        });
        if let Some(namespace) = namespace {
            qualify_module(&mut parsed, &namespace);
        }
        self.program.extend(parsed);
        self.stack.pop();
        self.loaded.insert(path.to_path_buf());
        Ok(())
    }
}

fn canonical_crb_path(path: &Path) -> Result<PathBuf, ModuleDiagnostic> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("crb") {
        return Err(ModuleDiagnostic::Io {
            path: path.to_path_buf(),
            message: "module files must use the .crb extension".into(),
        });
    }
    fs::canonicalize(path).map_err(|error| ModuleDiagnostic::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn qualify_module(program: &mut [LocatedInput], namespace: &str) {
    let globals = program
        .iter()
        .filter_map(|located| match &located.input {
            ParsedInput::Let { name, .. }
            | ParsedInput::FunctionDefinition { name, .. }
            | ParsedInput::TypeDefinition { name, .. }
            | ParsedInput::EnumDefinition { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut rewriter = NamespaceRewriter::new(namespace, globals);
    for located in program {
        rewriter.statement(&mut located.input, true);
    }
}

struct NamespaceRewriter {
    namespace: String,
    globals: HashSet<String>,
    locals: Vec<HashSet<String>>,
}

impl NamespaceRewriter {
    fn new(namespace: &str, globals: HashSet<String>) -> Self {
        Self {
            namespace: namespace.into(),
            globals,
            locals: vec![HashSet::new()],
        }
    }

    fn qualified(&self, name: &str) -> String {
        format!("{}::{name}", self.namespace)
    }

    fn resolves_global(&self, name: &str) -> bool {
        !name.contains("::")
            && self.globals.contains(name)
            && !self.locals.iter().rev().any(|scope| scope.contains(name))
    }

    fn statement(&mut self, statement: &mut ParsedInput, top_level: bool) {
        match statement {
            ParsedInput::Let {
                name,
                type_annotation,
                value,
            } => {
                if let Some(type_name) = type_annotation {
                    self.type_name(type_name);
                }
                self.expression(value);
                if top_level {
                    *name = self.qualified(name);
                } else {
                    self.locals
                        .last_mut()
                        .expect("local scope")
                        .insert(name.clone());
                }
            }
            ParsedInput::Assignment { name, value } => {
                self.expression(value);
                if self.resolves_global(name) {
                    *name = self.qualified(name);
                }
            }
            ParsedInput::EnvironmentAssignment { value, .. }
            | ParsedInput::Return { value: Some(value) } => self.expression(value),
            ParsedInput::Return { value: None } | ParsedInput::Break | ParsedInput::Continue => {}
            ParsedInput::FunctionDefinition { name, definition } => {
                *name = self.qualified(name);
                self.function(definition);
            }
            ParsedInput::TypeDefinition { name, definition } => {
                *name = self.qualified(name);
                for type_name in definition.fields.values_mut() {
                    self.type_name(type_name);
                }
            }
            ParsedInput::EnumDefinition { name, definition } => {
                *name = self.qualified(name);
                for type_name in definition.variants.values_mut().flatten() {
                    self.type_name(type_name);
                }
            }
            ParsedInput::If {
                branches,
                else_body,
            } => {
                for branch in branches {
                    self.expression(&mut branch.condition);
                    self.block(&mut branch.body);
                }
                if let Some(body) = else_body {
                    self.block(body);
                }
            }
            ParsedInput::Match { value, arms } => {
                self.expression(value);
                for arm in arms {
                    self.pattern(&mut arm.pattern);
                    self.locals.push(pattern_bindings(&arm.pattern));
                    self.statement(&mut arm.body, false);
                    self.locals.pop();
                }
            }
            ParsedInput::While { condition, body } => {
                self.expression(condition);
                self.block(body);
            }
            ParsedInput::For {
                name,
                iterable,
                body,
            } => {
                match iterable {
                    crab_lang::parser::Iterable::Range { start, end, .. } => {
                        self.expression(start);
                        self.expression(end);
                    }
                    crab_lang::parser::Iterable::Expression(value) => self.expression(value),
                    crab_lang::parser::Iterable::Glob(_) => {}
                }
                self.locals.push(HashSet::from([name.clone()]));
                for statement in body {
                    self.statement(statement, false);
                }
                self.locals.pop();
            }
            ParsedInput::Pipeline(pipeline) | ParsedInput::BackgroundPipeline { pipeline, .. } => {
                self.pipeline(pipeline)
            }
            ParsedInput::PipelineChain { first, rest } => {
                self.pipeline(first);
                for (_, pipeline) in rest {
                    self.pipeline(pipeline);
                }
            }
            ParsedInput::Module { .. } | ParsedInput::Import { .. } => {}
        }
    }

    fn function(&mut self, definition: &mut FunctionDefinition) {
        for param in &mut definition.params {
            if let Some(type_name) = &mut param.type_annotation {
                self.type_name(type_name);
            }
        }
        if let Some(type_name) = &mut definition.return_type {
            self.type_name(type_name);
        }
        self.locals.push(
            definition
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        );
        for statement in &mut definition.body {
            self.statement(statement, false);
        }
        self.locals.pop();
    }

    fn block(&mut self, body: &mut [ParsedInput]) {
        self.locals.push(HashSet::new());
        for statement in body {
            self.statement(statement, false);
        }
        self.locals.pop();
    }

    fn pipeline(&mut self, pipeline: &mut crab_lang::parser::Pipeline) {
        for command in &mut pipeline.commands {
            if self.resolves_global(&command.name) {
                command.name = self.qualified(&command.name);
            }
            for argument in &mut command.args {
                self.expression(argument);
            }
        }
    }

    fn expression(&mut self, expression: &mut Expression) {
        match expression {
            Expression::Identifier(name) => {
                if self.resolves_global(name) {
                    *name = self.qualified(name);
                }
            }
            Expression::Call { name, args } => {
                if self.resolves_global(name) {
                    *name = self.qualified(name);
                }
                for argument in args {
                    self.expression(argument);
                }
            }
            Expression::Construct { type_name, fields } => {
                if self.resolves_global(type_name) {
                    *type_name = self.qualified(type_name);
                }
                for value in fields.values_mut() {
                    self.expression(value);
                }
            }
            Expression::EnumVariant {
                enum_name, payload, ..
            } => {
                if self.resolves_global(enum_name) {
                    *enum_name = self.qualified(enum_name);
                }
                if let Some(payload) = payload {
                    self.expression(payload);
                }
            }
            Expression::List(values) => {
                for value in values {
                    self.expression(value);
                }
            }
            Expression::Index { target, index }
            | Expression::Binary {
                left: target,
                right: index,
                ..
            } => {
                self.expression(target);
                self.expression(index);
            }
            Expression::Field { target, .. } | Expression::Len(target) => self.expression(target),
            Expression::Match { value, arms } => {
                self.expression(value);
                for arm in arms {
                    self.pattern(&mut arm.pattern);
                    self.locals.push(pattern_bindings(&arm.pattern));
                    self.expression(&mut arm.value);
                    self.locals.pop();
                }
            }
            Expression::Literal(_) | Expression::EnvironmentVariable(_) | Expression::Status => {}
        }
    }

    fn type_name(&self, type_name: &mut crab_lang::runtime::TypeName) {
        match type_name {
            crab_lang::runtime::TypeName::Named(name) if self.resolves_global(name) => {
                *name = self.qualified(name);
            }
            crab_lang::runtime::TypeName::List(Some(element)) => self.type_name(element),
            _ => {}
        }
    }

    fn pattern(&self, pattern: &mut crab_lang::parser::MatchPattern) {
        if let crab_lang::parser::MatchPattern::EnumVariant { enum_name, .. } = pattern
            && self.resolves_global(enum_name)
        {
            *enum_name = self.qualified(enum_name);
        }
    }
}

fn pattern_bindings(pattern: &crab_lang::parser::MatchPattern) -> HashSet<String> {
    match pattern {
        crab_lang::parser::MatchPattern::EnumVariant {
            binding: Some(binding),
            ..
        } => HashSet::from([binding.clone()]),
        _ => HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("crbsh_modules_{name}_{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolves_relative_imports_and_loads_each_file_once() {
        let directory = temp_directory("relative_deduplicated");
        let library = directory.join("lib");
        fs::create_dir(&library).unwrap();
        fs::write(
            library.join("math.crb"),
            "module math\nlet answer: int = 42\n",
        )
        .unwrap();
        let root = directory.join("main.crb");
        fs::write(
            &root,
            "import \"lib/math.crb\"\nimport \"lib/../lib/math.crb\"\nlet result: int = math::answer\n",
        )
        .unwrap();

        let program = load_program(&root).unwrap();
        fs::remove_dir_all(directory).unwrap();

        assert_eq!(program.len(), 2);
        assert!(matches!(
            &program[0].input,
            ParsedInput::Let { name, .. } if name == "math::answer"
        ));
    }

    #[test]
    fn rejects_cyclic_imports() {
        let directory = temp_directory("cycle");
        let first = directory.join("first.crb");
        let second = directory.join("second.crb");
        fs::write(&first, "module first\nimport \"second.crb\"\n").unwrap();
        fs::write(&second, "module second\nimport \"first.crb\"\n").unwrap();

        let diagnostics = load_program(&first).unwrap_err();
        fs::remove_dir_all(directory).unwrap();

        assert!(matches!(
            diagnostics.as_slice(),
            [ModuleDiagnostic::Cycle(_)]
        ));
    }

    #[test]
    fn rejects_duplicate_module_namespaces() {
        let directory = temp_directory("duplicate_namespace");
        fs::write(directory.join("one.crb"), "module shared\n").unwrap();
        fs::write(directory.join("two.crb"), "module shared\n").unwrap();
        let root = directory.join("main.crb");
        fs::write(&root, "import \"one.crb\"\nimport \"two.crb\"\n").unwrap();

        let diagnostics = load_program(&root).unwrap_err();
        fs::remove_dir_all(directory).unwrap();

        assert!(matches!(
            diagnostics.as_slice(),
            [ModuleDiagnostic::DuplicateNamespace { name, .. }] if name == "shared"
        ));
    }

    #[test]
    fn type_checks_calls_to_imported_functions() {
        let directory = temp_directory("type_check");
        fs::write(
            directory.join("math.crb"),
            "module math\nfn add(value: int) -> int {\nreturn value + 1\n}\n",
        )
        .unwrap();
        let root = directory.join("main.crb");
        fs::write(
            &root,
            "import \"math.crb\"\nlet result: int = math::add(\"wrong\")\n",
        )
        .unwrap();

        let diagnostics = load_program(&root).unwrap_err();
        fs::remove_dir_all(directory).unwrap();

        assert!(matches!(
            diagnostics.as_slice(),
            [ModuleDiagnostic::Static { .. }]
        ));
    }

    #[test]
    fn qualifies_named_types_exported_by_modules() {
        let directory = temp_directory("named_type");
        fs::write(
            directory.join("models.crb"),
            "module models\ntype User { name: string }\n",
        )
        .unwrap();
        let root = directory.join("main.crb");
        fs::write(
            &root,
            "import \"models.crb\"\nlet user: models::User = models::User { name: \"Tony\" }\n",
        )
        .unwrap();

        let program = load_program(&root).unwrap();
        fs::remove_dir_all(directory).unwrap();

        assert!(matches!(
            &program[0].input,
            ParsedInput::TypeDefinition { name, .. } if name == "models::User"
        ));
    }

    #[test]
    fn qualifies_enum_types_variants_and_patterns_inside_modules() {
        let directory = temp_directory("enum_type");
        fs::write(
            directory.join("jobs.crb"),
            "module jobs\nenum JobState { Running, Done(int) }\nlet value = 99\nfn code(state: JobState) -> int {\nreturn match state { JobState::Done(value) => value, _ => 0 }\n}\n",
        )
        .unwrap();
        let root = directory.join("main.crb");
        fs::write(
            &root,
            "import \"jobs.crb\"\nlet code: int = jobs::code(jobs::JobState::Done(42))\n",
        )
        .unwrap();

        let program = load_program(&root).unwrap();
        fs::remove_dir_all(directory).unwrap();

        assert!(matches!(
            &program[0].input,
            ParsedInput::EnumDefinition { name, .. } if name == "jobs::JobState"
        ));
    }
}
