//! `LanguageEmitter` trait and two-pass lifecycle driver (`DryRun` Pass 1 + `Emit` Pass 2).

use crate::ast::{
    CheckDecl, FieldDecl, FunctionDef, InitFieldDecl, JsonDef, Module, Pipeline, RequestDecl,
    RestArtifact, RestEndpointDecl, StructDecl, StructKind, StructTypeDef,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::writer::CodeWriter;

/// Target language emitter protocol invoked by [`drive_module`] across Pass 1 (`DryRun`) and Pass 2 (`Emit`).
pub trait LanguageEmitter {
    /// Returns the single-level indentation string for this target language (defaults to 4 spaces `"    "`).
    fn indent_unit(&self) -> &'static str {
        "    "
    }

    /// Invoked at the start of a module before imports and utilities.
    fn on_module_start(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits accumulated top-level imports from `builder`.
    fn emit_imports(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits accumulated standard runtime helpers, constants, and extension helpers from `builder`.
    fn emit_utilities(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a top-level or hoisted `JsonDef` model and its allowlist JSON projector function.
    fn emit_json_def(
        &mut self,
        _json_def: &JsonDef,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a synthesized `StructTypeDef` model (`TypedDict`, `@dataclass(slots=True)`, or `interface`/`type`).
    fn emit_typedef(
        &mut self,
        _typedef: &StructTypeDef,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a synthesized REST artifact (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    fn emit_rest_artifact(
        &mut self,
        _artifact: &RestArtifact,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Invoked at the start of a `struct` declaration (`class <Name>:`).
    fn on_struct_start(
        &mut self,
        _struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the constructor (`__init__`) and precomputed `@init` field caching calls for an HTML/Raw struct.
    fn emit_init(
        &mut self,
        _struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a cached `@init` precomputed field computation method (`_init_<name>`).
    fn emit_init_field(
        &mut self,
        _struct_decl: &StructDecl,
        _init_field: &InitFieldDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@pre-validate` method (`_pre_validate`).
    fn emit_pre_validate(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a `@check <name>` boolean validation method.
    fn emit_check(
        &mut self,
        _struct_decl: &StructDecl,
        _check: &CheckDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@split-doc` pipeline method (`_split_doc`).
    fn emit_split_doc(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@key` pipeline method (`_parse_key`) for a `(dict)struct`.
    fn emit_key_pipeline(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@value` pipeline method (`_parse_value`) for a `(dict)struct` or `(table)struct`.
    fn emit_value_pipeline(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@table` pipeline method (`_table_config`) for a `(table)struct`.
    fn emit_table_pipeline(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@rows` pipeline method (`_table_rows`) for a `(table)struct`.
    fn emit_rows_pipeline(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the `@match` pipeline method (`_table_match_key`) for a `(table)struct`.
    fn emit_match_pipeline(
        &mut self,
        _struct_decl: &StructDecl,
        _pipeline: &Pipeline,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits an extracted field pipeline method (`_parse_<name>`).
    fn emit_field(
        &mut self,
        _struct_decl: &StructDecl,
        _field: &FieldDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits the public `.parse()` entrypoint method on an HTML/Raw struct.
    fn emit_start_parse(
        &mut self,
        _struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits `@request` HTTP `fetch` / `async_fetch` methods on an HTML/Raw struct.
    fn emit_request(
        &mut self,
        _struct_decl: &StructDecl,
        _request: &RequestDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a typed REST endpoint (`fetch` / `async_fetch`) on a `(rest)struct`.
    fn emit_rest_endpoint(
        &mut self,
        _struct_decl: &StructDecl,
        _endpoint: &RestEndpointDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Invoked at the end of a `struct` declaration.
    fn on_struct_end(
        &mut self,
        _struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a standalone top-level `fn` or `(raw)fn` function declaration.
    fn emit_function(
        &mut self,
        _func: &FunctionDef,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Invoked at the end of a module after all declarations have been emitted.
    fn on_module_end(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }
}

/// Executes the two-pass `LanguageEmitter` lifecycle (`DryRun` Pass 1 followed by `Emit` Pass 2),
/// returning the final emitted source string and the populated [`ModuleBuilder`].
pub fn drive_module<E: LanguageEmitter>(
    emitter: &mut E,
    module: &Module,
    prologue: Option<&str>,
    epilogue: Option<&str>,
) -> (String, ModuleBuilder) {
    let mut builder = ModuleBuilder::new();

    // Pass 1: DryRun (zero output buffer allocation, populates `builder`)
    let mut dry_writer = CodeWriter::dry_run();
    drive_single_pass(
        emitter,
        module,
        &mut builder,
        &mut dry_writer,
        prologue,
        epilogue,
    );
    debug_assert_eq!(dry_writer.capacity(), 0);

    // Pass 2: Emit (writes formatted target code using `builder` populated in Pass 1)
    let mut emit_writer = CodeWriter::with_indent(emitter.indent_unit());
    drive_single_pass(
        emitter,
        module,
        &mut builder,
        &mut emit_writer,
        prologue,
        epilogue,
    );

    (emit_writer.finish(), builder)
}

/// Executes a single pass of the module lifecycle over `module`.
pub fn drive_single_pass<E: LanguageEmitter>(
    emitter: &mut E,
    module: &Module,
    builder: &mut ModuleBuilder,
    writer: &mut CodeWriter,
    prologue: Option<&str>,
    epilogue: Option<&str>,
) {
    emitter.on_module_start(module, builder, writer);
    if writer.is_emit() {
        emitter.emit_imports(module, builder, writer);
        emitter.emit_utilities(module, builder, writer);

        if let Some(pro) = prologue.map(str::trim).filter(|s| !s.is_empty()) {
            writer.write_block(pro);
            writer.blankline();
        }
    }

    for json_def in &module.json_defs {
        emitter.emit_json_def(json_def, module, builder, writer);
    }

    for typedef in &module.typedefs {
        emitter.emit_typedef(typedef, module, builder, writer);
    }

    for artifact in &module.rest_artifacts {
        emitter.emit_rest_artifact(artifact, module, builder, writer);
    }

    for struct_decl in &module.structs {
        drive_struct(emitter, struct_decl, module, builder, writer);
    }

    for func in &module.functions {
        emitter.emit_function(func, module, builder, writer);
    }

    emitter.on_module_end(module, builder, writer);

    if writer.is_emit()
        && let Some(epi) = epilogue.map(str::trim).filter(|s| !s.is_empty())
    {
        writer.blankline();
        writer.write_block(epi);
    }
}

/// Executes the deterministic struct lifecycle for a single [`StructDecl`].
pub fn drive_struct<E: LanguageEmitter>(
    emitter: &mut E,
    struct_decl: &StructDecl,
    module: &Module,
    builder: &mut ModuleBuilder,
    writer: &mut CodeWriter,
) {
    emitter.on_struct_start(struct_decl, module, builder, writer);

    if struct_decl.kind == StructKind::Rest {
        for endpoint in &struct_decl.rest_endpoints {
            emitter.emit_rest_endpoint(struct_decl, endpoint, module, builder, writer);
        }
        emitter.on_struct_end(struct_decl, module, builder, writer);
        return;
    }

    emitter.emit_init(struct_decl, module, builder, writer);

    for init_field in &struct_decl.init_fields {
        emitter.emit_init_field(struct_decl, init_field, module, builder, writer);
    }

    if let Some(ref pre_val) = struct_decl.pre_validate {
        emitter.emit_pre_validate(struct_decl, pre_val, module, builder, writer);
    }

    for check in &struct_decl.checks {
        emitter.emit_check(struct_decl, check, module, builder, writer);
    }

    if let Some(ref split_doc) = struct_decl.split_doc {
        emitter.emit_split_doc(struct_decl, split_doc, module, builder, writer);
    }

    if let Some(ref key_pipe) = struct_decl.key_pipeline {
        emitter.emit_key_pipeline(struct_decl, key_pipe, module, builder, writer);
    }

    if let Some(ref val_pipe) = struct_decl.value_pipeline {
        emitter.emit_value_pipeline(struct_decl, val_pipe, module, builder, writer);
    }

    if let Some(ref table_pipe) = struct_decl.table_pipeline {
        emitter.emit_table_pipeline(struct_decl, table_pipe, module, builder, writer);
    }

    if let Some(ref rows_pipe) = struct_decl.rows_pipeline {
        emitter.emit_rows_pipeline(struct_decl, rows_pipe, module, builder, writer);
    }

    if let Some(ref match_pipe) = struct_decl.match_pipeline {
        emitter.emit_match_pipeline(struct_decl, match_pipe, module, builder, writer);
    }

    for field in &struct_decl.fields {
        emitter.emit_field(struct_decl, field, module, builder, writer);
    }

    emitter.emit_start_parse(struct_decl, module, builder, writer);

    for request in &struct_decl.requests {
        emitter.emit_request(struct_decl, request, module, builder, writer);
    }

    emitter.on_struct_end(struct_decl, module, builder, writer);
}
