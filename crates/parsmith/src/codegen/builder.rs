//! Idempotent `ModuleBuilder` accumulator populated during Pass 1 (`DryRun`).

use crate::kdl::Span;
use indexmap::{IndexMap, IndexSet};

use crate::ast::{ExtensionImport, Module};
use crate::codegen::writer::CodeWriter;
use crate::diagnostic::{Diagnostic, DiagnosticCode};

/// Dedents a multi-line code block by stripping leading/trailing blank lines and the common
/// leading whitespace shared by all non-empty lines (preserving relative indentation of inner blocks).
pub fn dedent_code_block(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                ""
            } else if line.len() >= min_indent {
                line[min_indent..].trim_end()
            } else {
                line.trim_end()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runtime helper definition registered in [`ModuleBuilder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperDef {
    /// Unique helper key/identifier (e.g. `"rt_assert"`, `"rt_re_search"`, `"RT_UNMATCHED_TABLE_ROW"`).
    pub name: String,
    /// Dedented target-language source code for the helper.
    pub code: String,
    /// Import lines required by this helper.
    pub imports: Vec<String>,
}

/// Language-agnostic idempotent accumulator for imports, standard runtime helpers,
/// and user `extension` helper definitions.
///
/// Populated during Pass 1 (`DryRun`) of the [`crate::codegen::emitter::LanguageEmitter`] lifecycle
/// and queried during Pass 2 (`Emit`) to emit minimal, deduplicated file headers and utility sections.
#[derive(Debug, Clone, Default)]
pub struct ModuleBuilder {
    imports: IndexSet<String>,
    std_defs: IndexMap<String, HelperDef>,
    std_imports: IndexSet<String>,
    runtime_defs: IndexMap<String, HelperDef>,
    runtime_imports: IndexSet<String>,
}

impl ModuleBuilder {
    /// Creates an empty `ModuleBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Idempotently registers a top-level import statement line.
    ///
    /// Multi-line import blocks are normalized with [`dedent_code_block`].
    pub fn require_import(&mut self, line: impl AsRef<str>) -> &mut Self {
        let raw = line.as_ref();
        let cleaned = if raw.contains('\n') {
            dedent_code_block(raw)
        } else {
            raw.trim().to_owned()
        };
        if !cleaned.is_empty() {
            self.imports.insert(cleaned);
        }
        self
    }

    /// Idempotently registers a standard runtime helper definition by `name` (first registration wins).
    pub fn require_std(&mut self, name: impl AsRef<str>, code: impl AsRef<str>) -> &mut Self {
        self.require_std_with_imports(name, code, std::iter::empty::<&str>())
    }

    /// Idempotently registers a standard runtime helper definition and its required imports by `name`
    /// (first registration wins).
    pub fn require_std_with_imports(
        &mut self,
        name: impl AsRef<str>,
        code: impl AsRef<str>,
        imports: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        let name_ref = name.as_ref();
        if self.std_defs.contains_key(name_ref) {
            return self;
        }
        let name = name_ref.to_owned();
        let imps: Vec<String> = imports.into_iter().map(Into::into).collect();
        for imp in &imps {
            self.std_imports.insert(imp.clone());
        }
        self.std_defs.insert(
            name.clone(),
            HelperDef {
                name,
                code: dedent_code_block(code.as_ref()),
                imports: imps,
            },
        );
        self
    }

    /// Registers a user `extension` helper function for runtime emission.
    ///
    /// Returns `Err` if a helper with the same `name` was already registered with conflicting
    /// code or imports.
    pub fn require_runtime(
        &mut self,
        name: impl Into<String>,
        code: impl AsRef<str>,
        imports: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<(), String> {
        let name = name.into();
        let imps: Vec<String> = imports.into_iter().map(Into::into).collect();
        let cleaned_code = dedent_code_block(code.as_ref());
        let candidate = HelperDef {
            name: name.clone(),
            code: cleaned_code,
            imports: imps.clone(),
        };

        if let Some(existing) = self.runtime_defs.get(&name) {
            if *existing != candidate {
                return Err(format!("conflicting runtime helper definition: {name}"));
            }
            return Ok(());
        }

        for imp in imps {
            self.runtime_imports.insert(imp);
        }
        self.runtime_defs.insert(name, candidate);
        Ok(())
    }

    /// Returns the ordered set of registered top-level import lines.
    pub fn imports(&self) -> &IndexSet<String> {
        &self.imports
    }

    /// Returns an iterator over the ordered registered standard helper names.
    pub fn std_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.std_defs.keys().map(String::as_str)
    }

    /// Returns the map of registered standard helper definitions.
    pub fn std_defs(&self) -> &IndexMap<String, HelperDef> {
        &self.std_defs
    }

    /// Returns the deduplicated set of import lines required by registered standard helpers.
    pub fn std_imports(&self) -> &IndexSet<String> {
        &self.std_imports
    }

    /// Returns `true` if at least one standard helper has been registered.
    pub fn has_std(&self) -> bool {
        !self.std_defs.is_empty()
    }

    /// Returns `true` if the standard helper `name` has been registered.
    pub fn has_std_helper(&self, name: &str) -> bool {
        self.std_defs.contains_key(name)
    }

    /// Returns an iterator over the ordered registered user `extension` helper names.
    pub fn runtime_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.runtime_defs.keys().map(String::as_str)
    }

    /// Returns the map of registered user `extension` helper definitions.
    pub fn runtime_defs(&self) -> &IndexMap<String, HelperDef> {
        &self.runtime_defs
    }

    /// Returns the deduplicated set of import lines required by user `extension` helpers.
    pub fn runtime_imports(&self) -> &IndexSet<String> {
        &self.runtime_imports
    }

    /// Returns `true` if at least one user `extension` helper has been registered.
    pub fn has_runtime(&self) -> bool {
        !self.runtime_defs.is_empty()
    }

    /// Merges imports, standard runtime helpers, and user `extension` helpers from `other`
    /// into `self`, returning `Err` if `other` contains a conflicting `extension` helper definition.
    pub fn merge_from(&mut self, other: &ModuleBuilder) -> Result<(), String> {
        for imp in &other.imports {
            self.imports.insert(imp.clone());
        }
        for imp in &other.std_imports {
            self.std_imports.insert(imp.clone());
        }
        for (name, def) in &other.std_defs {
            if !self.std_defs.contains_key(name) {
                for imp in &def.imports {
                    self.std_imports.insert(imp.clone());
                }
                self.std_defs.insert(name.clone(), def.clone());
            }
        }
        for def in other.runtime_defs.values() {
            self.require_runtime(def.name.clone(), &def.code, def.imports.iter().cloned())?;
        }
        for imp in &other.runtime_imports {
            self.runtime_imports.insert(imp.clone());
        }
        Ok(())
    }
}

