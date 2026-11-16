//! Core compiler, linter, AST, and multi-target codegen engine for `parsmith`.

pub mod ast;
pub mod check;
pub mod codegen;
pub mod diagnostic;
pub mod gjson;
pub mod imports;
pub mod inspect;
pub mod json_to_kdl;
pub mod kdl;
pub mod linter;
pub mod loader;
pub mod lowering;
pub mod request;
pub mod typecheck;
pub mod validators;

pub use crate::kdl as parsmith_kdl;

pub use ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, ExtensionDecl, ExtensionHelper, ExtensionImport,
    ExtensionOp, ExtensionTargetBlock, ExtensionTypeSig, FieldDecl, FnParam, FunctionDef,
    InitFieldDecl, JsonDef, JsonFieldDef, JsonKind, LiteralValue, MatcherEntry, MatcherListDef,
    Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StepOp, StructDecl, StructKind, StructTypeDef, TypeDefField, TypeInfo,
    TypedStep,
};
pub use check::{
    CheckOptions, check_entries_with_loader_and_targets, check_entries_with_options, check_path,
    check_path_with_targets, check_paths, check_paths_with_options, check_paths_with_targets,
    check_source, check_source_with_loader, check_source_with_loader_and_targets,
    check_source_with_options, check_source_with_targets, check_with_loader,
    check_with_loader_and_targets, check_with_options, compile_path, compile_source,
    compile_source_with_loader, compile_source_with_options, compile_with_loader,
    compile_with_options,
};
pub use codegen::{
    Bs4Parser, Bs4Spelling, CSharpEmitter, CodeWriter, CodegenOptions, DEFAULT_PYTHON_RUNTIME_NAME,
    DEFAULT_RUNTIME_NAME, DomSpelling, ElixirEmitter, GenerateResult, GoEmitter, HelperDef, HttpIo,
    JsTsEmitter, JsTsMode, KotlinEmitter, LanguageEmitter, LxmlSpelling, ModuleBuilder,
    ParselSpelling, PhpDom, PhpDomCrawlerSpelling, PhpDomSpelling, PhpEmitter, PhpHtmlLib,
    PhpNativeSpelling, PyDomSpelling, PythonDom, PythonEmitter, PythonHtmlLib, PythonModels,
    RustEmitter, SelectolaxSpelling, Target, WriteMode, drive_module, drive_single_pass,
    drive_struct, emit_csharp_module, emit_csharp_runtime_module, emit_dts_module,
    emit_elixir_module, emit_elixir_runtime_module, emit_go_module, emit_go_runtime_module,
    emit_js_module, emit_kotlin_module, emit_kotlin_runtime_module, emit_php_module,
    emit_php_runtime_module, emit_python_module, emit_python_runtime_module, emit_rust_module,
    emit_rust_runtime_module, emit_ts_module, format_elixir_module_prefix,
    generate_entries_with_loader, generate_module, generate_path, generate_paths, generate_source,
    generate_source_with_loader, generate_with_loader, module_has_fetch, module_has_html,
    module_has_rest, module_has_table, module_has_xpath, module_uses_http, normalize_php_namespace,
    parse_target_or_shorthand, parse_target_with_libs, validate_csharp_namespace,
    validate_elixir_package_name, validate_go_package_name, validate_kotlin_package_name,
    validate_php_namespace,
};
pub use diagnostic::{CheckResult, Diagnostic, DiagnosticCode, Severity};
pub use gjson::{
    GjsonPath, GjsonPathError, GjsonSegment, GjsonSegmentKind, GjsonStep, classify_gjson_path,
    is_malformed_gjson_path, parse_gjson_segments,
};
pub use imports::{
    ImportDecl, ImportedSymbol, ModuleGraph, ResolvedItem, ResolvedProgram, SourceUnit, SymbolId,
    SymbolKind, resolve_source_with_loader, resolve_with_loader,
};
pub use inspect::{
    CheckMethodInfo, ErrorVariantInfo, ExtensionInfo, ExtensionOpInfo, FieldInfo, FunctionInfo,
    InitFieldInfo, JsonDefInfo, JsonFieldInfo, ModuleInfo, RequestInfo, StructInfo, inspect_module,
    inspect_schema, inspect_schema_path, inspect_schema_source, inspect_schema_source_with_loader,
    inspect_schema_with_loader,
};
pub use json_to_kdl::{
    JsonToKdlError, is_reserved_field_name, json_to_kdl, json_to_kdl_document, json_value_to_kdl,
    json_value_to_kdl_document, normalize_field_name, to_pascal_case,
};
pub use linter::symbols::{
    CSHARP_CANNOT_BE_RAW, CSHARP_RESERVED, ELIXIR_CANNOT_BE_RAW, ELIXIR_RESERVED,
    KOTLIN_CANNOT_BE_RAW, KOTLIN_RESERVED, PHP_CANNOT_BE_RAW, PHP_RESERVED, RUST_CANNOT_BE_RAW,
    RUST_RESERVED, SymbolFinding, SymbolRecord, SymbolScope, TARGETS, TargetSymbolKind,
    collect_program_symbol_records, is_valid_symbol, js_json_projector_name, json_field_symbol,
    lint_pass4_symbols, local_symbol, normalize_target_name, normalize_targets,
    python_json_projector_name, target_reserved_words, target_symbol_plan, to_camel_case,
    to_snake_case, top_symbol_names,
};
pub use linter::{DefineEntry, UnitEnv, did_you_mean_hint, lint_program, suggest_closest};
pub use loader::{
    FsSourceLoader, LoadedSource, LoaderError, MemorySourceLoader, MemorySourceMap, SourceLoader,
    expand_kdl_input_paths, normalize_fs_path, normalize_virtual_path, resolve_virtual_relative,
};
pub use lowering::{clean_doc, lower_program};
pub use request::{
    ErrorConditionValue, ErrorResponseSpec, HTTP_REQUEST_SIGNATURES, JsonTemplateValue,
    PlaceholderPart, PlaceholderSpec, PlaceholderStyle, PlaceholderTemplate, PlaceholderType,
    REQUEST_LINE_CONTINUATION_HINT, RequestBodyKind, RequestPayload, RequestSpec,
    err_subclass_name, find_placeholder_tokens, has_backslash_line_comment, is_error_directive,
    is_http_status, is_valid_error_suffix, leaf_key_pascal, parse_json_template, parse_to_http,
    validate_json_body,
};
pub use typecheck::{OpSig, apply_op_sig, base_type_compatible, op_signature, type_mismatch_hint};
pub use validators::{
    css_to_xpath, extract_inline_flags, normalize_regex_pattern, unverbosify_regex,
    validate_css_selector, validate_regex, validate_xpath, xpath_literal,
};

/// Returns the crate version of `parsmith`.
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
