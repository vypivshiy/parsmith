//! Pass 1 (KDL 2.0 parsing) and Pass 2 (explicit typed module imports, private per-file scopes,
//! and topological dependency closure).

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::str::FromStr;

use crate::kdl::{KdlDocument, KdlNode, NodeId, NodeRef, Span};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::loader::{LoaderError, SourceLoader};

/// The five importable top-level declaration kinds in KDL Schema DSL v2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// `define` (scalar `define KEY="val"` or block `define NAME { ... }`).
    Define,
    /// `extension Namespace { ... }`.
    Extension,
    /// `fn name { ... }` or `(raw)fn name { ... }`.
    Fn,
    /// `json Name { ... }`, `(array)json Name { ... }`, or `(dict)json Name { ... }`.
    Json,
    /// `struct Name { ... }` or `(item|list|dict|table|flat|rest|raw)struct Name { ... }`.
    Struct,
}

impl SymbolKind {
    /// Returns the canonical lowercase DSL keyword (`"define"`, `"extension"`, `"fn"`, `"json"`, `"struct"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Define => "define",
            Self::Extension => "extension",
            Self::Fn => "fn",
            Self::Json => "json",
            Self::Struct => "struct",
        }
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SymbolKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "define" => Ok(Self::Define),
            "extension" => Ok(Self::Extension),
            "fn" => Ok(Self::Fn),
            "json" => Ok(Self::Json),
            "struct" => Ok(Self::Struct),
            other => Err(format!("unknown symbol kind '{other}'")),
        }
    }
}

/// Globally unique identifier for a top-level declaration symbol across a multi-file schema graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SymbolId {
    /// Canonical logical path of the file that declares this symbol.
    pub path: String,
    /// Declaration kind (`struct`, `json`, `fn`, `define`, `extension`).
    pub kind: SymbolKind,
    /// Declared symbol name.
    pub name: String,
}

impl SymbolId {
    /// Creates a new `SymbolId`.
    pub fn new(path: impl Into<String>, kind: SymbolKind, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            name: name.into(),
        }
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "({}){}", self.kind, self.name)
        } else {
            write!(f, "{}::({}){}", self.path, self.kind, self.name)
        }
    }
}

/// A single `(kind)Name` entry inside an `import "..." { ... }` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedSymbol {
    /// Requested declaration kind.
    pub kind: SymbolKind,
    /// Requested symbol name.
    pub name: String,
    /// Source span of the `(kind)Name` child node.
    pub span: Span,
    /// Resolved target [`SymbolId`], if resolution succeeded.
    pub target: Option<SymbolId>,
}

/// A parsed and validated `import "<path>" { ... }` declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportDecl {
    /// Raw import path string as written in the KDL file.
    pub raw_path: String,
    /// Canonical logical path resolved via [`SourceLoader`], if resolution succeeded.
    pub resolved_path: Option<String>,
    /// Requested symbol imports inside the block.
    pub symbols: Vec<ImportedSymbol>,
    /// Full source span of the `import` node.
    pub span: Span,
}

/// A single parsed KDL source file and its private symbol tables.
#[derive(Debug, Clone)]
pub struct SourceUnit {
    /// Canonical logical path of this file.
    pub path: String,
    /// Raw UTF-8 source text.
    pub source: String,
    /// Indexed KDL 2.0 document tree.
    pub document: KdlDocument,
    /// Parsed `import` declarations in this file.
    pub imports: Vec<ImportDecl>,
    /// Symbols declared locally at the top level of this file (`(kind, name) -> SymbolId`).
    ///
    /// Note: Imported symbols are NEVER added to `exports` (enforcing private per-file scopes).
    pub exports: IndexMap<(SymbolKind, String), SymbolId>,
    /// Arena [`NodeId`] of the declaring top-level node for each local export.
    pub export_nodes: IndexMap<SymbolId, NodeId>,
    /// Symbols explicitly imported by this file in source order.
    pub imported_symbols: Vec<SymbolId>,
    /// Private per-file visibility scope (local `exports` + explicitly `imported_symbols`).
    pub scope: IndexMap<(SymbolKind, String), SymbolId>,
    /// Ordered root symbols for this unit (explicit imports followed by local declarations).
    pub ordered_roots: Vec<SymbolId>,
    /// Symbols from failed/cyclic imports that should not trigger secondary `E300` cascades.
    pub poisoned_symbols: HashSet<(SymbolKind, String)>,
    /// Parsed and validated `@request` specifications keyed by `@request` [`NodeId`].
    pub parsed_requests: IndexMap<NodeId, crate::request::RequestSpec>,
    /// Parsed and validated `@error` specifications keyed by `@error` [`NodeId`].
    pub parsed_errors: IndexMap<NodeId, crate::request::ErrorResponseSpec>,
}

/// A topologically ordered declaration item in a [`ResolvedProgram`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedItem {
    /// Globally unique [`SymbolId`] of this declaration.
    pub symbol: SymbolId,
    /// Arena [`NodeId`] within the defining [`SourceUnit::document`].
    pub node_id: NodeId,
    /// Cloned [`KdlNode`] (with scalar `define` properties filtered to selected symbols).
    pub node: KdlNode,
}

