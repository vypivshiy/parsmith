mod common;

use std::collections::{BTreeMap, HashMap};

use common::TempTestDir;
use parsmith::{
    CheckResult, DiagnosticCode, FsSourceLoader, MemorySourceLoader, Severity, SourceLoader,
    SymbolId, SymbolKind, check_path, check_source, check_with_loader,
};

// ============================================================================
// 1. Pass 1: E000 KDL 2.0 Syntax Errors (with line/col and hints)
// ============================================================================

#[test]
fn e000_syntax_error_bare_keyword_reports_line_col_and_hint() {
    let src = "struct Page {\n    active true\n}\n";
    let res = check_source(src, Some("page.kdl"));

    assert!(res.has_errors());
    assert_eq!(res.diagnostics.len(), 1);
    let diag = &res.diagnostics[0];
    assert_eq!(diag.code, DiagnosticCode::E000);
    assert_eq!(diag.severity, Severity::Error);
    assert_eq!(diag.path, "page.kdl");
    assert_eq!(diag.line, 2);
    assert_eq!(diag.col, 12);
    let hint = diag.hint.as_deref().expect("expected actionable hint");
    assert!(
        hint.contains("#true"),
        "expected hint to suggest #true, got: {hint}"
    );
}

#[test]
fn e000_syntax_error_unclosed_block_and_escline_comment() {
    let unclosed = "struct Broken {\n    title {\n        css \"h1\"\n";
    let res1 = check_source(unclosed, Some("broken.kdl"));
    assert!(res1.has_errors());
    assert_eq!(res1.diagnostics[0].code, DiagnosticCode::E000);
    assert!(res1.diagnostics[0].hint.is_some());

    let escline = "struct Broken {\n    @request \\\n    // comment after backslash\n    \"GET / HTTP/1.1\"\n}\n";
    let res2 = check_source(escline, Some("escline.kdl"));
    assert!(res2.has_errors());
    assert_eq!(res2.diagnostics[0].code, DiagnosticCode::E000);
    assert_eq!(res2.diagnostics[0].line, 3);
    assert!(
        res2.diagnostics[0]
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("escline")
    );
}

#[test]
fn e000_syntax_error_in_imported_file_reports_imported_file_path_and_span() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "root.kdl",
            "import \"./broken.kdl\" { (struct)Broken }\nstruct Root { b { nested Broken } }\n",
        )
        .with_file("broken.kdl", "struct Broken {\n    flag false\n}\n");

    let res = check_with_loader("root.kdl", &vfs);
    assert!(res.has_errors());
    assert_eq!(res.diagnostics.len(), 1);
    let diag = &res.diagnostics[0];
    assert_eq!(diag.code, DiagnosticCode::E000);
    assert_eq!(diag.path, "broken.kdl");
    assert_eq!(diag.line, 2);
    assert_eq!(diag.col, 10);
    assert!(diag.hint.as_deref().unwrap_or("").contains("#false"));

    let rendered = res.format_text();
    assert!(rendered.contains("--> broken.kdl:2:10"));
    assert!(rendered.contains("flag false"));
}

// ============================================================================
// 2. Pass 2: E001 Malformed `import` Blocks
// ============================================================================

#[test]
fn e001_malformed_import_node_syntax() {
    let cases = [
        (
            "import { (struct)Foo }\n",
            "requires exactly one path argument",
        ),
        (
            "import \"./a.kdl\" \"./b.kdl\" { (struct)Foo }\n",
            "requires exactly one path argument",
        ),
        (
            "import \"   \" { (struct)Foo }\n",
            "non-empty string path argument",
        ),
        (
            "(module)import \"./a.kdl\" { (struct)Foo }\n",
            "type annotation",
        ),
        (
            "import \"./a.kdl\" alias=\"a\" { (struct)Foo }\n",
            "does not accept properties",
        ),
        (
            "import \"./shared.kdl\"\n",
            "explicit non-empty symbol block",
        ),
        (
            "import \"./shared.kdl\" {}\n",
            "explicit non-empty symbol block",
        ),
    ];

    for (kdl_source, expected_msg_substring) in cases {
        let res = check_source(kdl_source, Some("root.kdl"));
        assert!(res.has_errors(), "expected error for: {kdl_source}");
        assert_eq!(res.diagnostics[0].code, DiagnosticCode::E001);
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E001
                    && d.message.contains(expected_msg_substring)),
            "expected E001 containing '{expected_msg_substring}' for {kdl_source}, got: {:?}",
            res.diagnostics
        );
    }
}