/// Resolves and emits a user `extension` operation step (`StepOp::ExtensionCall`), registering
/// required target imports and helpers in [`ModuleBuilder`] during Pass 1 (`DryRun`) and writing
/// the interpolated target template during Pass 2 (`Emit`).
#[allow(clippy::too_many_arguments)]
pub fn emit_extension_call(
    module: &Module,
    qualified_name: &str,
    span: Span,
    target_keys: &[&str],
    missing_target_label: &str,
    prv: &str,
    nxt: &str,
    in_type: &str,
    out_type: &str,
    map_import: impl Fn(&ExtensionImport) -> String,
    map_code: impl Fn(&str) -> String,
    builder: &mut ModuleBuilder,
    writer: &mut CodeWriter,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(ext_op) = module.extension_op(qualified_name) else {
        return;
    };
    let Some(target) = target_keys.iter().find_map(|k| ext_op.target(k)) else {
        if writer.is_dry_run() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E400,
                    format!(
                        "extension operation '{qualified_name}' has no {missing_target_label} target"
                    ),
                    span,
                )
                .with_path(module.source_file.clone()),
            );
        }
        return;
    };

    if writer.is_dry_run() {
        for imp in &target.imports {
            builder.require_import(map_import(imp));
        }
        for helper in &target.helpers {
            let helper_imps: Vec<String> = helper.imports.iter().map(&map_import).collect();
            let helper_code = map_code(&helper.source);
            if let Err(err_msg) = builder.require_runtime(&helper.name, helper_code, helper_imps) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticCode::E400, err_msg, span)
                        .with_path(module.source_file.clone()),
                );
            }
        }
    } else {
        let rendered = map_code(&dedent_code_block(&target.emit))
            .replace("{{in}}", prv)
            .replace("{{out}}", nxt)
            .replace("{{in_type}}", in_type)
            .replace("{{out_type}}", out_type);
        writer.write_block(&rendered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_idempotent_registration() {
        let mut b = ModuleBuilder::new();
        b.require_import("import re")
            .require_import("import re")
            .require_import("from typing import TypedDict")
            .require_import("from typing import Any")
            .require_std_with_imports(
                "rt_assert",
                "def rt_assert(c: bool) -> None:\n    pass",
                ["import sys"],
            )
            .require_std("rt_assert", "different body ignored")
            .require_std_with_imports(
                "rt_assert",
                "different body and imports ignored",
                ["import os"],
            )
            .require_std(
                "rt_repl_map",
                "def rt_repl_map(s: str) -> str:\n    return s",
            );

        assert_eq!(
            b.imports().iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "import re",
                "from typing import TypedDict",
                "from typing import Any"
            ]
        );
        assert_eq!(
            b.std_imports()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["import sys"]
        );
        assert_eq!(b.std_names().len(), 2);
        assert_eq!(
            b.std_names().collect::<Vec<_>>(),
            vec!["rt_assert", "rt_repl_map"]
        );
        assert!(b.has_std());
        assert!(b.has_std_helper("rt_assert"));
        assert_eq!(
            b.std_defs()["rt_assert"],
            HelperDef {
                name: "rt_assert".to_owned(),
                code: "def rt_assert(c: bool) -> None:\n    pass".to_owned(),
                imports: vec!["import sys".to_owned()],
            }
        );
    }

    #[test]
    fn builder_detects_conflicting_runtime_helper() {
        let mut b = ModuleBuilder::new();
        b.require_runtime("norm", "def norm(x): return x", ["import re"])
            .unwrap();
        b.require_runtime("norm", "def norm(x): return x", ["import re"])
            .unwrap();
        assert_eq!(b.runtime_names().collect::<Vec<_>>(), vec!["norm"]);
        assert_eq!(
            b.runtime_imports()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["import re"]
        );
        let err = b
            .require_runtime("norm", "def norm(x): return x.strip()", ["import re"])
            .unwrap_err();
        assert!(err.contains("conflicting runtime helper definition: norm"));
    }

    #[test]
    fn builder_merge_from_combines_and_detects_conflicts() {
        let mut b1 = ModuleBuilder::new();
        b1.require_import("import re")
            .require_std_with_imports(
                "rt_fetch",
                "def rt_fetch(): pass",
                ["import urllib.request"],
            )
            .require_runtime("helper_a", "def helper_a(x): return x", ["import math"])
            .unwrap();

        let mut b2 = ModuleBuilder::new();
        b2.require_import("import json")
            .require_std_with_imports("rt_rest", "def rt_rest(): pass", ["import urllib.error"])
            .require_runtime("helper_a", "def helper_a(x): return x", ["import math"])
            .unwrap();
        b2.require_runtime("helper_b", "def helper_b(x): return x + 1", ["import os"])
            .unwrap();

        b1.merge_from(&b2).unwrap();
        assert!(b1.has_std_helper("rt_fetch"));
        assert!(b1.has_std_helper("rt_rest"));
        assert!(b1.std_imports().contains("import urllib.request"));
        assert!(b1.std_imports().contains("import urllib.error"));
        assert_eq!(
            b1.runtime_names().collect::<Vec<_>>(),
            vec!["helper_a", "helper_b"]
        );
        assert!(b1.runtime_imports().contains("import math"));
        assert!(b1.runtime_imports().contains("import os"));

        let mut b3 = ModuleBuilder::new();
        b3.require_runtime(
            "helper_a",
            "def helper_a(x): return x.strip()",
            ["import math"],
        )
        .unwrap();
        let err = b1.merge_from(&b3).unwrap_err();
        assert!(err.contains("conflicting runtime helper definition: helper_a"));
    }
}
