use parsmith::{
    BaseType, CheckOptions, CodegenOptions, Cond, CondOp, DiagnosticCode, LiteralValue,
    MemorySourceLoader, PhpHtmlLib, RawMode, RestArtifact, StepOp, StructKind, Target, TypeInfo,
    check_source, check_source_with_targets, check_with_loader, check_with_options, compile_source,
    compile_with_loader, inspect_schema_source, inspect_schema_with_loader, is_valid_symbol,
    js_json_projector_name, json_field_symbol, normalize_targets, parse_target_or_shorthand,
    parse_target_with_libs, python_json_projector_name,
};
use serde::Serialize;

// ============================================================================
// 1. Pass 4: 4-Ecosystem Symbol Policy & Target Filtering (`E402`, `E403`)
// ============================================================================

#[test]
fn targets_are_normalized_and_unknown_targets_rejected() {
    let normalized = normalize_targets(Some(["py", "js", "ts", "typescript", "go", "rust", "py"]))
        .expect("valid target aliases");
    assert_eq!(normalized, vec!["python", "javascript", "go", "rust"]);

    let normalized_new = normalize_targets(Some([
        "ex",
        "elixir",
        "php",
        "php-native",
        "php-dom-crawler",
        "cs",
        "csharp",
        "kt",
        "kotlin",
    ]))
    .expect("valid new target aliases");
    assert_eq!(normalized_new, vec!["elixir", "php", "csharp", "kotlin"]);

    let default_all = normalize_targets::<[&str; 0], &str>(None).expect("default targets");
    assert_eq!(default_all, vec!["python", "javascript", "go", "rust"]);

    let err = normalize_targets(Some(["ruby"])).unwrap_err();
    assert!(err.contains("Unknown target 'ruby'"));
}

#[test]
fn new_targets_and_php_html_lib_parsing() {
    assert_eq!("elixir".parse::<Target>().unwrap(), Target::Elixir);
    assert_eq!("ex".parse::<Target>().unwrap(), Target::Elixir);
    assert_eq!(Target::Elixir.as_str(), "elixir");
    assert_eq!(Target::Elixir.ecosystem_name(), "elixir");
    assert_eq!(Target::Elixir.extension(), "ex");

    assert_eq!("php".parse::<Target>().unwrap(), Target::Php);
    assert_eq!(Target::Php.as_str(), "php");
    assert_eq!(Target::Php.ecosystem_name(), "php");
    assert_eq!(Target::Php.extension(), "php");

    assert_eq!("csharp".parse::<Target>().unwrap(), Target::CSharp);
    assert_eq!("cs".parse::<Target>().unwrap(), Target::CSharp);
    assert_eq!(Target::CSharp.as_str(), "csharp");
    assert_eq!(Target::CSharp.ecosystem_name(), "csharp");
    assert_eq!(Target::CSharp.extension(), "cs");

    assert_eq!("kotlin".parse::<Target>().unwrap(), Target::Kotlin);
    assert_eq!("kt".parse::<Target>().unwrap(), Target::Kotlin);
    assert_eq!(Target::Kotlin.as_str(), "kotlin");
    assert_eq!(Target::Kotlin.ecosystem_name(), "kotlin");
    assert_eq!(Target::Kotlin.extension(), "kt");

    assert_eq!("native".parse::<PhpHtmlLib>().unwrap(), PhpHtmlLib::Native);
    assert_eq!("ext-dom".parse::<PhpHtmlLib>().unwrap(), PhpHtmlLib::Native);
    assert_eq!(
        "dom-crawler".parse::<PhpHtmlLib>().unwrap(),
        PhpHtmlLib::DomCrawler
    );
    assert_eq!(
        "symfony".parse::<PhpHtmlLib>().unwrap(),
        PhpHtmlLib::DomCrawler
    );
    assert_eq!(PhpHtmlLib::Native.as_str(), "native");
    assert_eq!(PhpHtmlLib::Native.backend_name(), "php-native");
    assert_eq!(PhpHtmlLib::DomCrawler.as_str(), "dom-crawler");
    assert_eq!(PhpHtmlLib::DomCrawler.backend_name(), "php-dom-crawler");

    assert_eq!(
        parse_target_with_libs("php-native").unwrap(),
        (Target::Php, None, Some(PhpHtmlLib::Native))
    );
    assert_eq!(
        parse_target_with_libs("php-dom-crawler").unwrap(),
        (Target::Php, None, Some(PhpHtmlLib::DomCrawler))
    );
    assert_eq!(
        parse_target_or_shorthand("ex").unwrap(),
        (Target::Elixir, None)
    );
    assert_eq!(
        parse_target_or_shorthand("cs").unwrap(),
        (Target::CSharp, None)
    );
    assert_eq!(
        parse_target_or_shorthand("kt").unwrap(),
        (Target::Kotlin, None)
    );

    let opts = CodegenOptions::new(Target::Php).with_php_html_lib(PhpHtmlLib::DomCrawler);
    assert_eq!(opts.target, Target::Php);
    assert_eq!(opts.php_html_lib, PhpHtmlLib::DomCrawler);
}