#[test]
fn e001_import_missing_or_invalid_kind_annotation_and_child_entries() {
    let vfs = MemorySourceLoader::new()
        .with_file("shared.kdl", "struct Foo { x { css \"a\"; text } }\n")
        .with_file(
            "root.kdl",
            "import \"./shared.kdl\" {\n    BareSymbol\n    (invalid)Foo\n    (struct)WithArg \"extra\"\n}\n",
        );

    let res = check_with_loader("root.kdl", &vfs);
    assert!(res.has_errors());
    assert_eq!(res.diagnostics.len(), 3);
    assert!(
        res.diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::E001)
    );
    assert!(
        res.diagnostics[0]
            .message
            .contains("requires a kind annotation")
    );
    assert!(res.diagnostics[1].message.contains("unknown import kind"));
    assert!(
        res.diagnostics[2]
            .message
            .contains("cannot have arguments, properties, or children")
    );
}

#[test]
fn e001_duplicate_imported_symbol_and_duplicate_import_path() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "shared.kdl",
            "struct Foo { x { css \"a\"; text } }\nstruct Bar { x { css \"b\"; text } }\n",
        )
        .with_file(
            "dup_sym.kdl",
            "import \"./shared.kdl\" {\n    (struct)Foo\n    (struct)Foo\n}\n",
        )
        .with_file(
            "dup_path.kdl",
            "import \"./shared.kdl\" { (struct)Foo }\nimport \"./shared.kdl\" { (struct)Bar }\n",
        )
        .with_file(
            "dup_norm_path.kdl",
            "import \"./shared.kdl\" { (struct)Foo }\nimport \"shared.kdl\" { (struct)Bar }\n",
        );

    let res_sym = check_with_loader("dup_sym.kdl", &vfs);
    assert!(res_sym.has_errors());
    assert!(res_sym.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E001
            && d.message
                .contains("duplicate imported symbol: struct 'Foo'")
    }));

    let res_path = check_with_loader("dup_path.kdl", &vfs);
    assert!(res_path.has_errors());
    assert!(
        res_path
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E001 && d.message.contains("duplicate import path"))
    );

    let res_norm_path = check_with_loader("dup_norm_path.kdl", &vfs);
    assert!(res_norm_path.has_errors());
    assert!(
        res_norm_path
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E001 && d.message.contains("duplicate import path"))
    );
}

// ============================================================================
// 3. Pass 2: E003 Missing File & 2-Node / 3-Node Circular Imports
// ============================================================================

#[test]
fn e003_missing_imported_file_in_vfs_and_fs() {
    let vfs = MemorySourceLoader::new().with_file(
        "root.kdl",
        "import \"./does_not_exist.kdl\" { (struct)Missing }\nstruct Root { x { css \"a\"; text } }\n",
    );
    let res_vfs = check_with_loader("root.kdl", &vfs);
    assert!(res_vfs.has_errors());
    assert_eq!(res_vfs.diagnostics.len(), 1);
    assert_eq!(res_vfs.diagnostics[0].code, DiagnosticCode::E003);
    assert!(res_vfs.diagnostics[0].message.contains("file not found"));

    let tmp = TempTestDir::new("missing_import");
    let root_file = tmp.write(
        "root.kdl",
        "import \"./does_not_exist.kdl\" { (struct)Missing }\n",
    );
    let res_fs = check_path(&root_file);
    assert!(res_fs.has_errors());
    assert_eq!(res_fs.diagnostics.len(), 1);
    assert_eq!(res_fs.diagnostics[0].code, DiagnosticCode::E003);
    assert!(res_fs.diagnostics[0].message.contains("file not found"));
}