/// Topologically sorted dependency closure and module graph produced by Pass 1 and Pass 2.
#[derive(Debug, Clone, Default)]
pub struct ResolvedProgram {
    /// Canonical logical path of the entry module.
    pub entry_path: String,
    /// Optional module-level `@doc` string from the entry module.
    pub doc_comment: Option<String>,
    /// All loaded [`SourceUnit`]s keyed by canonical logical path.
    pub units: IndexMap<String, SourceUnit>,
    /// Deterministic topological order of all reachable [`SymbolId`]s (dependencies before dependents).
    pub topological_order: Vec<SymbolId>,
    /// Deduplicated, topologically ordered declaration items ready for downstream compiler passes.
    pub items: Vec<ResolvedItem>,
}

/// Alias for [`ResolvedProgram`].
pub type ModuleGraph = ResolvedProgram;

impl ResolvedProgram {
    /// Returns the slice of topologically ordered [`SymbolId`]s.
    #[inline]
    pub fn symbols(&self) -> &[SymbolId] {
        &self.topological_order
    }

    /// Returns the `(SymbolKind, &str)` pairs in topological order.
    pub fn symbol_names(&self) -> Vec<(SymbolKind, &str)> {
        self.topological_order
            .iter()
            .map(|s| (s.kind, s.name.as_str()))
            .collect()
    }

    /// Looks up a loaded [`SourceUnit`] by canonical path.
    #[inline]
    pub fn unit(&self, path: &str) -> Option<&SourceUnit> {
        self.units.get(path)
    }

    /// Returns the entry [`SourceUnit`].
    #[inline]
    pub fn entry_unit(&self) -> Option<&SourceUnit> {
        self.units.get(&self.entry_path)
    }
}

const JSON_PRIMITIVES: &[&str] = &[
    "bool", "float", "int", "nil", "null", "str", "(dict)", "array",
];

const VALID_STRUCT_ANNOTATIONS: &[&str] = &["item", "list", "dict", "table", "flat", "rest", "raw"];
const VALID_JSON_ANNOTATIONS: &[&str] = &["array", "dict"];
const VALID_FN_ANNOTATIONS: &[&str] = &["raw"];

/// Runs Pass 1 (KDL 2.0 parse) and Pass 2 (explicit imports) starting from `entry_path` using `loader`.
pub fn resolve_with_loader(
    entry_path: &str,
    loader: &dyn SourceLoader,
) -> (
    Option<ResolvedProgram>,
    BTreeMap<String, String>,
    Vec<Diagnostic>,
) {
    let mut resolver = ImportResolver::new(loader);
    let program = resolver.resolve_entry_path(entry_path);
    (program, resolver.sources, resolver.diagnostics)
}

/// Runs Pass 1 (KDL 2.0 parse) and Pass 2 (explicit imports) starting from an in-memory `source` string
/// and optional `path`, resolving any relative imports via `loader`.
pub fn resolve_source_with_loader(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
) -> (
    Option<ResolvedProgram>,
    BTreeMap<String, String>,
    Vec<Diagnostic>,
) {
    let mut resolver = ImportResolver::new(loader);
    let program = resolver.resolve_entry_source(source, path);
    (program, resolver.sources, resolver.diagnostics)
}

struct PendingImportNode {
    span: Span,
    raw_path: String,
    path_span: Span,
    items: Vec<(SymbolKind, String, Span)>,
}

struct ImportResolver<'a> {
    loader: &'a dyn SourceLoader,
    sources: BTreeMap<String, String>,
    units: IndexMap<String, SourceUnit>,
    active_stack: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    reported_refs: HashSet<(String, usize, SymbolKind, String)>,
}

impl<'a> ImportResolver<'a> {
    fn new(loader: &'a dyn SourceLoader) -> Self {
        Self {
            loader,
            sources: BTreeMap::new(),
            units: IndexMap::new(),
            active_stack: Vec::new(),
            diagnostics: Vec::new(),
            reported_refs: HashSet::new(),
        }
    }