#[test]
fn ecosystem_reserved_keywords_and_json_field_symbol_mappings() {
    // Rust raw-escapable keywords are valid symbols, while non-raw keywords are rejected
    assert!(is_valid_symbol("rust", "ref"));
    assert!(is_valid_symbol("rust", "type"));
    assert!(is_valid_symbol("rust", "pub"));
    assert!(is_valid_symbol("rust", "match"));
    assert!(!is_valid_symbol("rust", "self"));
    assert!(!is_valid_symbol("rust", "Self"));
    assert!(!is_valid_symbol("rust", "super"));
    assert!(!is_valid_symbol("rust", "crate"));
    assert!(!is_valid_symbol("rust", "_"));

    // Python
    assert!(is_valid_symbol("python", "id"));
    assert!(is_valid_symbol("python", "ref"));
    assert!(is_valid_symbol("python", "type"));
    assert!(is_valid_symbol("python", "pub"));
    assert!(!is_valid_symbol("python", "class"));
    assert!(!is_valid_symbol("python", "def"));

    // Go
    assert!(is_valid_symbol("go", "id"));
    assert!(!is_valid_symbol("go", "func"));
    assert!(!is_valid_symbol("go", "type"));
    assert!(!is_valid_symbol("go", "struct"));

    // JSON field symbol mappings across all 4 targets
    assert_eq!(json_field_symbol("user-id", "python"), "user_id");
    assert_eq!(json_field_symbol("user-id", "javascript"), "userId");
    assert_eq!(json_field_symbol("user-id", "go"), "UserId");
    assert_eq!(json_field_symbol("user-id", "rust"), "user_id");

    // 'type' is valid in all 4 targets (Go exports PascalCase 'Type', Rust uses r#type)
    for target in ["python", "javascript", "go", "rust"] {
        let sym = json_field_symbol("type", target);
        assert!(is_valid_symbol(target, &sym));
    }

    assert!(!is_valid_symbol(
        "python",
        &json_field_symbol("class", "python")
    ));
    assert!(!is_valid_symbol(
        "javascript",
        &json_field_symbol("class", "javascript")
    ));
    assert!(!is_valid_symbol(
        "javascript",
        &json_field_symbol("function", "javascript")
    ));
    assert!(!is_valid_symbol("rust", &json_field_symbol("self", "rust")));

    // Projector naming helpers
    assert_eq!(python_json_projector_name("Episode"), "rt_project_episode");
    assert_eq!(
        python_json_projector_name("CatalogReleasesResponse"),
        "rt_project_catalog_releases_response"
    );
    assert_eq!(js_json_projector_name("Episode"), "rtProjectEpisode");
    assert_eq!(
        js_json_projector_name("CatalogReleasesResponse"),
        "rtProjectCatalogReleasesResponse"
    );
}

#[test]
fn pass4_case_folded_collisions_and_target_filtering_end_to_end() {
    // 1. Normalized field and module name collisions across targets
    let src_collisions = r#"
struct Names {
    foo-bar { css "a"; text }
    foo_bar { css "b"; text }
}
(raw)fn names { trim }
"#;
    let res_col = check_source(src_collisions, Some("collisions.kdl"));
    let messages: Vec<&str> = res_col.errors().map(|d| d.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("python symbol collision") && m.contains("foo_bar")),
        "expected python field collision, got: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go symbol collision") && m.contains("Names")),
        "expected go struct/fn collision, got: {messages:?}"
    );

    // 2. Keyword and placeholder case-folded collisions
    let src_kw_ph = r#"
(raw)fn class { trim }
json R { ok bool }
(rest)struct API {
    @request name=get response=R """
    GET /u?a={{page-num}}&b={{page_num}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let res_kw_ph = check_source(src_kw_ph, Some("kw_ph.kdl"));
    let kw_msgs: Vec<&str> = res_kw_ph.errors().map(|d| d.message.as_str()).collect();
    assert!(
        kw_msgs
            .iter()
            .any(|m| m.contains("invalid python identifier 'class'"))
    );
    assert!(
        kw_msgs.iter().any(|m| m.contains("placeholder")
            && m.contains("collision")
            && m.contains("page_num"))
    );

    // 3. @request and @check sharing method namespace on same struct
    let src_method_col = r#"
struct API {
    @request name=validate """
    GET / HTTP/1.1
    Host: example.com
    """
    @check validate { css ".ok"; to-bool }
    title { css "h1"; text }
}
"#;
    let res_mcol = check_source(src_method_col, Some("method_col.kdl"));
    assert!(
        res_mcol
            .errors()
            .any(|d| d.code == DiagnosticCode::E402 && d.message.contains("symbol collision"))
    );

    // 4. Target filtering: 'self' field is valid when filtered to `python`, but rejected on `rust` (and default 4-target check)
    let src_self_field = "struct Data {\n    self { css \"a\"; text }\n}\n";
    let res_all = check_source(src_self_field, Some("data.kdl"));
    assert!(res_all.errors().any(|d| {
        d.code == DiagnosticCode::E403 && d.message.contains("invalid rust identifier 'self'")
    }));

    let res_py_only = check_source_with_targets(src_self_field, Some("data.kdl"), Some(["python"]));
    assert!(
        !res_py_only.has_errors(),
        "expected no errors when filtered to python, got: {:?}",
        res_py_only.diagnostics
    );

    // 5. Rust escapable keywords (`ref`, `type`, `pub`) pass in all 4 targets
    let src_escapable = r#"
struct Data {
    ref { css "a"; text }
    type { css "b"; text }
}
(rest)struct API {
    @request name=get """
    GET /?pub={{pub}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let res_esc = check_source(src_escapable, Some("escapable.kdl"));
    assert!(
        !res_esc.has_errors(),
        "expected 0 errors for escapable keywords, got: {:?}",
        res_esc.diagnostics
    );

    // 6. E403 inside inline JSON fields and inline @value blocks
    let src_inline_e403 = r#"
json Root {
    nested_obj {
        url: str
        class int
    }
    translations (dict) {
        @key str
        @value {
            @type str
            function str
            self int
        }
    }
}
(dict)json TopDict {
    @key str
    @value {
        @context str
        def bool
    }
}
"#;
    let res_inline_e403 = check_source(src_inline_e403, Some("inline_e403.kdl"));
    let e403_msgs: Vec<&str> = res_inline_e403
        .errors()
        .filter(|d| d.code == DiagnosticCode::E403)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        e403_msgs
            .iter()
            .any(|m| m.contains("invalid python identifier 'class'"))
    );
    assert!(
        e403_msgs
            .iter()
            .any(|m| m.contains("invalid javascript identifier 'function'"))
    );
    assert!(
        e403_msgs
            .iter()
            .any(|m| m.contains("invalid rust identifier 'self'"))
    );
    assert!(
        e403_msgs
            .iter()
            .any(|m| m.contains("invalid python identifier 'def'"))
    );
}