#[test]
fn e003_import_without_base_file_path_fails() {
    let res = check_source(
        "import \"./shared.kdl\" { (struct)Shared }\nstruct Root { x { css \"a\"; text } }\n",
        None,
    );
    assert!(res.has_errors());
    assert_eq!(res.diagnostics[0].code, DiagnosticCode::E003);
    assert!(res.diagnostics[0].message.contains("without a file path"));
}

#[test]
fn e003_two_node_and_three_node_circular_imports() {
    // 2-node cycle: a.kdl -> b.kdl -> a.kdl
    let vfs_2 = MemorySourceLoader::new()
        .with_file(
            "a.kdl",
            "import \"./b.kdl\" { (struct)B }\nstruct A { b { nested B } }\n",
        )
        .with_file(
            "b.kdl",
            "import \"./a.kdl\" { (struct)A }\nstruct B { a { nested A } }\n",
        );

    let res_2 = check_with_loader("a.kdl", &vfs_2);
    assert!(res_2.has_errors());
    assert_eq!(res_2.diagnostics.len(), 1);
    assert_eq!(res_2.diagnostics[0].code, DiagnosticCode::E003);
    assert!(
        res_2.diagnostics[0]
            .message
            .contains("circular import detected: a.kdl -> b.kdl -> a.kdl"),
        "unexpected message: {}",
        res_2.diagnostics[0].message
    );

    // 3-node cycle: a.kdl -> b.kdl -> c.kdl -> a.kdl
    let vfs_3 = MemorySourceLoader::new()
        .with_file(
            "a.kdl",
            "import \"./b.kdl\" { (struct)B }\nstruct A { b { nested B } }\n",
        )
        .with_file(
            "b.kdl",
            "import \"./c.kdl\" { (struct)C }\nstruct B { c { nested C } }\n",
        )
        .with_file(
            "c.kdl",
            "import \"./a.kdl\" { (struct)A }\nstruct C { a { nested A } }\n",
        );

    let res_3 = check_with_loader("a.kdl", &vfs_3);
    assert!(res_3.has_errors());
    assert_eq!(res_3.diagnostics.len(), 1);
    assert_eq!(res_3.diagnostics[0].code, DiagnosticCode::E003);
    assert!(
        res_3.diagnostics[0]
            .message
            .contains("circular import detected: a.kdl -> b.kdl -> c.kdl -> a.kdl"),
        "unexpected message: {}",
        res_3.diagnostics[0].message
    );
}

// ============================================================================
// 4. Pass 2: E300 Missing Symbol, Wrong Kind, & Private Per-File Scopes
// ============================================================================

#[test]
fn e300_import_non_existent_symbol_and_wrong_kind_mismatch() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "shared.kdl",
            "json SharedPayload { id int }\n(raw)fn clean-text { trim }\n",
        )
        .with_file(
            "missing.kdl",
            "import \"./shared.kdl\" { (struct)DoesNotExist }\n",
        )
        .with_file(
            "wrong_kind.kdl",
            "import \"./shared.kdl\" {\n    (struct)SharedPayload\n    (define)clean-text\n}\n",
        );

    let res_missing = check_with_loader("missing.kdl", &vfs);
    assert!(res_missing.has_errors());
    assert_eq!(res_missing.diagnostics.len(), 1);
    assert_eq!(res_missing.diagnostics[0].code, DiagnosticCode::E300);
    assert!(
        res_missing.diagnostics[0]
            .message
            .contains("struct 'DoesNotExist' is not declared in shared.kdl")
    );

    let res_wrong = check_with_loader("wrong_kind.kdl", &vfs);
    assert!(res_wrong.has_errors());
    assert_eq!(res_wrong.diagnostics.len(), 2);
    assert_eq!(res_wrong.diagnostics[0].code, DiagnosticCode::E300);
    assert_eq!(
        res_wrong.diagnostics[0].hint.as_deref(),
        Some("use (json)SharedPayload")
    );
    assert_eq!(res_wrong.diagnostics[1].code, DiagnosticCode::E300);
    assert_eq!(
        res_wrong.diagnostics[1].hint.as_deref(),
        Some("use (fn)clean-text")
    );
}