    fn resolve_entry_path(&mut self, entry_path: &str) -> Option<ResolvedProgram> {
        let loaded = match self.loader.resolve(None, entry_path) {
            Ok(loaded) => loaded,
            Err(LoaderError::NotFound { path }) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E003,
                        format!("file not found: {path}"),
                        Span::EMPTY,
                    )
                    .with_path(path),
                );
                return None;
            }
            Err(err) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticCode::E003, err.to_string(), Span::EMPTY)
                        .with_path(entry_path),
                );
                return None;
            }
        };

        let root_path = loaded.path.clone();
        self.load_unit(loaded.path, loaded.source)?;
        Some(self.build_program(root_path))
    }

    fn resolve_entry_source(
        &mut self,
        source: &str,
        path: Option<&str>,
    ) -> Option<ResolvedProgram> {
        let cleaned = source.strip_prefix('\u{feff}').unwrap_or(source).to_owned();
        let logical_path = path
            .map(|p| {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    String::new()
                } else {
                    let p_obj = std::path::Path::new(trimmed);
                    if p_obj.exists() {
                        crate::loader::normalize_fs_path(p_obj)
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        trimmed.to_string()
                    }
                }
            })
            .unwrap_or_default();

        self.load_unit(logical_path.clone(), cleaned)?;
        Some(self.build_program(logical_path))
    }

    fn load_unit(&mut self, path: String, source: String) -> Option<()> {
        if self.units.contains_key(&path) {
            return Some(());
        }

        self.sources.insert(path.clone(), source.clone());

        // Pass 1: Parse KDL 2.0 source
        let doc = match crate::kdl::parse(&source) {
            Ok(doc) => doc,
            Err(kdl_err) => {
                self.diagnostics
                    .push(Diagnostic::from_kdl_error(&kdl_err, path));
                return None;
            }
        };

        let mut unit = SourceUnit {
            path: path.clone(),
            source,
            document: doc,
            imports: Vec::new(),
            exports: IndexMap::new(),
            export_nodes: IndexMap::new(),
            imported_symbols: Vec::new(),
            scope: IndexMap::new(),
            ordered_roots: Vec::new(),
            poisoned_symbols: HashSet::new(),
            parsed_requests: IndexMap::new(),
            parsed_errors: IndexMap::new(),
        };

        self.index_exports(&mut unit);
        let pending_imports = self.validate_import_syntax(&mut unit);

        self.units.insert(path.clone(), unit);
        self.active_stack.push(path.clone());

        self.resolve_unit_imports(&path, pending_imports);

        self.active_stack.pop();
        Some(())
    }

    fn index_exports(&self, unit: &mut SourceUnit) {
        for node in unit.document.roots() {
            if node.name() == "import" || node.name() == "@doc" {
                continue;
            }
            for symbol in extract_node_symbols(node, &unit.path) {
                let key = (symbol.kind, symbol.name.clone());
                if unit.exports.contains_key(&key) {
                    continue;
                }
                unit.exports.insert(key.clone(), symbol.clone());
                unit.export_nodes.insert(symbol.clone(), node.id());
                unit.scope.insert(key, symbol.clone());
                unit.ordered_roots.push(symbol.clone());

                if symbol.kind == SymbolKind::Json {
                    for inline_name in crate::linter::json::collect_inline_json_names(node) {
                        unit.scope
                            .entry((SymbolKind::Json, inline_name))
                            .or_insert_with(|| symbol.clone());
                    }
                }
            }
        }
    }

    fn validate_import_syntax(&mut self, unit: &mut SourceUnit) -> Vec<PendingImportNode> {
        let mut pending = Vec::new();
        let mut seen_raw_paths: HashSet<String> = HashSet::new();
        let mut seen_imported_items: HashSet<(SymbolKind, String)> = HashSet::new();

        for node in unit.document.roots() {
            if node.name() != "import" {
                continue;
            }

            let mut node_ok = true;

            // 1. Check type annotation on `import` itself
            if let Some(ty) = node.ty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E001,
                        format!("'import' cannot have a type annotation '({ty})'"),
                        node.type_span().unwrap_or_else(|| node.span()),
                    )
                    .with_path(unit.path.clone())
                    .with_hint("remove the type annotation before 'import'"),
                );
                node_ok = false;
            }

            // 2. Check properties on `import`
            if !node.props().is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E001,
                        "'import' does not accept properties",
                        node.props()[0].span,
                    )
                    .with_path(unit.path.clone())
                    .with_hint("remove key=value properties from 'import'"),
                );
                node_ok = false;
            }

            // 3. Check positional arguments on `import`
            let mut raw_path_opt: Option<(String, Span)> = None;
            if node.args().len() != 1 {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E001,
                        "'import' requires exactly one path argument",
                        node.span(),
                    )
                    .with_path(unit.path.clone())
                    .with_hint("example: import \"./shared.kdl\" { (struct)Shared }"),
                );
                node_ok = false;
            } else {
                let arg = &node.args()[0];
                if let Some(arg_ty) = arg.ty() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            format!(
                                "'import' path argument cannot have a type annotation '({arg_ty})'"
                            ),
                            arg.type_span.unwrap_or(arg.span),
                        )
                        .with_path(unit.path.clone()),
                    );
                    node_ok = false;
                }
                match arg.value.as_str() {
                    Some(s) if !s.trim().is_empty() => {
                        if !seen_raw_paths.insert(s.to_string()) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::E001,
                                    format!("duplicate import path '{s}'"),
                                    arg.span,
                                )
                                .with_path(unit.path.clone())
                                .with_hint("combine symbols into one import block"),
                            );
                            node_ok = false;
                        } else {
                            raw_path_opt = Some((s.to_string(), arg.span));
                        }
                    }
                    _ => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::E001,
                                "'import' requires a non-empty string path argument",
                                arg.span,
                            )
                            .with_path(unit.path.clone())
                            .with_hint("example: import \"./shared.kdl\" { (struct)Shared }"),
                        );
                        node_ok = false;
                    }
                }
            }

            // 4. Check children block on `import`
            let children = node.children();
            if children.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E001,
                        "'import' requires an explicit non-empty symbol block",
                        node.span(),
                    )
                    .with_path(unit.path.clone())
                    .with_hint("example: import \"./shared.kdl\" { (struct)Shared }"),
                );
                continue;
            }

            // 5. Validate each child symbol inside `import { ... }`
            let mut valid_items = Vec::new();
            for child in children {
                let Some(raw_kind) = child.ty() else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            format!(
                                "imported symbol '{}' requires a kind annotation",
                                child.name()
                            ),
                            child.span(),
                        )
                        .with_path(unit.path.clone())
                        .with_hint(format!(
                            "use `(struct){}` or another explicit kind (define, extension, fn, json, struct)",
                            child.name()
                        )),
                    );
                    continue;
                };

                let Ok(kind) = SymbolKind::from_str(raw_kind) else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            format!("unknown import kind '{raw_kind}'"),
                            child.type_span().unwrap_or_else(|| child.span()),
                        )
                        .with_path(unit.path.clone())
                        .with_hint("valid kinds: define, extension, fn, json, struct"),
                    );
                    continue;
                };

                if child.name().trim().is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            "imported symbol name cannot be empty",
                            child.span(),
                        )
                        .with_path(unit.path.clone()),
                    );
                    continue;
                }

                if !child.args().is_empty()
                    || !child.props().is_empty()
                    || child.has_children_block()
                {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            "import items cannot have arguments, properties, or children",
                            child.span(),
                        )
                        .with_path(unit.path.clone())
                        .with_hint(format!("use `({kind}){}`", child.name())),
                    );
                    continue;
                }

                let item_key = (kind, child.name().to_string());
                if !seen_imported_items.insert(item_key.clone()) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            format!("duplicate imported symbol: {kind} '{}'", child.name()),
                            child.span(),
                        )
                        .with_path(unit.path.clone())
                        .with_hint(format!(
                            "remove duplicate `({kind}){}` from import block",
                            child.name()
                        )),
                    );
                    continue;
                }

                valid_items.push((kind, child.name().to_string(), child.span()));
            }

            if node_ok {
                if let Some((raw_path, path_span)) = raw_path_opt {
                    pending.push(PendingImportNode {
                        span: node.span(),
                        raw_path,
                        path_span,
                        items: valid_items,
                    });
                }
            }
        }

        pending
    }

    fn resolve_unit_imports(&mut self, unit_path: &str, pending_imports: Vec<PendingImportNode>) {
        let mut seen_resolved_paths: HashSet<String> = HashSet::new();
        let mut resolved_import_decls: Vec<ImportDecl> = Vec::new();
        let mut newly_imported_symbols: Vec<SymbolId> = Vec::new();

        for pending in pending_imports {
            let from_arg = if unit_path.is_empty() {
                Some("")
            } else {
                Some(unit_path)
            };

            let loaded = match self.loader.resolve(from_arg, &pending.raw_path) {
                Ok(loaded) => loaded,
                Err(LoaderError::NoBasePath { .. }) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E003,
                            "cannot use 'import' when checking source without a file path",
                            pending.span,
                        )
                        .with_path(unit_path.to_string())
                        .with_hint("supply a file path or use a MemorySourceLoader"),
                    );
                    self.mark_poisoned(unit_path, &pending.items);
                    continue;
                }
                Err(LoaderError::NotFound { path }) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E003,
                            format!("import: file not found: {path}"),
                            pending.path_span,
                        )
                        .with_path(unit_path.to_string())
                        .with_hint(format!(
                            "check that '{}' exists relative to the importing file",
                            pending.raw_path
                        )),
                    );
                    self.mark_poisoned(unit_path, &pending.items);
                    continue;
                }
                Err(err) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E003,
                            format!("import: {err}"),
                            pending.path_span,
                        )
                        .with_path(unit_path.to_string()),
                    );
                    self.mark_poisoned(unit_path, &pending.items);
                    continue;
                }
            };

            let target_path = loaded.path.clone();

            // Check if canonical target path was already imported by an earlier `import` block in the same file
            if !seen_resolved_paths.insert(target_path.clone()) {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E001,
                        format!("duplicate import path '{}'", pending.raw_path),
                        pending.path_span,
                    )
                    .with_path(unit_path.to_string())
                    .with_hint("combine symbols into one import block"),
                );
                continue;
            }

            // Check for circular import chain (A -> B -> A or A -> B -> C -> A)
            if let Some(cycle_start) = self.active_stack.iter().position(|p| p == &target_path) {
                let chain: Vec<&str> = self.active_stack[cycle_start..]
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::once(target_path.as_str()))
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E003,
                        format!("circular import detected: {}", chain.join(" -> ")),
                        pending.span,
                    )
                    .with_path(unit_path.to_string())
                    .with_hint(
                        "break the circular dependency by moving shared declarations into a separate module",
                    ),
                );
                self.mark_poisoned(unit_path, &pending.items);
                continue;
            }

            // Load and resolve target module
            if self.load_unit(target_path.clone(), loaded.source).is_none() {
                self.mark_poisoned(unit_path, &pending.items);
                continue;
            }

            // Verify each requested `(kind)Name` exists as a top-level export in `target_path`
            let mut decl_symbols = Vec::with_capacity(pending.items.len());
            for (kind, name, item_span) in pending.items {
                let key = (kind, name.clone());
                let target_sym_opt = self
                    .units
                    .get(&target_path)
                    .and_then(|u| u.exports.get(&key).cloned());

                let Some(target_symbol) = target_sym_opt else {
                    let hint = self.missing_export_hint(&target_path, kind, &name);
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E300,
                            format!("{kind} '{name}' is not declared in {target_path}"),
                            item_span,
                        )
                        .with_path(unit_path.to_string())
                        .with_hint(hint),
                    );
                    if let Some(u) = self.units.get_mut(unit_path) {
                        u.poisoned_symbols.insert(key);
                    }
                    decl_symbols.push(ImportedSymbol {
                        kind,
                        name,
                        span: item_span,
                        target: None,
                    });
                    continue;
                };

                // Check for collision with local declaration or earlier import in `unit_path`
                let conflict_path = self
                    .units
                    .get(unit_path)
                    .and_then(|u| u.scope.get(&key))
                    .filter(|existing| **existing != target_symbol)
                    .map(|existing| existing.path.clone());

                if let Some(prev_path) = conflict_path {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E001,
                            format!(
                                "imported {kind} '{name}' conflicts with declaration in {prev_path}"
                            ),
                            item_span,
                        )
                        .with_path(unit_path.to_string())
                        .with_hint(format!("rename or remove duplicate {kind} '{name}'")),
                    );
                    continue;
                }

                if let Some(u) = self.units.get_mut(unit_path) {
                    u.scope.insert(key, target_symbol.clone());
                }
                newly_imported_symbols.push(target_symbol.clone());
                decl_symbols.push(ImportedSymbol {
                    kind,
                    name,
                    span: item_span,
                    target: Some(target_symbol),
                });
            }

            resolved_import_decls.push(ImportDecl {
                raw_path: pending.raw_path,
                resolved_path: Some(target_path),
                symbols: decl_symbols,
                span: pending.span,
            });
        }

        if let Some(unit) = self.units.get_mut(unit_path) {
            let local_exports: Vec<SymbolId> = unit.exports.values().cloned().collect();
            unit.imported_symbols = newly_imported_symbols.clone();
            let mut ordered = newly_imported_symbols;
            ordered.extend(local_exports);
            unit.ordered_roots = ordered;
            unit.imports = resolved_import_decls;
        }
    }

    fn mark_poisoned(&mut self, unit_path: &str, items: &[(SymbolKind, String, Span)]) {
        if let Some(unit) = self.units.get_mut(unit_path) {
            for (kind, name, _) in items {
                unit.poisoned_symbols.insert((*kind, name.clone()));
            }
        }
    }

    fn missing_export_hint(&self, target_path: &str, kind: SymbolKind, name: &str) -> String {
        let Some(target_unit) = self.units.get(target_path) else {
            return "only declarations local to the imported file can be imported".to_string();
        };

        // Check if `name` is declared in `target_unit.exports` under a different `SymbolKind`
        let mut matching_kinds: Vec<SymbolKind> = target_unit
            .exports
            .keys()
            .filter(|(_, exp_name)| exp_name == name)
            .map(|(exp_kind, _)| *exp_kind)
            .collect();
        matching_kinds.sort();

        if let Some(actual_kind) = matching_kinds.first() {
            return format!("use ({actual_kind}){name}");
        }

        // Check if `name` was imported by `target_unit` (private transitive import, not re-exported)
        if let Some(transitive_sym) = target_unit.scope.get(&(kind, name.to_string())) {
            return format!(
                "'{name}' is privately imported by {}, not re-exported; import `({kind}){name}` directly from {}",
                target_path, transitive_sym.path
            );
        }

        "only declarations local to the imported file can be imported".to_string()
    }

    fn build_program(&mut self, entry_path: String) -> ResolvedProgram {
        let root_symbols = self
            .units
            .get(&entry_path)
            .map(|u| u.ordered_roots.clone())
            .unwrap_or_default();

        let mut ordered: Vec<SymbolId> = Vec::new();
        let mut visiting: Vec<SymbolId> = Vec::new();
        let mut visited: HashSet<SymbolId> = HashSet::new();

        for symbol in root_symbols {
            self.visit_symbol(&symbol, &mut visiting, &mut visited, &mut ordered);
        }

        // Extract entry module's `@doc` if present
        let doc_comment = self.units.get(&entry_path).and_then(|u| {
            u.document
                .roots()
                .into_iter()
                .find(|n| n.name() == "@doc")
                .and_then(|n| n.arg(0))
                .and_then(|a| a.value.as_str())
                .map(str::to_owned)
        });

        // Track which scalar `define` keys were actually selected per `(path, NodeId)`
        let mut selected_define_names: BTreeMap<(String, NodeId), HashSet<String>> =
            BTreeMap::new();
        for sym in &ordered {
            if sym.kind == SymbolKind::Define {
                if let Some(&node_id) = self
                    .units
                    .get(&sym.path)
                    .and_then(|u| u.export_nodes.get(sym))
                {
                    selected_define_names
                        .entry((sym.path.clone(), node_id))
                        .or_default()
                        .insert(sym.name.clone());
                }
            }
        }

        let mut items = Vec::new();
        let mut emitted_nodes: HashSet<(String, NodeId)> = HashSet::new();
        let mut emitted_names: IndexMap<(SymbolKind, String), SymbolId> = IndexMap::new();

        for symbol in &ordered {
            let key = (symbol.kind, symbol.name.clone());
            if let Some(prev) = emitted_names.get(&key) {
                if prev != symbol {
                    let span = self
                        .units
                        .get(&symbol.path)
                        .and_then(|u| u.export_nodes.get(symbol))
                        .and_then(|&nid| self.units.get(&symbol.path)?.document.get_node(nid))
                        .map_or(Span::EMPTY, |n| n.span());
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E003,
                            format!(
                                "symbol collision: {} '{}' is provided by both {} and {}",
                                symbol.kind, symbol.name, prev.path, symbol.path
                            ),
                            span,
                        )
                        .with_path(symbol.path.clone()),
                    );
                    continue;
                }
            }
            emitted_names.insert(key, symbol.clone());

            let Some(unit) = self.units.get(&symbol.path) else {
                continue;
            };
            let Some(&node_id) = unit.export_nodes.get(symbol) else {
                continue;
            };

            let node_key = (symbol.path.clone(), node_id);
            if !emitted_nodes.insert(node_key.clone()) {
                continue;
            }

            let mut cloned_node = unit.document[node_id].clone();
            if symbol.kind == SymbolKind::Define
                && !cloned_node.has_children_block()
                && !cloned_node.props.is_empty()
            {
                if let Some(allowed) = selected_define_names.get(&node_key) {
                    cloned_node.props.retain(|p| allowed.contains(&p.key));
                }
            }

            items.push(ResolvedItem {
                symbol: symbol.clone(),
                node_id,
                node: cloned_node,
            });
        }

        ResolvedProgram {
            entry_path,
            doc_comment,
            units: std::mem::take(&mut self.units),
            topological_order: ordered,
            items,
        }
    }

    fn visit_symbol(
        &mut self,
        symbol: &SymbolId,
        visiting: &mut Vec<SymbolId>,
        visited: &mut HashSet<SymbolId>,
        ordered: &mut Vec<SymbolId>,
    ) {
        if visited.contains(symbol) {
            return;
        }
        if let Some(pos) = visiting.iter().position(|s| s == symbol) {
            let mut cycle: Vec<String> = visiting[pos..]
                .iter()
                .map(|s| format!("{} {}", s.kind, s.name))
                .collect();
            cycle.push(format!("{} {}", symbol.kind, symbol.name));
            let span = self
                .units
                .get(&symbol.path)
                .and_then(|u| u.export_nodes.get(symbol))
                .and_then(|&nid| self.units.get(&symbol.path)?.document.get_node(nid))
                .map_or(Span::EMPTY, |n| n.span());
            let msg = if symbol.kind == SymbolKind::Json {
                format!(
                    "dependency cycle detected (circular reference involving json definition '{}'): {}",
                    symbol.name,
                    cycle.join(" -> ")
                )
            } else {
                format!("dependency cycle detected: {}", cycle.join(" -> "))
            };
            self.diagnostics.push(
                Diagnostic::error(DiagnosticCode::E300, msg, span).with_path(symbol.path.clone()),
            );
            return;
        }

        visiting.push(symbol.clone());
        let deps = self.symbol_dependencies(symbol);
        for dep in deps {
            self.visit_symbol(&dep, visiting, visited, ordered);
        }
        visiting.pop();
        visited.insert(symbol.clone());
        ordered.push(symbol.clone());
    }

    fn symbol_dependencies(&mut self, symbol: &SymbolId) -> Vec<SymbolId> {
        let Some(unit) = self.units.get(&symbol.path) else {
            return Vec::new();
        };
        let Some(&node_id) = unit.export_nodes.get(symbol) else {
            return Vec::new();
        };

        let mut deps: IndexSet<SymbolId> = IndexSet::new();

        if symbol.kind == SymbolKind::Extension {
            return deps.into_iter().collect();
        }

        let unit_path = symbol.path.clone();
        let root_ref = unit.document.node(node_id);
        let mut nodes_to_walk = Vec::with_capacity(1 + root_ref.descendants().len());
        nodes_to_walk.push(root_ref);
        nodes_to_walk.extend(root_ref.descendants());

        // Collect lookup requests first so we don't hold an immutable borrow on `self.units` while mutating `self.diagnostics`.
        struct RefCheck {
            kind: SymbolKind,
            name: String,
            node_id: NodeId,
            span: Span,
            label: String,
        }

        let mut checks: Vec<RefCheck> = Vec::new();
        let mut direct_deps: Vec<SymbolId> = Vec::new();

        for current in nodes_to_walk {
            let cur_name = current.name();

            // 1. Custom extension call `!Namespace.op`
            if let Some(rest) = cur_name.strip_prefix('!') {
                let namespace = rest.split('.').next().unwrap_or("");
                if !namespace.is_empty() {
                    checks.push(RefCheck {
                        kind: SymbolKind::Extension,
                        name: namespace.to_string(),
                        node_id: current.id(),
                        span: current.span(),
                        label: format!("custom operation '{rest}'"),
                    });
                }
            }

            // 2. `nested <StructName>`
            if cur_name == "nested" {
                if let Some(target_name) = current.arg_value(0).and_then(|v| v.as_str()) {
                    checks.push(RefCheck {
                        kind: SymbolKind::Struct,
                        name: target_name.to_string(),
                        node_id: current.id(),
                        span: current.arg(0).map_or_else(|| current.span(), |a| a.span),
                        label: "nested".to_string(),
                    });
                }
            }

            // 3. `jsonify <SchemaName>`
            if cur_name == "jsonify" {
                if let Some(schema_name) = current.arg_value(0).and_then(|v| v.as_str()) {
                    checks.push(RefCheck {
                        kind: SymbolKind::Json,
                        name: schema_name.to_string(),
                        node_id: current.id(),
                        span: current.arg(0).map_or_else(|| current.span(), |a| a.span),
                        label: "jsonify".to_string(),
                    });
                }
            }

            // 4. `@request response="<SchemaName>"`
            if cur_name == "@request" {
                if let Some(resp_prop) = current.prop("response") {
                    if let Some(schema_name) = resp_prop.value.as_str() {
                        if !schema_name.is_empty() {
                            checks.push(RefCheck {
                                kind: SymbolKind::Json,
                                name: schema_name.to_string(),
                                node_id: current.id(),
                                span: resp_prop.value_span,
                                label: "@request".to_string(),
                            });
                        }
                    }
                }
            }

            // 5. `@error` / `@error:<suffix> <status> <SchemaName>`
            if cur_name == "@error" || cur_name.starts_with("@error:") {
                if let Some(schema_name) = current.arg_value(1).and_then(|v| v.as_str()) {
                    checks.push(RefCheck {
                        kind: SymbolKind::Json,
                        name: schema_name.to_string(),
                        node_id: current.id(),
                        span: current.arg(1).map_or_else(|| current.span(), |a| a.span),
                        label: "@error".to_string(),
                    });
                }
            }

            // 6. Block `define` expansion in pipelines
            if current.id() != node_id {
                let def_key = (SymbolKind::Define, cur_name.to_string());
                if let Some(def_sym) = unit.scope.get(&def_key) {
                    direct_deps.push(def_sym.clone());
                } else if self.is_declared_anywhere(SymbolKind::Define, cur_name) {
                    checks.push(RefCheck {
                        kind: SymbolKind::Define,
                        name: cur_name.to_string(),
                        node_id: current.id(),
                        span: current.span(),
                        label: format!("pipeline operation '{cur_name}'"),
                    });
                }
            }

            // 7. Scalar `define` references in arguments and properties
            let mut string_vals: Vec<(&str, Span)> = Vec::new();
            for arg in current.args() {
                if let Some(s) = arg.value.as_str() {
                    string_vals.push((s, arg.span));
                }
            }
            if current.id() == node_id
                && symbol.kind == SymbolKind::Define
                && !current.has_children_block()
            {
                if let Some(prop) = current.prop(&symbol.name) {
                    if let Some(s) = prop.value.as_str() {
                        string_vals.push((s, prop.value_span));
                    }
                }
            } else {
                for prop in current.props() {
                    if let Some(s) = prop.value.as_str() {
                        string_vals.push((s, prop.value_span));
                    }
                }
            }

            for (raw_str, val_span) in string_vals {
                let exact_key = (SymbolKind::Define, raw_str.to_string());
                if let Some(def_sym) = unit.scope.get(&exact_key) {
                    direct_deps.push(def_sym.clone());
                }
                for interp_name in crate::linter::top_level::extract_double_brace_tokens(raw_str)
                    .into_iter()
                    .filter(|s| crate::linter::is_define_name(s))
                {
                    let interp_key = (SymbolKind::Define, interp_name.to_string());
                    if let Some(def_sym) = unit.scope.get(&interp_key) {
                        direct_deps.push(def_sym.clone());
                    } else if self.is_declared_anywhere(SymbolKind::Define, interp_name) {
                        checks.push(RefCheck {
                            kind: SymbolKind::Define,
                            name: interp_name.to_string(),
                            node_id: current.id(),
                            span: val_span,
                            label: format!("template interpolation '{{{{{interp_name}}}}}'"),
                        });
                    }
                }
            }
        }

        for sym in direct_deps {
            if &sym != symbol {
                deps.insert(sym);
            }
        }

        for check in checks {
            if let Some(sym) = self.lookup_in_unit(
                &unit_path,
                check.kind,
                &check.name,
                check.node_id,
                check.span,
                &check.label,
            ) {
                if &sym != symbol || (check.kind == symbol.kind && check.name == symbol.name) {
                    deps.insert(sym);
                }
            }
        }

        if symbol.kind == SymbolKind::Json {
            let mut visiting_defines = HashSet::new();
            self.collect_json_dependencies(
                &unit_path,
                node_id,
                symbol,
                &mut deps,
                &mut visiting_defines,
            );
        }

        if deps.is_empty() {
            if let Some(u) = self.units.get(&unit_path) {
                if u.exports.len() == 1 {
                    for imp_sym in &u.imported_symbols {
                        if imp_sym != symbol {
                            deps.insert(imp_sym.clone());
                        }
                    }
                }
            }
        }

        deps.into_iter().collect()
    }

    fn collect_json_dependencies(
        &mut self,
        unit_path: &str,
        parent_node_id: NodeId,
        root_symbol: &SymbolId,
        deps: &mut IndexSet<SymbolId>,
        visiting_defines: &mut HashSet<SymbolId>,
    ) {
        let Some(unit) = self.units.get(unit_path) else {
            return;
        };
        let child_ids: Vec<NodeId> = unit.document[parent_node_id].children.clone();

        for child_id in child_ids {
            let (field_name, has_args, has_children, field_span, type_candidate) = {
                let u = &self.units[unit_path];
                let field_node = u.document.node(child_id);
                let field_name = field_node.name().to_string();
                let has_args = !field_node.args().is_empty();
                let has_children = field_node.has_children_block();
                let field_span = field_node.span();

                let mut type_cand: Option<(String, Span)> = None;
                if !has_children && field_name.trim_end_matches('?') != "@key" {
                    if field_name.trim_end_matches('?') == "@value" {
                        let (explicit_model, _) =
                            crate::linter::json::extract_value_model_and_array(field_node);
                        if !explicit_model.is_empty() {
                            let span = field_node
                                .arg(0)
                                .map_or_else(|| field_node.span(), |a| a.span);
                            type_cand = Some((explicit_model, span));
                        }
                    } else {
                        for arg in field_node.args() {
                            if let Some(val_str) = arg.value.as_str() {
                                if !val_str.starts_with('@') {
                                    let clean = val_str
                                        .trim_start_matches("(array)")
                                        .trim_end_matches('?')
                                        .to_string();
                                    type_cand = Some((clean, arg.span));
                                    break;
                                }
                            }
                        }
                    }
                }
                (field_name, has_args, has_children, field_span, type_cand)
            };

            if !has_args && !has_children {
                let def_opt = self.units.get(unit_path).and_then(|u| {
                    u.scope
                        .get(&(SymbolKind::Define, field_name.clone()))
                        .cloned()
                });
                if let Some(def_sym) = def_opt {
                    if &def_sym != root_symbol {
                        deps.insert(def_sym.clone());
                    }
                    if visiting_defines.insert(def_sym.clone()) {
                        let def_node_opt = self
                            .units
                            .get(&def_sym.path)
                            .and_then(|u| u.export_nodes.get(&def_sym).copied());
                        if let Some(def_node_id) = def_node_opt {
                            let def_path = def_sym.path.clone();
                            self.collect_json_dependencies(
                                &def_path,
                                def_node_id,
                                root_symbol,
                                deps,
                                visiting_defines,
                            );
                        }
                    }
                    continue;
                } else if self.is_declared_anywhere(SymbolKind::Define, &field_name) {
                    let label = format!("json field expansion '{field_name}'");
                    self.lookup_in_unit(
                        unit_path,
                        SymbolKind::Define,
                        &field_name,
                        child_id,
                        field_span,
                        &label,
                    );
                    continue;
                }
            }

            if let Some((type_name, type_span)) = type_candidate {
                if !type_name.is_empty() && !JSON_PRIMITIVES.contains(&type_name.as_str()) {
                    let label = format!("json field '{field_name}'");
                    if let Some(sym) = self.lookup_in_unit(
                        unit_path,
                        SymbolKind::Json,
                        &type_name,
                        child_id,
                        type_span,
                        &label,
                    ) {
                        if &sym != root_symbol || type_name == root_symbol.name {
                            deps.insert(sym);
                        }
                    }
                }
            }

            if has_children {
                self.collect_json_dependencies(
                    unit_path,
                    child_id,
                    root_symbol,
                    deps,
                    visiting_defines,
                );
            }
        }
    }

    fn lookup_in_unit(
        &mut self,
        unit_path: &str,
        kind: SymbolKind,
        name: &str,
        node_id: NodeId,
        span: Span,
        label: &str,
    ) -> Option<SymbolId> {
        let key = (kind, name.to_string());
        if let Some(unit) = self.units.get(unit_path) {
            if let Some(sym) = unit.scope.get(&key) {
                return Some(sym.clone());
            }
            if unit.poisoned_symbols.contains(&key) {
                return None;
            }
        }

        let report_key = (
            unit_path.to_string(),
            node_id.index(),
            kind,
            name.to_string(),
        );
        if self.reported_refs.insert(report_key) {
            let hint = if self.is_declared_anywhere(kind, name) {
                format!("add `({kind}){name}` to an import block")
            } else {
                format!("declare `{kind} {name}` above this reference or import `({kind}){name}`")
            };
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E300,
                    format!("{label} references {kind} '{name}' which is not visible in this file"),
                    span,
                )
                .with_path(unit_path.to_string())
                .with_hint(hint),
            );
        }
        None
    }

    fn is_declared_anywhere(&self, kind: SymbolKind, name: &str) -> bool {
        let key = (kind, name.to_string());
        self.units.values().any(|u| u.exports.contains_key(&key))
    }
}