// ============================================================================
// 2. Pass 4: Cross-Reference & Cycle Detection (`E300`, `E301`)
// ============================================================================

#[test]
fn pass4_cross_references_and_recursive_cycles() {
    // 1. Self-recursive struct (`struct A { f { nested A } }`) -> E300
    let self_struct = "struct A {\n    f { nested A }\n}\n";
    let res_ss = check_source(self_struct, Some("self_struct.kdl"));
    assert!(
        res_ss
            .errors()
            .any(|d| d.code == DiagnosticCode::E300 && d.message.contains("cycle")),
        "expected E300 cycle for self-recursive struct, got: {:?}",
        res_ss.diagnostics
    );

    // 2. Self-recursive json (`json A { x A }`) -> E300
    let self_json = "json A {\n    x A\n}\n";
    let res_sj = check_source(self_json, Some("self_json.kdl"));
    assert!(
        res_sj.errors().any(|d| d.code == DiagnosticCode::E300),
        "expected E300 cycle for self-recursive json, got: {:?}",
        res_sj.diagnostics
    );

    // 3. Cycle between parent json and inline hoisted child (`json A { sub { back A } }`) -> E300
    let inline_cycle = r#"
json A {
    sub {
        back A
    }
}
"#;
    let res_ic = check_source(inline_cycle, Some("inline_cycle.kdl"));
    assert!(
        res_ic.errors().any(|d| {
            d.code == DiagnosticCode::E300 && d.message.contains("circular reference")
        }),
        "expected E300 circular reference for inline json cycle, got: {:?}",
        res_ic.diagnostics
    );

    // 4. Undefined json reference inside inline @value block -> E300
    let undef_in_val = r#"
json AnimeResponse {
    translations (dict) {
        @key str
        @value {
            author NonExistentModel
        }
    }
}
"#;
    let res_uv = check_source(undef_in_val, Some("undef_val.kdl"));
    assert!(
        res_uv
            .errors()
            .any(|d| { d.code == DiagnosticCode::E300 && d.message.contains("NonExistentModel") })
    );

    // 5. Undefined @init reference -> E301
    let undef_init = "struct S {\n    f { @missing_init; text }\n}\n";
    let res_ui = check_source(undef_init, Some("undef_init.kdl"));
    assert!(
        res_ui
            .errors()
            .any(|d| d.code == DiagnosticCode::E301 && d.message.contains("missing_init"))
    );
}

// ============================================================================
// 3. Pass 5: Post-Order Inline JSON & `@value` Hoisting & Collision (`E001`)
// ============================================================================

#[test]
fn pass5_post_order_inline_json_and_dict_value_hoisting() {
    // 1. Multi-level named dict & @value recursive hoisting (leaf-first post-order)
    let src_named = r#"
json AnimeResponse {
    translations (dict)Translation {
        @key str
        @value TranslationValue {
            episodes (dict)EpisodeMap {
                @key int
                @value EpisodeValue {
                    link str
                    screenshots @skip
                }
            }
            is_active bool
        }
    }
}
"#;
    let module_named = compile_source(src_named, Some("anime.kdl")).expect("compile named dict");
    let json_names: Vec<&str> = module_named
        .json_defs
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(
        json_names,
        vec!["EpisodeValue", "TranslationValue", "AnimeResponse"]
    );
    assert!(module_named.json_defs[0].is_hoisted);
    assert!(module_named.json_defs[1].is_hoisted);
    assert!(!module_named.json_defs[2].is_hoisted);

    let ep_val = &module_named.json_defs[0];
    assert_eq!(ep_val.fields[0].name, "link");
    assert_eq!(ep_val.fields[0].type_info.base, BaseType::String);
    assert!(ep_val.fields[1].skip);

    let trans_val = &module_named.json_defs[1];
    assert!(trans_val.fields[0].is_dict);
    assert_eq!(
        trans_val.fields[0].key_type.as_ref().unwrap().base,
        BaseType::Int
    );
    assert_eq!(
        trans_val.fields[0].value_type.as_ref().unwrap().ref_name(),
        Some("EpisodeValue")
    );

    // 2. Multi-level anonymous recursive hoisting (`{Parent}{Field}Value`)
    let src_anon = r#"
json Root {
    data (dict) {
        @key str
        @value {
            episodes (dict) {
                @key int
                @value {
                    leaf_str str
                }
            }
        }
    }
}
"#;
    let module_anon = compile_source(src_anon, Some("anon.kdl")).expect("compile anon dict");
    let anon_names: Vec<&str> = module_anon
        .json_defs
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(
        anon_names,
        vec!["RootDataValueEpisodesValue", "RootDataValue", "Root"]
    );

    // 3. Top-level `(dict)json` with optional modifiers and nested inline object inside `@value`
    let src_top_dict = r#"
(dict)json Translations {
    @key str
    @value? TranslationValue {
        wire_url str from="url"
        backup_url str? @omitempty
        internal_id int @skip
        nested_meta {
            version int
        }
    }
}
"#;
    let module_top = compile_source(src_top_dict, Some("top_dict.kdl")).expect("compile top dict");
    let top_names: Vec<&str> = module_top
        .json_defs
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(
        top_names,
        vec![
            "TranslationValueNestedMeta",
            "TranslationValue",
            "Translations"
        ]
    );
    let top_def = &module_top.json_defs[2];
    assert!(top_def.is_dict);
    let val_ti = top_def.value_type.as_ref().expect("value_type");
    assert!(val_ti.is_optional);
    assert_eq!(val_ti.ref_name(), Some("TranslationValue"));

    // 4. Synthesized anonymous inline schema name colliding with top-level `json` -> E001
    let src_synth_collision = r#"
json RootDataValue {
    id int
}
json Root {
    data (dict) {
        @key str
        @value {
            link str
        }
    }
}
"#;
    let res_col = check_source(src_synth_collision, Some("collision.kdl"));
    assert!(res_col.errors().any(|d| {
        d.code == DiagnosticCode::E001
            && d.message
                .contains("duplicate json definition 'RootDataValue'")
    }));
}