#[test]
fn e300_private_per_file_scope_rejects_transitive_reexport_and_leaky_references() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "common.kdl",
            "struct Common { x { css \"a\"; text } }\ndefine PIPE { css \".x\"; text }\n",
        )
        .with_file(
            "middle.kdl",
            "import \"./common.kdl\" {\n    (struct)Common\n    (define)PIPE\n}\nstruct Middle {\n    c { nested Common }\n    v { PIPE }\n}\n",
        )
        .with_file(
            "reexport_attempt.kdl",
            "import \"./middle.kdl\" { (struct)Common }\n",
        )
        .with_file(
            "leaky_struct_ref.kdl",
            "import \"./middle.kdl\" { (struct)Middle }\nstruct Root {\n    m { nested Middle }\n    leaked { nested Common }\n}\n",
        )
        .with_file(
            "leaky_define_ref.kdl",
            "import \"./middle.kdl\" { (struct)Middle }\nstruct Root {\n    m { nested Middle }\n    leaked { PIPE }\n}\n",
        );

    // 1. Attempting to import `Common` from `middle.kdl` (which only imported `Common` from `common.kdl`)
    let res_reexport = check_with_loader("reexport_attempt.kdl", &vfs);
    assert!(res_reexport.has_errors());
    assert_eq!(res_reexport.diagnostics.len(), 1);
    assert_eq!(res_reexport.diagnostics[0].code, DiagnosticCode::E300);
    assert!(
        res_reexport.diagnostics[0]
            .message
            .contains("struct 'Common' is not declared in middle.kdl")
    );
    assert!(
        res_reexport.diagnostics[0]
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("not re-exported")
    );

    // 2. Attempting to reference `Common` in `Root` when `Root` only imported `Middle`
    let res_leaky_struct = check_with_loader("leaky_struct_ref.kdl", &vfs);
    assert!(res_leaky_struct.has_errors());
    assert_eq!(res_leaky_struct.diagnostics.len(), 1);
    assert_eq!(res_leaky_struct.diagnostics[0].code, DiagnosticCode::E300);
    assert!(
        res_leaky_struct.diagnostics[0]
            .message
            .contains("struct 'Common' which is not visible in this file")
    );

    // 3. Attempting to use `PIPE` in `Root` when `Root` only imported `Middle`
    let res_leaky_def = check_with_loader("leaky_define_ref.kdl", &vfs);
    assert!(res_leaky_def.has_errors());
    assert_eq!(res_leaky_def.diagnostics.len(), 1);
    assert_eq!(res_leaky_def.diagnostics[0].code, DiagnosticCode::E300);
    assert!(
        res_leaky_def.diagnostics[0]
            .message
            .contains("define 'PIPE' which is not visible in this file")
    );
}

#[test]
fn all_dsl_v2_1_declaration_variants_satisfy_import_kinds() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "declarations.kdl",
            r##"
define SCALAR-A="1" SCALAR-B="2"
define BLOCK-DEF { trim }
extension ExtNs {
    op {
        sig str str
        py { emit #"{{out}} = {{in}}"# }
    }
}
json ObjJson { id int }
(array)json ArrJson { id int }
(dict)json DictJson { @value str }
struct UntypedStruct { f { css "a"; text } }
(item)struct ItemStruct { f { css "a"; text } }
(list)struct ListStruct { @split-doc { css-all "li" }; f { text } }
(dict)struct DictStruct { @split-doc { css-all "li" }; @key { text }; @value { text } }
(table)struct TableStruct { @table { css "table" }; @rows { css-all "tr" }; @match { text }; @value { text }; f { match { eq "A" } } }
(flat)struct FlatStruct { f { css-all "a"; text } }
(rest)struct RestStruct { @request name="get" response="ObjJson" "curl https://example.com" }
(raw)struct RawStruct { f { trim } }
fn html-fn { css "a"; text }
(raw)fn raw-fn { trim }
"##,
        )
        .with_file(
            "consumer.kdl",
            r#"
import "./declarations.kdl" {
    (define)SCALAR-A
    (define)BLOCK-DEF
    (extension)ExtNs
    (json)ObjJson
    (json)ArrJson
    (json)DictJson
    (struct)UntypedStruct
    (struct)ItemStruct
    (struct)ListStruct
    (struct)DictStruct
    (struct)TableStruct
    (struct)FlatStruct
    (struct)RestStruct
    (struct)RawStruct
    (fn)html-fn
    (fn)raw-fn
}

struct Consumer {
    a { css "a"; text; BLOCK-DEF; !ExtNs.op; fmt SCALAR-A }
    u { nested UntypedStruct }
    i { nested ItemStruct }
    j { css "script"; text; jsonify ObjJson }
}
"#,
        );

    let res = check_with_loader("consumer.kdl", &vfs);
    assert!(
        !res.has_errors(),
        "expected 0 errors, got: {:?}",
        res.diagnostics
    );

    let program = res.program.as_ref().expect("resolved program");
    // Verify SCALAR-B was not leaked into the resolved define node's properties
    let scalar_item = program
        .items
        .iter()
        .find(|item| item.symbol.name == "SCALAR-A")
        .expect("SCALAR-A item");
    assert!(scalar_item.node.has_prop("SCALAR-A"));
    assert!(!scalar_item.node.has_prop("SCALAR-B"));
}