fn extract_node_symbols(node: NodeRef<'_>, path: &str) -> Vec<SymbolId> {
    match node.name() {
        "struct" => {
            if let Some(ty) = node.ty() {
                if !VALID_STRUCT_ANNOTATIONS.contains(&ty) {
                    return Vec::new();
                }
            }
            node.arg_value(0)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|name| vec![SymbolId::new(path, SymbolKind::Struct, name)])
                .unwrap_or_default()
        }
        "json" => {
            if let Some(ty) = node.ty() {
                if !VALID_JSON_ANNOTATIONS.contains(&ty) {
                    return Vec::new();
                }
            }
            node.arg_value(0)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|name| vec![SymbolId::new(path, SymbolKind::Json, name)])
                .unwrap_or_default()
        }
        "fn" => {
            if let Some(ty) = node.ty() {
                if !VALID_FN_ANNOTATIONS.contains(&ty) {
                    return Vec::new();
                }
            }
            node.arg_value(0)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|name| vec![SymbolId::new(path, SymbolKind::Fn, name)])
                .unwrap_or_default()
        }
        "define" => {
            if node.ty().is_some() {
                return Vec::new();
            }
            if node.has_children_block() {
                node.arg_value(0)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|name| vec![SymbolId::new(path, SymbolKind::Define, name)])
                    .unwrap_or_default()
            } else {
                node.props()
                    .iter()
                    .map(|p| SymbolId::new(path, SymbolKind::Define, &p.key))
                    .collect()
            }
        }
        "extension" => {
            if node.ty().is_some() {
                return Vec::new();
            }
            node.arg_value(0)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|name| vec![SymbolId::new(path, SymbolKind::Extension, name)])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}