// ============================================================================
// 4. Pass 5: Extension Generics (`T`) & `ts` -> `js` Target Fallback
// ============================================================================

#[test]
fn pass5_extension_generic_binding_and_ts_to_js_fallback() {
    let src = r##"
extension Utils {
    to-base64 {
        sig str str
        py {
            import "from base64 import b64encode"
            emit #"{{out}} = b64encode({{in}}.encode("utf-8")).decode("ascii")"#
        }
        js {
            emit #"const {{out}} = btoa({{in}});"#
        }
    }
    truthy {
        sig T bool
        py { emit #"{{out}} = bool({{in}})"# }
        js { emit #"const {{out}} = Boolean({{in}});"# }
        ts { emit #"const {{out}}: boolean = Boolean({{in}});"# }
    }
    identity {
        sig T T
        py { emit #"{{out}} = {{in}}"# }
        js { emit #"const {{out}} = {{in}};"# }
    }
}

(raw)fn encode { !Utils.to-base64 }

struct Page {
    @check is-ok {
        css ".status"
        !Utils.truthy
    }
    scalar-id {
        css ".id"
        text
        !Utils.identity
    }
    list-tags {
        css-all ".tag"
        text
        !Utils.identity
    }
}
"##;
    let module = compile_source(src, Some("ext.kdl")).expect("compile extension schema");

    // 1. Verify `ts` falls back to `js` on `to-base64`, while `truthy` uses its explicit `ts` block
    let to_b64 = module.extension_op("Utils.to-base64").expect("to-base64");
    assert_eq!(
        to_b64.target("ts").unwrap().emit,
        "const {{out}} = btoa({{in}});"
    );
    let truthy = module.extension_op("Utils.truthy").expect("truthy");
    assert_eq!(
        truthy.target("ts").unwrap().emit,
        "const {{out}}: boolean = Boolean({{in}});"
    );

    // 2. Verify generic `T` binding per call site
    let encode_fn = module.function_def("encode").expect("encode fn");
    assert_eq!(encode_fn.return_type, TypeInfo::string());

    let page = module.struct_decl("Page").expect("Page struct");
    assert_eq!(page.checks[0].pipeline.output, TypeInfo::bool());

    let scalar_field = page.fields.iter().find(|f| f.name == "scalar-id").unwrap();
    assert_eq!(scalar_field.type_info, TypeInfo::string());
    assert!(!scalar_field.type_info.is_array);

    let list_field = page.fields.iter().find(|f| f.name == "list-tags").unwrap();
    assert_eq!(list_field.type_info.base, BaseType::String);
    assert!(list_field.type_info.is_array);

    // 3. Verify `ex`/`elixir`, `php`, `cs`/`csharp`, `kt`/`kotlin` extension targets and duplicate alias detection
    let multi_ext_src = r##"
extension Multi {
    clean {
        sig str str
        elixir { emit #"{{out}} = String.trim({{in}})"# }
        php { emit #"{{out}} = trim({{in}});"# }
        csharp { emit #"var {{out}} = {{in}}.Trim();"# }
        kotlin { emit #"val {{out}} = {{in}}.trim()"# }
    }
}
"##;
    let multi_mod = compile_source(multi_ext_src, Some("multi.kdl")).expect("compile multi ext");
    let clean_op = multi_mod.extension_op("Multi.clean").expect("Multi.clean");
    assert_eq!(
        clean_op.target("ex").unwrap().emit,
        "{{out}} = String.trim({{in}})"
    );
    assert_eq!(
        clean_op.target("elixir").unwrap().emit,
        "{{out}} = String.trim({{in}})"
    );
    assert_eq!(
        clean_op.target("php").unwrap().emit,
        "{{out}} = trim({{in}});"
    );
    assert_eq!(
        clean_op.target("cs").unwrap().emit,
        "var {{out}} = {{in}}.Trim();"
    );
    assert_eq!(
        clean_op.target("csharp").unwrap().emit,
        "var {{out}} = {{in}}.Trim();"
    );
    assert_eq!(
        clean_op.target("kt").unwrap().emit,
        "val {{out}} = {{in}}.trim()"
    );
    assert_eq!(
        clean_op.target("kotlin").unwrap().emit,
        "val {{out}} = {{in}}.trim()"
    );

    let dup_alias_src = r##"
extension Dup {
    clean {
        sig str str
        ex { emit #"{{out}} = {{in}}"# }
        elixir { emit #"{{out}} = {{in}}"# }
    }
}
"##;
    let dup_res = check_source(dup_alias_src, Some("dup.kdl"));
    assert!(dup_res.errors().any(
        |d| d.code == DiagnosticCode::E402 && d.message.contains("duplicate extension target")
    ));
}

// ============================================================================
// 5. Pass 5: `OpSig` Pipeline Type Inference & Checking (`E100`)
// ============================================================================

#[test]
fn pass5_pipeline_type_checking_e100() {
    // 1. String op directly on DOCUMENT without text/raw/attr -> E100
    let doc_to_trim = "struct S {\n    f { css \"h1\"; trim }\n}\n";
    let res1 = check_source(doc_to_trim, Some("e100_1.kdl"));
    let d1 = res1
        .errors()
        .find(|d| d.code == DiagnosticCode::E100)
        .expect("expected E100 for css -> trim");
    assert!(
        d1.message
            .contains("'trim' does not accept DOCUMENT; expected STRING")
    );
    assert!(
        d1.hint
            .as_deref()
            .unwrap_or("")
            .contains("add 'text', 'raw', or 'attr'")
    );

    // 2. Solo `to-bool` on DOCUMENT without preceding operation -> E100
    let solo_bool = "struct S {\n    f { to-bool }\n}\n";
    let res2 = check_source(solo_bool, Some("e100_2.kdl"));
    assert!(res2.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("'to-bool' requires a preceding operation")
    }));

    // 3. List operations (`first`, `index`, `slice`, `len`, `unique`, `join`, `filter`) on scalar -> E100
    for list_op in [
        "first",
        "last",
        "index 0",
        "slice 0 2",
        "len",
        "unique",
        "join \",\"",
        "filter { eq \"a\" }",
    ] {
        let src = format!(
            "struct S {{\n    f {{\n        css \"a\"\n        text\n        {list_op}\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("list_op.kdl"));
        assert!(
            res.errors().any(|d| d.code == DiagnosticCode::E100),
            "expected E100 for '{list_op}' on scalar STRING, got: {:?}",
            res.diagnostics
        );
    }

    // 4. `@check` not returning BOOL -> E100
    let bad_check =
        "struct S {\n    @check is_ok { css \"h1\"; text }\n    f { css \"h1\"; text }\n}\n";
    let res_chk = check_source(bad_check, Some("bad_check.kdl"));
    assert!(res_chk.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("@check is_ok must return BOOL, got STRING")
    }));

    // 5. `fallback` type mismatches (`fallback {}` on scalar, `fallback #null` on DOCUMENT/BOOL, scalar literal mismatch) -> E100
    let bad_fallbacks = [
        "struct S { f { css \"a\"; text; fallback {} } }",
        "struct S { f { css \"a\"; fallback #null } }",
        "struct S { f { css \"a\"; to-bool; fallback #null } }",
        "struct S { f { css \"a\"; text; fallback 0 } }",
        "struct S { f { css \"a\"; text; to-int; fallback \"default\" } }",
        "struct S { f { css-all \"a\"; text; fallback \"scalar\" } }",
    ];
    for src in bad_fallbacks {
        let res = check_source(src, Some("fb.kdl"));
        assert!(
            res.errors().any(|d| d.code == DiagnosticCode::E100),
            "expected E100 for fallback in: {src}, got: {:?}",
            res.diagnostics
        );
    }

    // 6. `nested` type mismatches ((raw)struct with DOCUMENT, (raw)struct with LIST_STRING, HTML struct with STRING) -> E100
    let raw_with_doc = r##"
(raw)struct ChildRaw { v { re #"v=(\d+)"# } }
struct ParentHtml {
    field {
        css "div"
        nested ChildRaw
    }
}
"##;
    let res_rwd = check_source(raw_with_doc, Some("rwd.kdl"));
    assert!(res_rwd.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("target is a (raw)struct and requires STRING input")
    }));

    let raw_with_list = r##"
(raw)struct ChildRaw { v { re #"v=(\d+)"# } }
(raw)struct ParentRaw {
    field {
        re-all #"v=(\d+)"#
        nested ChildRaw
    }
}
"##;
    let res_rwl = check_source(raw_with_list, Some("rwl.kdl"));
    assert!(res_rwl.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("'nested' does not accept STRING; expected DOCUMENT")
    }));

    // 7. Extension call with incompatible input type -> E100
    let ext_mismatch = r#"
extension Math {
    double {
        sig int int
        py { emit "{{out}} = {{in}} * 2" }
    }
}
(raw)fn bad_run { !Math.double }
"#;
    let res_ext = check_source(ext_mismatch, Some("ext_mm.kdl"));
    assert!(res_ext.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("custom operation 'Math.double' does not accept STRING; expected INT")
    }));
}

// ============================================================================
// 6. Algebraic AST Lowering Verification on Canonical Schemas
// ============================================================================

#[test]
fn pass5_algebraic_ast_lowering_books_quotes_imdb_and_rest() {
    let books_kdl = include_str!("../../../tests/integration/schemas/books_toscrape.kdl");
    let quotes_kdl = include_str!("../../../tests/integration/schemas/quotes_toscrape.kdl");
    let imdb_kdl = include_str!("../../../tests/integration/schemas/imdb_nextjs.kdl");

    // 1. books_toscrape.kdl
    let books_mod =
        compile_source(books_kdl, Some("books_toscrape.kdl")).expect("compile books_toscrape.kdl");
    assert_eq!(books_mod.structs.len(), 3);
    assert_eq!(books_mod.typedefs.len(), 3);

    let product_info = books_mod.struct_decl("ProductInfo").unwrap();
    assert_eq!(product_info.kind, StructKind::Table);
    assert!(product_info.table_pipeline.is_some());
    assert!(product_info.rows_pipeline.is_some());
    assert!(product_info.match_pipeline.is_some());
    assert!(product_info.value_pipeline.is_some());
    assert!(product_info.pre_validate.is_some());

    let num_reviews = product_info
        .fields
        .iter()
        .find(|f| f.name == "number-of-reviews")
        .unwrap();
    assert_eq!(num_reviews.type_info, TypeInfo::int());
    assert_eq!(num_reviews.pipeline.fallback, Some(LiteralValue::Int(0)));
    assert!(matches!(
        num_reviews.pipeline.steps[0].op,
        StepOp::Match {
            cond: Cond::Atom(CondOp::Starts { .. })
        }
    ));

    let book = books_mod.struct_decl("Book").unwrap();
    assert_eq!(book.kind, StructKind::List);
    assert!(book.split_doc.is_some());
    let rating = book.fields.iter().find(|f| f.name == "rating").unwrap();
    assert_eq!(rating.type_info, TypeInfo::int());
    assert!(rating.pipeline.steps.iter().any(|s| matches!(
        &s.op,
        StepOp::ReplMap { replacements } if replacements.get("Five").map(String::as_str) == Some("5")
    )));

    let main_cat = books_mod.struct_decl("MainCatalogue").unwrap();
    let prev_page = main_cat
        .fields
        .iter()
        .find(|f| f.name == "prev-page")
        .unwrap();
    assert_eq!(prev_page.type_info, TypeInfo::string().with_optional(true));
    assert_eq!(prev_page.pipeline.fallback, Some(LiteralValue::Null));
    let books_field = main_cat.fields.iter().find(|f| f.name == "books").unwrap();
    assert_eq!(books_field.type_info, TypeInfo::nested("Book", true));

    // 2. quotes_toscrape.kdl
    let quotes_mod = compile_source(quotes_kdl, Some("quotes_toscrape.kdl"))
        .expect("compile quotes_toscrape.kdl");
    assert_eq!(quotes_mod.json_defs.len(), 2);
    assert_eq!(quotes_mod.structs.len(), 3);
    let quotes_main = quotes_mod.struct_decl("QuotesByJs").unwrap();
    assert_eq!(quotes_main.init_fields.len(), 1);
    assert_eq!(quotes_main.init_fields[0].name, "raw-json");
    assert_eq!(quotes_main.init_fields[0].type_info, TypeInfo::string());
    let data_field = quotes_main
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert!(matches!(
        data_field.pipeline.steps[0].op,
        StepOp::LoadInit { ref name } if name == "raw-json"
    ));
    assert_eq!(data_field.type_info, TypeInfo::json("Quote", true));

    // 3. imdb_nextjs.kdl
    let imdb_mod =
        compile_source(imdb_kdl, Some("imdb_nextjs.kdl")).expect("compile imdb_nextjs.kdl");
    assert_eq!(imdb_mod.json_defs.len(), 3);
    assert_eq!(imdb_mod.structs.len(), 3);
    let next_data = imdb_mod.struct_decl("ImdbNextData").unwrap();
    let title_results = next_data
        .fields
        .iter()
        .find(|f| f.name == "title-results")
        .unwrap();
    assert!(matches!(
        &title_results.pipeline.steps[1].op,
        StepOp::Jsonify {
            schema_name,
            path: Some(path),
            parsed_path: Some(_),
        } if schema_name == "TitleResults" && path == "props.pageProps.titleResults"
    ));

    // 4. Selector fallback blocks, raw inner/outer modes, and REST artifacts
    let rest_and_blocks = r#"
define MAIN-SEL=".article h1"
json User { id int; name str }
json ApiErr { code int; detail str }

struct Article {
    title {
        css {
            MAIN-SEL
            "h1"
        }
        raw inner
    }
    links {
        xpath-all {
            "//a[@href]"
            "//area[@href]"
        }
        attr "href"
        filter {
            not {
                contains "utm_"
            }
        }
    }
}

(rest)struct UsersClient {
    @request name="get-user" response="User" "curl https://api.example.com/users/{{id:int}}"
    @error:not-found 404 ApiErr detail="Not found"
    @error 500 ApiErr "code"
}
"#;
    let mod_rest =
        compile_source(rest_and_blocks, Some("rest_blocks.kdl")).expect("compile rest_blocks");
    let article = mod_rest.struct_decl("Article").unwrap();
    assert_eq!(
        article.fields[0].pipeline.steps[0].op,
        StepOp::Css {
            queries: vec![".article h1".to_string(), "h1".to_string()]
        }
    );
    assert_eq!(
        article.fields[0].pipeline.steps[1].op,
        StepOp::Raw {
            mode: RawMode::Inner
        }
    );
    assert!(matches!(
        &article.fields[1].pipeline.steps[2].op,
        StepOp::Filter { cond: Cond::Not(_) }
    ));

    // Verify synthesized REST artifacts (2 ErrorVariants, 1 ResultAlias, 1 MatcherList)
    assert_eq!(mod_rest.rest_artifacts.len(), 4);
    assert!(matches!(
        &mod_rest.rest_artifacts[0],
        RestArtifact::ErrorVariant(v) if v.name == "UsersClientErr404NotFound" && v.status == 404
    ));
    assert!(matches!(
        &mod_rest.rest_artifacts[1],
        RestArtifact::ErrorVariant(v) if v.name == "UsersClientErr500Code" && v.status == 500
    ));
    assert!(matches!(
        &mod_rest.rest_artifacts[2],
        RestArtifact::ResultAlias(a) if a.name == "GetUserResult" && a.response_schema.as_deref() == Some("User")
    ));
    assert!(matches!(
        &mod_rest.rest_artifacts[3],
        RestArtifact::MatcherList(m) if m.struct_name == "UsersClient" && m.entries.len() == 2
    ));
}

// ============================================================================
// 7. `insta` Snapshots: All Diagnostic Codes (`E000`–`E403`, `W011`–`W041`) & `ModuleInfo`
// ============================================================================

#[derive(Debug, Serialize)]
struct DiagSnapshot {
    case: &'static str,
    code: DiagnosticCode,
    severity: String,
    line: usize,
    col: usize,
    message: String,
    hint: Option<String>,
}

#[test]
fn snapshot_all_diagnostic_codes_e000_to_e403_and_w011_to_w041() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "missing_import.kdl",
            "import \"./does_not_exist.kdl\" { (struct)Missing }\n",
        )
        .with_file(
            "all_codes.kdl",
            r#"
// E000 tested separately because syntax error halts Pass 1
"#,
        );

    let mut snapshots: Vec<DiagSnapshot> = Vec::new();

    let push_first = |snapshots: &mut Vec<DiagSnapshot>,
                      case: &'static str,
                      res: parsmith::CheckResult,
                      expected_code: DiagnosticCode| {
        let diag = res
            .diagnostics
            .into_iter()
            .find(|d| d.code == expected_code)
            .unwrap_or_else(|| panic!("missing {expected_code} for case '{case}'"));
        snapshots.push(DiagSnapshot {
            case,
            code: diag.code,
            severity: diag.severity.to_string(),
            line: diag.line,
            col: diag.col,
            message: diag.message,
            hint: diag.hint,
        });
    };

    // E000: KDL 2.0 syntax error
    push_first(
        &mut snapshots,
        "E000_syntax_error",
        check_source("struct Page {\n    active true\n}\n", Some("e000.kdl")),
        DiagnosticCode::E000,
    );

    // E001: Invalid argument count / inline schema collision
    push_first(
        &mut snapshots,
        "E001_inline_json_hoisting_collision",
        check_source(
            "json RootDataValue { id int }\njson Root {\n    data (dict) {\n        @key str\n        @value { link str }\n    }\n}\n",
            Some("e001.kdl"),
        ),
        DiagnosticCode::E001,
    );

    // E002: Invalid CSS selector / regex / HTTP status
    push_first(
        &mut snapshots,
        "E002_invalid_css_selector",
        check_source(
            "struct S {\n    f { css \"div:not()\"; text }\n}\n",
            Some("e002.kdl"),
        ),
        DiagnosticCode::E002,
    );

    // E003: Missing imported file
    push_first(
        &mut snapshots,
        "E003_missing_import",
        check_with_loader("missing_import.kdl", &vfs),
        DiagnosticCode::E003,
    );

    // E040: Malformed GJSON dot-path
    push_first(
        &mut snapshots,
        "E040_malformed_gjson_path",
        check_source(
            "json Item {\n    val str from=\"items.#[id==1]\"\n}\n",
            Some("e040.kdl"),
        ),
        DiagnosticCode::E040,
    );

    // E041: Duplicate JSON source key
    push_first(
        &mut snapshots,
        "E041_duplicate_json_key",
        check_source(
            "json Item {\n    first str from=\"wire\"\n    second str from=\"wire\"\n}\n",
            Some("e041.kdl"),
        ),
        DiagnosticCode::E041,
    );

    // E100: Pipeline type mismatch
    push_first(
        &mut snapshots,
        "E100_pipeline_type_mismatch",
        check_source(
            "struct S {\n    f { css \"h1\"; trim }\n}\n",
            Some("e100.kdl"),
        ),
        DiagnosticCode::E100,
    );

    // E200: Unknown operation with typo suggestion
    push_first(
        &mut snapshots,
        "E200_unknown_operation",
        check_source(
            "struct S {\n    f { css \"h1\"; normlize-space }\n}\n",
            Some("e200.kdl"),
        ),
        DiagnosticCode::E200,
    );

    // E203: Disallowed directive / DOM op in raw struct
    push_first(
        &mut snapshots,
        "E203_disallowed_dom_in_raw",
        check_source(
            "(raw)struct S {\n    f { css \"h1\"; text }\n}\n",
            Some("e203.kdl"),
        ),
        DiagnosticCode::E203,
    );

    // E300: Undefined symbol reference
    push_first(
        &mut snapshots,
        "E300_undefined_symbol",
        check_source(
            "struct S {\n    f { nested NonExistent }\n}\n",
            Some("e300.kdl"),
        ),
        DiagnosticCode::E300,
    );

    // E301: Undefined @init field reference
    push_first(
        &mut snapshots,
        "E301_undefined_init_field",
        check_source(
            "struct S {\n    f { @missing_seed; text }\n}\n",
            Some("e301.kdl"),
        ),
        DiagnosticCode::E301,
    );

    // E302: Intra-file top-down declaration order violation
    push_first(
        &mut snapshots,
        "E302_topdown_order",
        check_source(
            "struct Main {\n    c { nested Child }\n}\nstruct Child {\n    t { css \"h1\"; text }\n}\n",
            Some("e302.kdl"),
        ),
        DiagnosticCode::E302,
    );

    // E400: Invalid type declaration / @error collision
    push_first(
        &mut snapshots,
        "E400_invalid_type_decl",
        check_source(
            "(bogus)struct S {\n    f { css \"a\"; text }\n}\n",
            Some("e400.kdl"),
        ),
        DiagnosticCode::E400,
    );

    // E401: Missing required struct directives
    push_first(
        &mut snapshots,
        "E401_missing_required_directive",
        check_source(
            "(list)struct S {\n    f { css \"a\"; text }\n}\n",
            Some("e401.kdl"),
        ),
        DiagnosticCode::E401,
    );

    // E402: Case-folded 4-ecosystem symbol collision
    push_first(
        &mut snapshots,
        "E402_symbol_collision",
        check_source(
            "struct S {\n    foo-bar { css \"a\"; text }\n    foo_bar { css \"b\"; text }\n}\n",
            Some("e402.kdl"),
        ),
        DiagnosticCode::E402,
    );

    // E403: Reserved target keyword identifier
    push_first(
        &mut snapshots,
        "E403_reserved_keyword",
        check_source("(raw)fn class { trim }\n", Some("e403.kdl")),
        DiagnosticCode::E403,
    );

    // W011: Deprecated positional JSON alias
    push_first(
        &mut snapshots,
        "W011_deprecated_json_alias",
        check_source("json User {\n    user_id int \"id\"\n}\n", Some("w011.kdl")),
        DiagnosticCode::W011,
    );

    // W040: Leading/trailing whitespace in `from` path
    push_first(
        &mut snapshots,
        "W040_whitespace_in_path",
        check_source(
            "json User {\n    user_id int from=\"  a.b  \"\n}\n",
            Some("w040.kdl"),
        ),
        DiagnosticCode::W040,
    );

    // W041: `path=` synonym used on JSON field instead of `from=`
    push_first(
        &mut snapshots,
        "W041_path_synonym_in_json",
        check_source(
            "json User {\n    user_id int path=\"id\"\n}\n",
            Some("w041.kdl"),
        ),
        DiagnosticCode::W041,
    );

    insta::assert_yaml_snapshot!("all_diagnostic_codes", snapshots);
}

#[test]
fn snapshot_inspect_schema_module_info_across_html_raw_json_and_rest() {
    let vfs = MemorySourceLoader::new()
        .with_file(
            "shared.kdl",
            r#"
extension StrExt {
    slugify {
        sig str str
        py { emit "{{out}} = {{in}}.lower().replace(' ', '-')" }
        js { emit "const {{out}} = {{in}}.toLowerCase().replaceAll(' ', '-');" }
    }
    truthy {
        sig T bool
        py { emit "{{out}} = bool({{in}})" }
        js { emit "const {{out}} = Boolean({{in}});" }
    }
}

json Author {
    id int
    display_name str from="profile.display_name"
}
"#,
        )
        .with_file(
            "catalog.kdl",
            r##"
@doc """
Multi-schema catalog module covering HTML, Raw, JSON hoisting, and REST.
"""

import "./shared.kdl" {
    (extension)StrExt
    (json)Author
}

(dict)json Translations {
    @key str
    @value {
        stream_url str from="url"
        author Author?
        internal_token str @skip
        backup_url str? @omitempty
    }
}

json ApiError {
    code int
    detail str
}

(list)struct BookCard {
    @split-doc { css-all "article.book" }
    title { css "h3 > a"; attr "title"; trim }
    slug { css "h3 > a"; text; !StrExt.slugify }
    price { css ".price"; text; re #"(\d+\.\d+)"#; to-float; fallback 0.0 }
}

struct CatalogPage {
    @doc "Main catalog page parser"
    @init {
        raw_state { css "script#state"; text }
    }
    @pre-validate {
        assert "missing catalog container" { css "main.catalog" }
    }
    @check has_books {
        css "article.book"
        !StrExt.truthy
    }
    @request name="fetch-page" """
    curl 'https://books.example.com/page/{{page:int}}?tag={{tags:str[]|csv}}&q={{query:str?}}'
    """
    heading { css "h1"; text; trim }
    books { nested BookCard }
    translations { @raw_state; jsonify Translations path="data.translations" }
}

(raw)struct LogParser {
    first_ip { re #"(\d+\.\d+\.\d+\.\d+)"#; fallback #null }
}

(rest)struct CatalogApi {
    @doc "Typed REST client for Catalog API"
    @request name="get-translations" response="Translations" response-path="data" """
    POST /v1/translations/{{anime_id:int}}?lang={{lang:str?}} HTTP/1.1
    Host: api.example.com
    Content-Type: application/json

    {"include_inactive": {{include_inactive:bool}}}
    """
    @error:not-found 404 ApiError detail="Not found"
    @error 500 ApiError "code"
}

fn extract_heading {
    @doc "Extracts page heading"
    css "h1"
    text
    trim
}

(raw)fn normalize_slug {
    trim
    !StrExt.slugify
}
"##,
        );

    let info = inspect_schema_with_loader("catalog.kdl", &vfs).expect("inspect catalog.kdl");

    // Verify round-trip JSON serialization of ModuleInfo
    let json_text = serde_json::to_string_pretty(&info).expect("serialize ModuleInfo to JSON");
    let roundtrip: parsmith::ModuleInfo =
        serde_json::from_str(&json_text).expect("deserialize ModuleInfo from JSON");
    assert_eq!(roundtrip, info);

    // Verify inspection error path returns CheckResult on invalid schema
    let err_res =
        inspect_schema_source("struct Broken { f { css \"h1\"; trim } }", Some("bad.kdl"));
    assert!(err_res.is_err());
    assert!(
        err_res
            .unwrap_err()
            .errors()
            .any(|d| d.code == DiagnosticCode::E100)
    );

    // Verify check_with_options and compile_with_loader
    let check_ok = check_with_options("catalog.kdl", &vfs, &CheckOptions::default());
    assert!(check_ok.is_ok());
    assert!(check_ok.module.is_some());
    let compiled = compile_with_loader("catalog.kdl", &vfs).expect("compile catalog.kdl");
    assert_eq!(compiled.structs.len(), 4);

    insta::assert_yaml_snapshot!("catalog_module_info", info);
}