// ============================================================================
// 5. Diamond Imports & Topological Ordering (FsSourceLoader & MemorySourceLoader)
// ============================================================================

#[test]
fn valid_diamond_imports_across_memory_source_loader() {
    let mut map = HashMap::new();
    map.insert(
        "pkg/common/d.kdl".to_string(),
        "struct D { title { css \"h1\"; text } }\n".to_string(),
    );
    map.insert(
        "pkg/left/b.kdl".to_string(),
        "import \"../common/d.kdl\" { (struct)D }\nstruct B { d { nested D } }\n".to_string(),
    );
    map.insert(
        "pkg/right/c.kdl".to_string(),
        "import \"../common/d.kdl\" { (struct)D }\nstruct C { d { nested D } }\n".to_string(),
    );
    map.insert(
        "pkg/a.kdl".to_string(),
        "import \"./left/b.kdl\" { (struct)B }\nimport \"./right/c.kdl\" { (struct)C }\nstruct A { b { nested B }; c { nested C } }\n"
            .to_string(),
    );

    let vfs = MemorySourceLoader::from(map);
    let res = check_with_loader("./pkg/a.kdl", &vfs);
    assert!(
        !res.has_errors(),
        "unexpected diagnostics: {:?}",
        res.diagnostics
    );

    let program = res.program.expect("resolved program");
    assert_eq!(
        program.topological_order,
        vec![
            SymbolId::new("pkg/common/d.kdl", SymbolKind::Struct, "D"),
            SymbolId::new("pkg/left/b.kdl", SymbolKind::Struct, "B"),
            SymbolId::new("pkg/right/c.kdl", SymbolKind::Struct, "C"),
            SymbolId::new("pkg/a.kdl", SymbolKind::Struct, "A"),
        ]
    );
    assert_eq!(program.items.len(), 4);
}

#[test]
fn valid_diamond_imports_across_fs_source_loader() {
    let tmp = TempTestDir::new("diamond_fs");
    let d_path = tmp.write("sub/d.kdl", "struct D { title { css \"h1\"; text } }\n");
    let b_path = tmp.write(
        "b.kdl",
        "import \"./sub/d.kdl\" { (struct)D }\nstruct B { d { nested D } }\n",
    );
    let c_path = tmp.write(
        "c.kdl",
        "import \"./sub/d.kdl\" { (struct)D }\nstruct C { d { nested D } }\n",
    );
    let a_path = tmp.write(
        "a.kdl",
        "import \"./b.kdl\" { (struct)B }\nimport \"./c.kdl\" { (struct)C }\nstruct A { b { nested B }; c { nested C } }\n",
    );

    let res = check_path(&a_path);
    assert!(
        !res.has_errors(),
        "unexpected diagnostics: {:?}",
        res.diagnostics
    );

    let program = res.program.expect("resolved program");
    let names: Vec<(SymbolKind, &str)> = program.symbol_names();
    assert_eq!(
        names,
        vec![
            (SymbolKind::Struct, "D"),
            (SymbolKind::Struct, "B"),
            (SymbolKind::Struct, "C"),
            (SymbolKind::Struct, "A"),
        ]
    );

    // Ensure canonical paths have no Windows verbatim `\\?\` prefix
    for sym in &program.topological_order {
        assert!(
            !sym.path.starts_with(r"\\?\"),
            "canonical path should strip verbatim prefix: {}",
            sym.path
        );
    }

    let loader = FsSourceLoader::new();
    let d_canon = loader.load(&d_path.to_string_lossy()).unwrap().path;
    let b_canon = loader.load(&b_path.to_string_lossy()).unwrap().path;
    let c_canon = loader.load(&c_path.to_string_lossy()).unwrap().path;
    let a_canon = loader.load(&a_path.to_string_lossy()).unwrap().path;
    assert_eq!(program.topological_order[0].path, d_canon);
    assert_eq!(program.topological_order[1].path, b_canon);
    assert_eq!(program.topological_order[2].path, c_canon);
    assert_eq!(program.topological_order[3].path, a_canon);
}

#[test]
fn transitive_local_helper_is_included_ahead_of_imported_public_struct() {
    let mut files = BTreeMap::new();
    files.insert(
        "shared.kdl".to_string(),
        "struct Helper { x { css \"span\"; text } }\nstruct Public { h { nested Helper } }\nstruct Unused { x { css \"div\"; text } }\n"
            .to_string(),
    );
    files.insert(
        "root.kdl".to_string(),
        "import \"./shared.kdl\" { (struct)Public }\nstruct Root { p { nested Public } }\n"
            .to_string(),
    );

    let vfs = MemorySourceLoader::from(files);
    let res = check_with_loader("root.kdl", &vfs);
    assert!(!res.has_errors());

    let program = res.program.expect("resolved program");
    assert_eq!(
        program.symbol_names(),
        vec![
            (SymbolKind::Struct, "Helper"),
            (SymbolKind::Struct, "Public"),
            (SymbolKind::Struct, "Root"),
        ]
    );
}

#[test]
fn e300_symbol_dependency_cycle_detected() {
    let src = "struct Alpha { b { nested Beta } }\nstruct Beta { a { nested Alpha } }\n";
    let res = check_source(src, Some("cycle.kdl"));
    assert!(res.has_errors());
    assert!(res.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E300
            && d.message
                .contains("dependency cycle detected: struct Alpha -> struct Beta -> struct Alpha")
    }));
}

// ============================================================================
// 6. CheckResult Text Snippet Rendering & JSON Serialization
// ============================================================================

#[test]
fn check_result_text_snippet_rendering_and_json_roundtrip() {
    let vfs = MemorySourceLoader::new()
        .with_file("shared.kdl", "json ItemData { id int }\n")
        .with_file(
            "main.kdl",
            "import \"./shared.kdl\" {\n    (struct)ItemData\n}\n",
        );

    let res = check_with_loader("main.kdl", &vfs);
    assert!(res.has_errors());

    let text = res.format_text();
    assert!(text.contains("error[E300]: struct 'ItemData' is not declared in shared.kdl"));
    assert!(text.contains("--> main.kdl:2:5"));
    assert!(text.contains("2 |     (struct)ItemData"));
    assert!(text.contains("  |     ^^^^^^^^^^^^^^^^"));
    assert!(text.contains("= hint: use (json)ItemData"));

    let json_str = res.format_json();
    let decoded: CheckResult =
        serde_json::from_str(&json_str).expect("deserialize CheckResult from JSON");
    assert_eq!(decoded, res);
    assert_eq!(decoded.diagnostics.len(), 1);
    assert_eq!(decoded.diagnostics[0].code, DiagnosticCode::E300);
    assert_eq!(decoded.diagnostics[0].severity, Severity::Error);
    assert_eq!(decoded.diagnostics[0].path, "main.kdl");
    assert_eq!(decoded.diagnostics[0].line, 2);
    assert_eq!(decoded.diagnostics[0].col, 5);
    assert_eq!(
        decoded.diagnostics[0].hint.as_deref(),
        Some("use (json)ItemData")
    );
}
