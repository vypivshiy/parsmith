use parsmith::{
    DiagnosticCode, GjsonPath, GjsonSegmentKind, GjsonStep, JsonTemplateValue, MemorySourceLoader,
    PlaceholderStyle, PlaceholderType, REQUEST_LINE_CONTINUATION_HINT, RequestBodyKind,
    check_source, check_with_loader, classify_gjson_path, is_malformed_gjson_path, parse_to_http,
};

fn error_codes(src: &str) -> Vec<DiagnosticCode> {
    check_source(src, Some("test.kdl"))
        .errors()
        .map(|d| d.code)
        .collect()
}

fn warning_codes(src: &str) -> Vec<DiagnosticCode> {
    check_source(src, Some("test.kdl"))
        .warnings()
        .map(|d| d.code)
        .collect()
}

// ============================================================================
// 1. E200 (`UNKNOWN_NODE`) & `strsim` Typo Suggestions (`Did you mean '...'?`)
// ============================================================================

#[test]
fn e200_unknown_top_level_declarations_with_typo_suggestions() {
    let cases = [
        ("strukt Page { f { css \"a\"; text } }", "struct"),
        ("jsn Data { id int }", "json"),
        ("defin X=\"1\"", "define"),
        (
            "extenson Utils { op { sig str str; py { emit \"{{out}} = {{in}}\" } } }",
            "extension",
        ),
        ("imprt \"./a.kdl\" { (struct)A }", "import"),
    ];

    for (src, expected_suggestion) in cases {
        let res = check_source(src, Some("top.kdl"));
        assert!(res.has_errors(), "expected error for: {src}");
        let e200 = res
            .errors()
            .find(|d| d.code == DiagnosticCode::E200)
            .expect("expected E200");
        assert!(
            e200.message.contains("Unknown node:"),
            "unexpected message: {}",
            e200.message
        );
        let hint = e200.hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("Did you mean") && hint.contains(expected_suggestion),
            "expected hint to suggest '{expected_suggestion}', got: '{hint}'"
        );
    }
}

#[test]
fn e200_unknown_struct_directives_pipeline_ops_and_predicates_with_suggestions() {
    // 1. Unknown struct directive
    let res_dir = check_source(
        "(list)struct S {\n    @splitt-doc { css-all \"li\" }\n    f { text }\n}\n",
        Some("dir.kdl"),
    );
    let d_dir = res_dir
        .errors()
        .find(|d| d.code == DiagnosticCode::E200)
        .expect("expected E200 for @splitt-doc");
    assert!(d_dir.hint.as_deref().unwrap_or("").contains("'@split-doc'"));

    // 2. Unknown pipeline operation
    let res_op = check_source(
        "struct S {\n    f {\n        css \"h1\"\n        normlize-space\n    }\n}\n",
        Some("op.kdl"),
    );
    let d_op = res_op
        .errors()
        .find(|d| d.code == DiagnosticCode::E200)
        .expect("expected E200 for normlize-space");
    assert!(d_op.message.contains("unknown operation 'normlize-space'"));
    assert!(
        d_op.hint
            .as_deref()
            .unwrap_or("")
            .contains("'normalize-space'"),
        "unexpected hint: {:?}",
        d_op.hint
    );

    // 3. Unknown predicate operation inside filter / assert
    let res_pred = check_source(
        "struct S {\n    f {\n        css-all \"a\"\n        filter {\n            has-atrr \"href\"\n        }\n    }\n}\n",
        Some("pred.kdl"),
    );
    let d_pred = res_pred
        .errors()
        .find(|d| d.code == DiagnosticCode::E200)
        .expect("expected E200 for has-atrr");
    assert!(d_pred.message.contains("unknown operation 'has-atrr'"));
    assert!(d_pred.hint.as_deref().unwrap_or("").contains("'has-attr'"));
}

#[test]
fn e200_unknown_extension_entries() {
    let src = r#"
extension Utils {
    op {
        sig str str
        unknown-target {
            emit "{{out}} = {{in}}"
        }
        py {
            unknown-entry "foo"
            helper my_helper {
                unknown-helper-entry "bar"
                source "def my_helper(x): return x"
            }
            emit "{{out}} = {{in}}"
        }
    }
}
"#;
    let res = check_source(src, Some("ext.kdl"));
    let e200s: Vec<_> = res
        .errors()
        .filter(|d| d.code == DiagnosticCode::E200)
        .collect();
    assert_eq!(
        e200s.len(),
        3,
        "expected 3 E200 diagnostics, got: {e200s:?}"
    );
}

// ============================================================================
// 2. E400, E401, E402 (Struct Types, Required Directives, Duplicate Names)
// ============================================================================

#[test]
fn e400_and_e401_struct_types_and_required_directives() {
    // Unknown struct type annotation -> E400
    assert!(
        error_codes("(bogus)struct S { f { css \"a\"; text } }").contains(&DiagnosticCode::E400)
    );
    assert!(
        error_codes("struct S type=\"bogus\" { f { css \"a\"; text } }")
            .contains(&DiagnosticCode::E400)
    );

    // Unknown fn annotation -> E400
    assert!(error_codes("(item)fn f { css \"a\"; text }").contains(&DiagnosticCode::E400));

    // Unknown json annotation -> E400
    assert!(error_codes("(list)json J { id int }").contains(&DiagnosticCode::E400));

    // Missing @split-doc on (list)struct -> E401
    let res_list = check_source("(list)struct S { f { css \"a\"; text } }", Some("t.kdl"));
    assert!(
        res_list
            .errors()
            .any(|d| d.code == DiagnosticCode::E401 && d.message.contains("@split-doc"))
    );

    // Missing @split-doc, @key, @value on (dict)struct -> E401
    let res_dict = check_source("(dict)struct S {}", Some("t.kdl"));
    assert!(res_dict.errors().any(|d| {
        d.code == DiagnosticCode::E401
            && d.message.contains("@split-doc")
            && d.message.contains("@key")
            && d.message.contains("@value")
    }));

    // Missing @table, @rows, @match, @value on (table)struct -> E401
    let res_table = check_source(
        "(table)struct S { f { match { eq \"A\" } } }",
        Some("t.kdl"),
    );
    assert!(res_table.errors().any(|d| {
        d.code == DiagnosticCode::E401
            && d.message.contains("@table")
            && d.message.contains("@rows")
            && d.message.contains("@match")
            && d.message.contains("@value")
    }));

    // Missing @request on (rest)struct -> E401
    let res_rest = check_source("(rest)struct S {}", Some("t.kdl"));
    assert!(
        res_rest
            .errors()
            .any(|d| d.code == DiagnosticCode::E401 && d.message.contains("@request"))
    );

    // Missing fields on item / list / table / flat / raw -> E401
    assert!(error_codes("struct Empty {}").contains(&DiagnosticCode::E401));
    assert!(
        error_codes("(list)struct EmptyList { @split-doc { css-all \"li\" } }")
            .contains(&DiagnosticCode::E401)
    );
    assert!(error_codes("(flat)struct EmptyFlat {}").contains(&DiagnosticCode::E401));
    assert!(error_codes("(raw)struct EmptyRaw {}").contains(&DiagnosticCode::E401));
}

#[test]
fn e402_duplicate_declarations_and_fields() {
    // Duplicate struct
    assert!(
        error_codes("struct Same { a { css \"a\"; text } }\nstruct Same { b { css \"b\"; text } }")
            .contains(&DiagnosticCode::E402)
    );

    // Duplicate struct field
    assert!(
        error_codes("struct S { f { css \"a\"; text }; f { css \"b\"; text } }")
            .contains(&DiagnosticCode::E402)
    );

    // Duplicate @init field
    assert!(
        error_codes("struct S { @init { x { css \"a\" }; x { css \"b\" } }; f { @x; text } }")
            .contains(&DiagnosticCode::E402)
    );

    // Duplicate fn
    assert!(
        error_codes("fn f { css \"a\"; text }\nfn f { css \"b\"; text }")
            .contains(&DiagnosticCode::E402)
    );

    // Duplicate extension namespace / op / target
    let dup_ext = r#"
extension Utils {
    op {
        sig str str
        py { emit "{{out}} = {{in}}" }
        py { emit "{{out}} = {{in}}" }
    }
    op {
        sig str str
        js { emit "const {{out}} = {{in}};" }
    }
}
extension Utils {
    other {
        sig str str
        py { emit "{{out}} = {{in}}" }
    }
}
"#;
    let res_ext = check_source(dup_ext, Some("ext.kdl"));
    let e402_count = res_ext
        .errors()
        .filter(|d| d.code == DiagnosticCode::E402)
        .count();
    assert_eq!(e402_count, 3);
}

#[test]
fn e400_and_e100_extension_signature_validation() {
    let bad_ext = r#"
extension Bad {
    op1 {
        sig unknown_type (tuple)str
        py { emit "{{out}} = {{in}}" }
    }
    op2 {
        sig T? (array)T
        py { emit "{{out}} = {{in}}" }
    }
    op3 {
        sig str T
        py { emit "{{out}} = {{in}}" }
    }
}
"#;
    let res = check_source(bad_ext, Some("bad_ext.kdl"));
    assert!(res.errors().any(|d| d.code == DiagnosticCode::E400
        && d.message.contains("unknown extension signature type")));
    assert!(res.errors().any(|d| d.code == DiagnosticCode::E400
        && d.message.contains("unsupported extension type annotation")));
    assert!(res.errors().any(|d| d.code == DiagnosticCode::E400
        && d.message.contains("generic type T cannot have modifiers")));
    assert!(res.errors().any(|d| {
        d.code == DiagnosticCode::E100
            && d.message
                .contains("generic output T must be bound by generic input T")
    }));
}

// ============================================================================
// 3. E203 (`DISALLOWED_DIRECTIVE` / Context Violations)
// ============================================================================

#[test]
fn e203_disallowed_directives_per_struct_and_fn_context() {
    // 1. @split-doc on item struct
    assert!(
        error_codes("struct S { @split-doc { css-all \"li\" }; f { text } }")
            .contains(&DiagnosticCode::E203)
    );

    // 2. @key and @value on item struct
    assert!(error_codes("struct S { @key { text }; f { text } }").contains(&DiagnosticCode::E203));

    // 3. @table / @rows / @match on list struct
    assert!(
        error_codes(
            "(list)struct S { @split-doc { css-all \"li\" }; @table { css \"table\" }; f { text } }"
        )
        .contains(&DiagnosticCode::E203)
    );

    // 4. @error on non-rest struct
    assert!(
        error_codes("json Err { code int }\nstruct S { @error 404 Err; f { css \"a\"; text } }")
            .contains(&DiagnosticCode::E203)
    );

    // 5. @init on rest struct
    assert!(
        error_codes(
            "(rest)struct S { @init { x { trim } }; @request \"curl https://example.com\" }"
        )
        .contains(&DiagnosticCode::E203)
    );

    // 6. Regular field inside (rest)struct and (dict)struct
    assert!(
        error_codes("(rest)struct S { @request \"curl https://example.com\"; f { text } }")
            .contains(&DiagnosticCode::E203)
    );
    assert!(
        error_codes(
            "(dict)struct S { @split-doc { css-all \"li\" }; @key { text }; @value { text }; extra { text } }"
        )
        .contains(&DiagnosticCode::E203)
    );

    // 7. @init, @check, @split-doc, @request inside fn
    for dir in [
        "@init { x { css \"a\" } }",
        "@check ok { css \"a\"; to-bool }",
        "@split-doc { css-all \"li\" }",
        "@request \"curl https://example.com\"",
    ] {
        let src = format!("fn my_fn {{\n    {dir}\n    css \"h1\"\n    text\n}}\n");
        assert!(
            error_codes(&src).contains(&DiagnosticCode::E203),
            "expected E203 for '{dir}' inside fn"
        );
    }

    // 8. response-join on (rest)struct -> E203
    let rest_join = r#"
json Log { line str }
(rest)struct API {
    @request response="Log" response-path="lines" response-join="\n" """
    GET /log HTTP/1.1
    Host: api.example.com
    """
}
"#;
    let res_join = check_source(rest_join, Some("rest_join.kdl"));
    assert!(res_join.errors().any(|d| {
        d.code == DiagnosticCode::E203
            && d.message.contains("response-join")
            && d.message.contains("forbidden")
    }));

    // 9. Table field not starting with match -> E203
    let bad_table = r#"
(table)struct Specs {
    @table { css "table" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }
    bad-field { css "td"; text }
}
"#;
    assert!(error_codes(bad_table).contains(&DiagnosticCode::E203));
}

#[test]
fn e203_dom_operations_forbidden_in_raw_struct_and_raw_fn() {
    for op in [
        "css \".x\"",
        "css-all \".x\"",
        "css-remove \".ad\"",
        "xpath \"//div\"",
        "xpath-all \"//div\"",
        "xpath-remove \"//div\"",
        "text",
        "raw",
        "attr \"href\"",
    ] {
        let raw_struct_src = format!("(raw)struct S {{\n    f {{ {op} }}\n}}\n");
        let res_s = check_source(&raw_struct_src, Some("raw_s.kdl"));
        assert!(
            res_s
                .errors()
                .any(|d| d.code == DiagnosticCode::E203 && d.message.contains("forbidden")),
            "expected E203 for '{op}' in (raw)struct, got: {:?}",
            res_s.diagnostics
        );

        let raw_fn_src = format!("(raw)fn f {{\n    {op}\n}}\n");
        let res_f = check_source(&raw_fn_src, Some("raw_f.kdl"));
        assert!(
            res_f
                .errors()
                .any(|d| d.code == DiagnosticCode::E203 && d.message.contains("forbidden")),
            "expected E203 for '{op}' in (raw)fn, got: {:?}",
            res_f.diagnostics
        );
    }

    // String & regex ops in (raw)struct and (raw)fn are valid
    let valid_raw = r##"
(raw)struct RawDoc {
    @init { lines { split "\n" } }
    @split-doc { split "\n" }
    first_word { re #"(\w+)"#; lower; trim }
}
(raw)fn parse_token { split ","; first; trim }
"##;
    let res_ok = check_source(valid_raw, Some("valid_raw.kdl"));
    assert!(
        !res_ok.has_errors(),
        "unexpected errors: {:?}",
        res_ok.diagnostics
    );
}

#[test]
fn e203_predicates_outside_containers_and_assert_only_predicates() {
    // 1. Predicates directly in a pipeline (outside filter/assert/match) -> E203
    for pred in [
        "eq \"a\"",
        "ne \"a\"",
        "contains \"a\"",
        "starts \"a\"",
        "ends \"a\"",
        "has-attr \"href\"",
        "attr-eq \"href\" \"x\"",
        "text-contains \"foo\"",
        "len-gt 0",
        "re-any #\"\\d+\"#",
        "not { eq \"a\" }",
        "and { eq \"a\"; ne \"b\" }",
        "or { eq \"a\"; eq \"b\" }",
    ] {
        let src = format!(
            "struct S {{\n    f {{\n        css \"a\"\n        text\n        {pred}\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("pred_outside.kdl"));
        assert!(
            res.errors().any(|d| d.code == DiagnosticCode::E203),
            "expected E203 for '{pred}' outside predicate block, got: {:?}",
            res.diagnostics
        );
    }

    // 2. Assert-only predicates (re-any, re-all, len-*, gt, lt, ge, le) inside `filter` -> E203
    for assert_only in [
        "re-any #\"\\d+\"#",
        "re-all #\"\\d+\"#",
        "len-eq 2",
        "len-gt 0",
        "len-range 1 5",
        "gt 5",
    ] {
        let src = format!(
            "struct S {{\n    f {{\n        css-all \"a\"\n        text\n        filter {{\n            {assert_only}\n        }}\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("filter_assert_only.kdl"));
        assert!(
            res.errors().any(|d| {
                d.code == DiagnosticCode::E203
                    && d.message.contains("only valid inside an assert block")
            }),
            "expected E203 for '{assert_only}' inside filter, got: {:?}",
            res.diagnostics
        );
    }

    // 3. DOM predicates inside table `match { ... }` -> E203
    let bad_match = r#"
(table)struct T {
    @table { css "table" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }
    col { match { has-attr "href" } }
}
"#;
    assert!(error_codes(bad_match).contains(&DiagnosticCode::E203));
}

// ============================================================================
// 4. E001 & E002 (Arguments, CSS3/4, XPath 1.0, and Regex Validators)
// ============================================================================

#[test]
fn e002_pure_rust_css3_css4_selector_validator() {
    // Valid formal CSS3/4 selectors (including engine-specific pseudo-classes & pseudo-elements)
    let valid_selectors = [
        "div",
        "article.product_pod > h3 > a[href][title]",
        "a[href^=\"https://\"][data-kind*=\"nav\" i]",
        "ul > li:first-child, ul > li:last-child, ul > li:only-child",
        "tr:nth-child(2n+1), tr:nth-last-child(odd), tr:nth-of-type(even)",
        "li:nth-child(2n+1 of .active)",
        "div:not(.ad, .promo):is(.card, .item):where([data-id]):matches(.box)",
        "div.card:has(> img.poster, + .caption)",
        "p::first-line, span::before, input::placeholder, x-card::custom-element",
        "x-tabs::part(tab-button), slot::slotted(.item)",
        "svg|path",
        "div:custom-pseudo-class",
        "div:-soup-contains('hello')",
        "div:-soup-contains-own('hello')",
        "div:contains('hello')",
        "div:has-text('hello')",
        "div:eq(0)",
    ];

    for sel in valid_selectors {
        let escaped = sel.replace('"', "\\\"");
        let src = format!("struct S {{\n    f {{ css \"{escaped}\"; text }}\n}}\n");
        let res = check_source(&src, Some("css_valid.kdl"));
        assert!(
            !res.has_errors(),
            "expected valid CSS selector '{sel}', got: {:?}",
            res.diagnostics
        );
    }

    // Invalid formal CSS selectors (unbalanced delimiters, dangling combinators, malformed structural or empty functional pseudo-classes) -> E002
    let invalid_selectors = [
        "",
        "   ",
        "div >",
        "> div",
        "div > > span",
        "div,",
        ",div",
        "#",
        ".",
        "div..foo",
        "[href",
        "[]",
        "[href==val]",
        "[href=]",
        "div:",
        "div::",
        "div:not()",
        "div:is()",
        "div:where()",
        "div:has()",
        "div:not(div >)",
        "div:is(.a, )",
        "div:has(> )",
        "li:nth-child(invalid)",
        "li:nth-of-type(2n+1 of .active)",
        "div:contains()",
        "div:unknown-func()",
        "div::part()",
    ];

    for bad_sel in invalid_selectors {
        let escaped = bad_sel.replace('"', "\\\"");
        let src = format!("struct S {{\n    f {{ css \"{escaped}\"; text }}\n}}\n");
        let res = check_source(&src, Some("css_invalid.kdl"));
        assert!(
            res.errors()
                .any(|d| d.code == DiagnosticCode::E002
                    && d.message.contains("invalid CSS selector")),
            "expected E002 for invalid CSS selector '{bad_sel}', got: {:?}",
            res.diagnostics
        );
    }
}

#[test]
fn e002_pure_rust_xpath1_validator() {
    // Valid XPath 1.0 expressions
    let valid_xpaths = [
        "//div",
        "/html/body/div[1]/a/@href",
        "//a[@href and contains(@class, 'link')]/text()",
        "//table//tr[position() > 1 and position() <= last() - 1]/td[2]",
        "descendant-or-self::node()/child::p[normalize-space(.) != '']",
        "//span[starts-with(@id, 'item-') or @data-idx = 0]/parent::div/following-sibling::div[1]",
        "//a[@href] | //area[@href]",
        "count(//li) + 1",
    ];

    for xp in valid_xpaths {
        let escaped = xp.replace('"', "\\\"");
        let src = format!("struct S {{\n    f {{ xpath \"{escaped}\"; text }}\n}}\n");
        let res = check_source(&src, Some("xpath_valid.kdl"));
        assert!(
            !res.has_errors(),
            "expected valid XPath '{xp}', got: {:?}",
            res.diagnostics
        );
    }

    // Invalid XPath 1.0 expressions -> E002
    let invalid_xpaths = [
        "",
        "///div",
        "//div[",
        "//div[]",
        "//div[@id='unclosed]",
        "//div/unknown-axis::span",
        "//div/unknown-func-in-step()",
        "//div/",
        "//div trailing",
        "//div[@a ! 'b']",
    ];

    for bad_xp in invalid_xpaths {
        let escaped = bad_xp.replace('"', "\\\"");
        let src = format!("struct S {{\n    f {{ xpath \"{escaped}\"; text }}\n}}\n");
        let res = check_source(&src, Some("xpath_invalid.kdl"));
        assert!(
            res.errors().any(|d| d.code == DiagnosticCode::E002
                && d.message.contains("invalid XPath expression")),
            "expected E002 for invalid XPath '{bad_xp}', got: {:?}",
            res.diagnostics
        );
    }
}

#[test]
fn e001_and_e002_pure_rust_regex_validator_lookarounds_flags_and_capture_groups() {
    // Valid portable regexes with inline flags (?imsx), non-capturing groups, and 1 capture group
    let valid_regexes = [
        r#"(\d+)"#,
        r#"(?i)price:\s*(\d+\.\d{2})"#,
        r#"(?:id=)(\w+)(?:&|$)"#,
        r#"(?ims)^header:.*value=(\d+)"#,
        r#"(?x)
            # Match version number
            v (\d+\.\d+)
        "#,
    ];

    for pat in valid_regexes {
        let src = if pat.contains('\n') {
            format!(
                "struct S {{\n    f {{\n        css \"a\"\n        text\n        re #\"\"\"\n{pat}\n\"\"\"#\n    }}\n}}\n"
            )
        } else {
            format!(
                "struct S {{\n    f {{\n        css \"a\"\n        text\n        re #\"{pat}\"#\n    }}\n}}\n"
            )
        };
        let res = check_source(&src, Some("re_valid.kdl"));
        assert!(
            !res.has_errors(),
            "expected valid regex '{pat}', got: {:?}",
            res.diagnostics
        );
    }

    // Forbidden portable regex constructs: lookarounds and named capture groups / backreferences -> E002
    let forbidden_portable_regexes = [
        (r#"(\w+)(?=&|$)"#, "lookaround"),
        (r#"([A-Za-z]+)(?!_skip)"#, "lookaround"),
        (r#"(?<=id=)(\w+)"#, "lookaround"),
        (r#"(?<!ignore )([A-Za-z]+)"#, "lookaround"),
        (r#"(?P<id>\d+)"#, "named capture group"),
        (r#"(?<id>\d+)"#, "named capture group"),
        (r#"(\w+)-(?P=id)"#, "named backreference"),
    ];

    for (forbidden_pat, expected_msg) in forbidden_portable_regexes {
        let src = format!(
            "struct S {{\n    f {{\n        css \"a\"\n        text\n        re #\"{forbidden_pat}\"#\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("re_forbidden.kdl"));
        assert!(
            res.errors().any(|d| d.code == DiagnosticCode::E002
                && d.message.contains("invalid regex pattern")
                && d.message.contains(expected_msg)),
            "expected E002 containing '{expected_msg}' for forbidden regex '{forbidden_pat}', got: {:?}",
            res.diagnostics
        );
    }

    // Malformed regex syntax -> E002
    for bad_pat in [r#"(\d+"#, r#"[a-z"#, r#"*abc"#] {
        let src = format!(
            "struct S {{\n    f {{\n        css \"a\"\n        text\n        re #\"{bad_pat}\"#\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("re_bad.kdl"));
        assert!(
            res.errors()
                .any(|d| d.code == DiagnosticCode::E002
                    && d.message.contains("invalid regex pattern")),
            "expected E002 for malformed regex '{bad_pat}', got: {:?}",
            res.diagnostics
        );
    }

    // Pipeline re / re-all with 0 capture groups or >1 capture groups -> E001
    let zero_groups = "struct S { f { css \"a\"; text; re #\"\\d+\"# } }";
    let res_zero = check_source(zero_groups, Some("zero.kdl"));
    assert!(res_zero.errors().any(|d| d.code == DiagnosticCode::E001
        && d.message.contains("must have exactly one capture group")));

    let two_groups = "struct S { f { css \"a\"; text; re-all #\"(a)(b)\"# } }";
    let res_two = check_source(two_groups, Some("two.kdl"));
    assert!(
        res_two
            .errors()
            .any(|d| d.code == DiagnosticCode::E001 && d.message.contains("got 2"))
    );

    // re-sub, re-split, and condition regexes (re, re-any, re-all, attr-re, text-re) allow 0, 1, or >1 capture groups
    let unrestricted_group_ops = r##"
struct S {
    sub_zero { css "a"; text; re-sub #"\d+"# "X" }
    sub_two  { css "a"; text; re-sub #"(\w+)-(\d+)"# "$1:$2" }
    split_zero { css "a"; text; re-split #"\s*,\s*"# }
    split_two  { css "a"; text; re-split #"(\s+)|(-)"# }
    cond_ops {
        css-all "a"
        filter {
            attr-re "href" #"^https://([a-z]+)\.example\.com/(\d+)"#
            text-re #"\d+"#
        }
        text
        filter {
            re #"^(foo)-(bar)-(baz)$"#
        }
        assert {
            re-any #"\d+"#
            re-all #"(\w+):(\d+)"#
        }
    }
}
"##;
    let res_unrestricted = check_source(unrestricted_group_ops, Some("unrestricted.kdl"));
    assert!(
        !res_unrestricted.has_errors(),
        "expected re-sub, re-split, and condition regexes to allow any capture group count, got: {:?}",
        res_unrestricted.diagnostics
    );
}

#[test]
fn e001_pipeline_and_predicate_argument_validation() {
    let cases = [
        "struct S { f { css \"a\"; text \"extra\" } }",
        "struct S { f { css \"a\"; raw \"invalid_mode\" } }",
        "struct S { f { css \"a\"; raw \"inner\" \"outer\" } }",
        "struct S { f { css \"a\"; attr } }",
        "struct S { f { css \"a\"; attr 123 } }",
        "struct S { f { css \"a\"; text; fmt \"missing_placeholder\" } }",
        "struct S { f { css \"a\"; text; repl \"only_one_arg\" } }",
        "struct S { f { css-all \"a\"; index \"not_an_int\" } }",
        "struct S { f { css-all \"a\"; slice 0 } }",
        "struct S { f { css \"a\"; text; assert \"msg1\" \"msg2\" { eq \"x\" } } }",
        "struct S { f { css \"a\"; text; assert #true { eq \"x\" } } }",
        "struct S { f { css \"a\"; text; assert { len-eq -1 } } }",
    ];

    for src in cases {
        let codes = error_codes(src);
        assert!(
            codes.contains(&DiagnosticCode::E001),
            "expected E001 for: {src}, got: {codes:?}"
        );
    }
}

// ============================================================================
// 5. `@request` & `@error` Validation (`E001`, `E002`, `E400`, `E402`)
// ============================================================================

#[test]
fn request_curl_and_raw_http_parsing_and_placeholder_extraction() {
    // 1. POSIX curl with --json, -u user:pass, and typed placeholders (both {{...}} and {...})
    let req_curl = parse_to_http(
        "curl -u admin:secret --json '{\"name\": \"{{name}}\", \"rate\": {rate:float}, \"active\": {{active:bool}}}' 'https://api.example.com/users/{id:int}?q={q:str?}&tags={tags:str[]:csv}'",
    )
    .expect("parse valid curl command");

    assert_eq!(req_curl.method, "POST");
    assert_eq!(req_curl.body_kind, RequestBodyKind::Json);
    assert_eq!(
        req_curl.headers.get("Authorization").unwrap().source(),
        "Basic YWRtaW46c2VjcmV0"
    );

    let phs = req_curl.placeholders();
    let ph_names: Vec<&str> = phs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(ph_names, vec!["id", "q", "tags", "name", "rate", "active"]);

    let id_ph = phs.iter().find(|p| p.name == "id").unwrap();
    assert_eq!(id_ph.type_name, PlaceholderType::Int);
    assert!(!id_ph.is_array && !id_ph.is_optional);

    let q_ph = phs.iter().find(|p| p.name == "q").unwrap();
    assert_eq!(q_ph.type_name, PlaceholderType::Str);
    assert!(q_ph.is_optional);

    let tags_ph = phs.iter().find(|p| p.name == "tags").unwrap();
    assert_eq!(tags_ph.type_name, PlaceholderType::Str);
    assert!(tags_ph.is_array);
    assert_eq!(tags_ph.style, Some(PlaceholderStyle::Csv));

    // Verify structured JsonTemplateValue preserved placeholders
    let Some(JsonTemplateValue::Object(ref obj)) = req_curl.json_template else {
        panic!("expected JsonTemplateValue::Object");
    };
    assert!(matches!(
        obj.get("name"),
        Some(JsonTemplateValue::Placeholder(_))
    ));
    assert!(matches!(
        obj.get("rate"),
        Some(JsonTemplateValue::Placeholder(_))
    ));
    assert!(matches!(
        obj.get("active"),
        Some(JsonTemplateValue::Placeholder(_))
    ));

    // 2. Raw HTTP/1.1 with Host, Cookie, and application/json body
    let req_http = parse_to_http(
        "POST /v1/items?tags={{tags:int[]|repeat}} HTTP/1.1\nHost: api.example.com\nContent-Type: application/json\nCookie: sid=xyz; theme={{theme}}\n\n{\"count\": {{count:int}}}",
    )
    .expect("parse valid raw HTTP request");
    assert_eq!(req_http.method, "POST");
    assert_eq!(req_http.url.source(), "https://api.example.com/v1/items");
    assert_eq!(req_http.cookies.get("sid").unwrap().source(), "xyz");
    assert_eq!(req_http.cookies.get("theme").unwrap().source(), "{{theme}}");
    assert_eq!(req_http.body_kind, RequestBodyKind::Json);
}

#[test]
fn request_validation_errors_and_backslash_comment_continuation_hint() {
    // 1. Malformed placeholder -> E002
    let bad_ph = r#"
struct S {
    @request "curl https://example.com/?q={{_bad}}&x={{foo:unknown}}"
    f { css "a"; text }
}
"#;
    assert!(error_codes(bad_ph).contains(&DiagnosticCode::E002));

    // 2. Uppercase placeholder in inline @request -> E002
    let upper_ph = r#"
struct S {
    @request "curl https://example.com/?q={{QUERY}}"
    f { css "a"; text }
}
"#;
    let res_upper = check_source(upper_ph, Some("upper.kdl"));
    assert!(
        res_upper
            .errors()
            .any(|d| d.code == DiagnosticCode::E002 && d.message.contains("must be lowercase"))
    );

    // 3. Array `[]` or optional `?` in URL path segment -> E002
    let path_arr = r#"
struct S {
    @request "curl https://example.com/items/{{id:int?}}"
    f { css "a"; text }
}
"#;
    assert!(error_codes(path_arr).contains(&DiagnosticCode::E002));

    // 4. Malformed JSON body when Content-Type is application/json -> E002
    let bad_json_body = r#"
struct S {
    @request """
    POST /items HTTP/1.1
    Host: api.example.com
    Content-Type: application/json

    {"name": "{{name}}", "broken": }
    """
    f { css "a"; text }
}
"#;
    let res_json = check_source(bad_json_body, Some("bad_json.kdl"));
    assert!(
        res_json
            .errors()
            .any(|d| d.code == DiagnosticCode::E002 && d.message.contains("Invalid JSON body"))
    );

    // 5. Conflicting placeholder specs in same @request -> E402
    let conflict_ph = r#"
struct S {
    @request "curl 'https://example.com/?a={{id:int}}&b={{id:str}}'"
    f { css "a"; text }
}
"#;
    assert!(error_codes(conflict_ph).contains(&DiagnosticCode::E402));

    // 6. Backslash + `//` comment inside @request string -> E002 with REQUEST_LINE_CONTINUATION_HINT
    let bs_comment_in_req = r##"
struct S {
    @request #"""
    curl https://example.com \
      // comment breaking line continuation
      -H "Accept: text/html"
    """#
    f { css "a"; text }
}
"##;
    let res_bs = check_source(bs_comment_in_req, Some("bs.kdl"));
    assert!(res_bs.errors().any(|d| {
        d.code == DiagnosticCode::E002 && d.hint.as_deref() == Some(REQUEST_LINE_CONTINUATION_HINT)
    }));

    // 7. Leaked HTTP signature as struct field -> E203/E001 with REQUEST_LINE_CONTINUATION_HINT
    let leaked_rest = r#"
(rest)struct S {
    @request "curl https://example.com"
    "-H 'Authorization: Bearer token'"
}
"#;
    let res_leaked = check_source(leaked_rest, Some("leaked.kdl"));
    assert!(res_leaked.errors().any(|d| {
        d.code == DiagnosticCode::E203 && d.hint.as_deref() == Some(REQUEST_LINE_CONTINUATION_HINT)
    }));
}

#[test]
fn error_directive_validation_status_range_args_and_variant_collisions() {
    // 1. Valid @error and @error:<suffix>
    let valid_err = r#"
json User { id int }
json Err { code int; detail str }
(rest)struct API {
    @request name="get-user" response="User" "curl https://api.example.com/users/{{id:int}}"
    @error:not-found 404 Err detail="Not found"
    @error 404 Err "detail"
    @error 400 Err "a.first" second="2" third="3"
}
"#;
    let res_ok = check_source(valid_err, Some("valid_err.kdl"));
    assert!(
        !res_ok.has_errors(),
        "unexpected errors: {:?}",
        res_ok.diagnostics
    );

    // 2. Invalid HTTP status range (`0`, `99`, `600`, `999`) -> E002
    for bad_status in ["0", "99", "600", "999", "\"not_an_int\""] {
        let src = format!(
            "json Err {{ code int }}\n(rest)struct API {{\n    @request \"curl https://example.com\"\n    @error {bad_status} Err\n}}\n"
        );
        assert!(
            error_codes(&src).contains(&DiagnosticCode::E002),
            "expected E002 for status {bad_status}"
        );
    }

    // 3. Invalid @error:<suffix> -> E002
    for bad_dir in ["@error:", "@error:123bad", "@error:bad.name"] {
        let src = format!(
            "json Err {{ code int }}\n(rest)struct API {{\n    @request \"curl https://example.com\"\n    {bad_dir} 404 Err\n}}\n"
        );
        assert!(
            error_codes(&src).contains(&DiagnosticCode::E002),
            "expected E002 for {bad_dir}"
        );
    }

    // 4. Missing arguments on @error -> E001
    let missing_args = "json Err { code int }\n(rest)struct API {\n    @request \"curl https://example.com\"\n    @error 404\n}\n";
    assert!(error_codes(missing_args).contains(&DiagnosticCode::E001));

    // 5. Duplicate key across positional and property args -> E400
    let dup_key = "json Err { code int }\n(rest)struct API {\n    @request \"curl https://example.com\"\n    @error 404 Err \"code\" code=\"123\"\n}\n";
    assert!(error_codes(dup_key).contains(&DiagnosticCode::E400));

    // 6. Colliding @error variant names (including 2-key auto-naming cap) -> E400
    let colliding_variants = r#"
json Err { code int }
(rest)struct API {
    @request "curl https://example.com"
    @error 400 Err a b c="1"
    @error 400 Err a b d="2"
}
"#;
    let res_col = check_source(colliding_variants, Some("col.kdl"));
    assert!(
        res_col
            .errors()
            .any(|d| { d.code == DiagnosticCode::E400 && d.message.contains("APIErr400AB") })
    );
}

// ============================================================================
// 6. GJSON Path Expression & `json` Schema Validation (`E040`, `E041`, `W011`, `W040`, `W041`)
// ============================================================================

#[test]
fn gjson_path_ast_segments_and_classification() {
    assert_eq!(classify_gjson_path("id"), (true, "id".to_string()));
    assert_eq!(
        classify_gjson_path("@context"),
        (true, "@context".to_string())
    );
    assert_eq!(
        classify_gjson_path(r"os\.version"),
        (true, "os.version".to_string())
    );
    assert_eq!(classify_gjson_path(r"a\\b"), (true, r"a\b".to_string()));
    assert_eq!(classify_gjson_path(r"\#"), (true, "#".to_string()));

    assert_eq!(classify_gjson_path("a.b"), (false, "a.b".to_string()));
    assert_eq!(classify_gjson_path("0"), (false, "0".to_string()));
    assert_eq!(classify_gjson_path("#"), (false, "#".to_string()));
    assert_eq!(
        classify_gjson_path("items.#.name"),
        (false, "items.#.name".to_string())
    );

    let parsed = GjsonPath::parse("user.badges.0.tags.#.label").expect("valid GJSON path");
    assert!(!parsed.is_direct_key());
    assert_eq!(parsed.segments.len(), 6);
    assert_eq!(parsed.segments[2].kind, GjsonSegmentKind::Index);
    assert_eq!(parsed.segments[2].index, Some(0));
    assert_eq!(parsed.segments[4].kind, GjsonSegmentKind::Hash);
    assert_eq!(
        parsed.steps,
        vec![
            GjsonStep::Key("user".to_string()),
            GjsonStep::Key("badges".to_string()),
            GjsonStep::Index(0),
            GjsonStep::Key("tags".to_string()),
            GjsonStep::ArrayProjection(vec![GjsonStep::Key("label".to_string())]),
        ]
    );

    let count_path = GjsonPath::parse("items.#").expect("valid count path");
    assert_eq!(
        count_path.steps,
        vec![GjsonStep::Key("items".to_string()), GjsonStep::ArrayCount]
    );
}

#[test]
fn e040_e041_w011_w040_w041_gjson_and_json_schema_validation() {
    // 1. E040: malformed GJSON dot-paths in `from=`, `json path=`, and `jsonify path=`
    let bad_paths = [
        "",
        ".a",
        "a.",
        "..",
        "a..b",
        "a.-1",
        "-1",
        r"a\",
        r"\",
        r"\0",
        r"\-1",
        "a.#[x==1]",
        "a.#(x==1)",
        "#[x==1]",
        "a.*.b",
        "a.?",
        "items.@reverse",
    ];

    for bad in bad_paths {
        assert!(
            is_malformed_gjson_path(bad),
            "expected '{bad}' to be malformed"
        );
        let escaped = bad.replace('\\', "\\\\");
        let src = format!(
            "json Item path=\"{escaped}\" {{\n    val str from=\"{escaped}\"\n}}\nstruct S {{\n    f {{\n        css \"script\"\n        text\n        jsonify Item path=\"{escaped}\"\n    }}\n}}\n"
        );
        let res = check_source(&src, Some("gjson_bad.kdl"));
        let e040_count = res
            .errors()
            .filter(|d| d.code == DiagnosticCode::E040)
            .count();
        assert_eq!(
            e040_count, 3,
            "expected 3 E040 errors for bad path '{bad}', got: {:?}",
            res.diagnostics
        );
    }

    // 2. E041: duplicate JSON field names, duplicate `from=` wire keys, and alias conflicts
    assert!(error_codes("json F { x str; x int }").contains(&DiagnosticCode::E041));
    assert!(
        error_codes("json F { a str from=\"wire\"; b str from=\"wire\" }")
            .contains(&DiagnosticCode::E041)
    );
    assert!(
        error_codes("json F { first str from=\"second\"; second str }")
            .contains(&DiagnosticCode::E041)
    );

    // 3. W011: deprecated positional alias on `json` field
    let w011_src = "json User {\n    user_id int \"id\"\n}\n";
    assert_eq!(warning_codes(w011_src), vec![DiagnosticCode::W011]);

    // Positional alias + `from=` together -> E001
    assert!(
        error_codes("json User {\n    user_id int \"id\" from=\"ident\"\n}\n")
            .contains(&DiagnosticCode::E001)
    );

    // 4. W040: leading/trailing whitespace in `from=` path
    let w040_src = "json User {\n    user_id int from=\"  a.b  \"\n}\n";
    assert_eq!(warning_codes(w040_src), vec![DiagnosticCode::W040]);

    // 5. W041: `path=` used on a `json` field instead of `from=`
    let w041_src = "json User {\n    user_id int path=\"id\"\n}\n";
    assert_eq!(warning_codes(w041_src), vec![DiagnosticCode::W041]);

    // 6. E403: non-portable JSON field names (`url:`, `@type`, `track-id`) with hints
    let e403_src = "json Article {\n    url: str\n    @type str\n    track-id str\n}\n";
    let res_e403 = check_source(e403_src, Some("e403.kdl"));
    let e403s: Vec<_> = res_e403
        .errors()
        .filter(|d| d.code == DiagnosticCode::E403)
        .collect();
    assert_eq!(e403s.len(), 3);
    assert!(
        e403s[0]
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("separated by space ('url str')")
    );
    assert!(
        e403s[1]
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("type str from=\"@type\"")
    );
    assert!(
        e403s[2]
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("track_id str from=\"track-id\"")
    );
}

#[test]
fn inline_dict_and_value_blocks_validation() {
    // Valid nested inline dict & @value blocks (including `field (dict) { ... }` form)
    let valid_inline = r#"
json AnimeResponse {
    translations (dict)Translation {
        @key str
        @value TranslationValue {
            episodes (dict) {
                @key int
                @value {
                    link str
                    screenshots @skip
                }
            }
            is_active bool
        }
    }
    extra TranslationValue
}
"#;
    let res_valid = check_source(valid_inline, Some("inline_ok.kdl"));
    assert!(
        !res_valid.has_errors(),
        "unexpected errors: {:?}",
        res_valid.diagnostics
    );

    // Empty inline @value block -> E001
    assert!(error_codes("(dict)json T { @key str; @value {} }").contains(&DiagnosticCode::E001));

    // @skip on inline @value block -> E002
    assert!(
        error_codes("(dict)json T { @key str; @value @skip { link str } }")
            .contains(&DiagnosticCode::E002)
    );

    // Anonymous array in @value without item model name -> E001
    assert!(
        error_codes("(dict)json T { @key str; (array)@value { link str } }")
            .contains(&DiagnosticCode::E001)
    );

    // Colliding inline model name -> E001
    let colliding_inline = r#"
json Model { id int }
(dict)json T {
    @key str
    @value Model { link str }
}
"#;
    assert!(error_codes(colliding_inline).contains(&DiagnosticCode::E001));
}

// ============================================================================
// 7. E302 (Intra-file Top-Down Declaration Order vs Explicit Imports)
// ============================================================================

#[test]
fn e302_intra_file_top_down_order_enforced_while_imports_allowed() {
    // 1. Local `struct` referenced via `nested` before its declaration -> E302
    let bad_nested_order = r#"
struct Main {
    child { nested Child }
}
struct Child {
    title { css "h1"; text }
}
"#;
    let res_nested = check_source(bad_nested_order, Some("order_struct.kdl"));
    assert!(res_nested.errors().any(|d| {
        d.code == DiagnosticCode::E302
            && d.message
                .contains("'nested Child': struct 'Child' must be declared BEFORE use")
    }));

    // 2. Local `json` referenced in another `json` field before declaration -> E302
    let bad_json_field_order = r#"
json Parent {
    items (array)Child?
}
json Child {
    id int
}
"#;
    assert!(error_codes(bad_json_field_order).contains(&DiagnosticCode::E302));

    // 3. Local `json` referenced via `jsonify` before declaration -> E302
    let bad_jsonify_order = r#"
struct Scraper {
    data { css "script"; text; jsonify Payload }
}
json Payload {
    id int
}
"#;
    assert!(error_codes(bad_jsonify_order).contains(&DiagnosticCode::E302));

    // 4. Local `json` referenced via `@request response=` or `@error` before declaration -> E302
    let bad_rest_order = r#"
(rest)struct Client {
    @request response="User" "curl https://api.example.com/user"
    @error 404 ApiErr
}
json User { id int }
json ApiErr { message str }
"#;
    let res_rest_order = check_source(bad_rest_order, Some("order_rest.kdl"));
    let e302_count = res_rest_order
        .errors()
        .filter(|d| d.code == DiagnosticCode::E302)
        .count();
    assert_eq!(e302_count, 2);

    // 5. Explicitly imported symbols resolve topologically ahead of local declarations (no E302)
    let vfs = MemorySourceLoader::new()
        .with_file(
            "shared.kdl",
            "json SharedJson { id int }\nstruct SharedStruct { title { css \"h1\"; text } }\n",
        )
        .with_file(
            "main.kdl",
            r#"
import "./shared.kdl" {
    (json)SharedJson
    (struct)SharedStruct
}
json LocalJson {
    shared SharedJson
}
struct Main {
    s { nested SharedStruct }
    j { css "script"; text; jsonify LocalJson }
}
"#,
        );
    let res_imported = check_with_loader("main.kdl", &vfs);
    assert!(
        !res_imported.has_errors(),
        "unexpected errors for imported symbols: {:?}",
        res_imported.diagnostics
    );
}

#[test]
fn field_types_and_value_constants_linter_validation() {
    // 1. Valid struct, @init, and json with postfix types and value= constants
    let valid_src = r#"
define SOURCE="habr"
extension Feed {
    now_ts {
        sig int int
        py { emit "{{out}} = {{in}}" }
        js { emit "const {{out}} = {{in}};" }
        go { emit "{{out}} := {{in}}" }
        rust { emit "let {{out}} = {{in}};" }
    }
    gen_id {
        sig str str
        py { emit "{{out}} = {{in}}" }
        js { emit "const {{out}} = {{in}};" }
        go { emit "{{out}} := {{in}}" }
        rust { emit "let {{out}} = {{in}};" }
    }
}

(list)struct HabrFeed {
    @split-doc { css-all "article.item" }
    @init {
        fetched_at int value=0 { !Feed.now_ts }
        const_init str value="{{SOURCE}}"
    }
    source str value="{{SOURCE}}"
    kind value="article"
    version int value=1
    rating float value=4.5
    is_live bool value=#false
    id str value="{{SOURCE}}" { !Feed.gen_id }
    title str { css "a.title"; text; trim }
    subtitle str? { css ".lead"; text; trim; fallback #null }
    tags (array)str { css-all ".tag"; text; trim }
    counts (array)int { css-all ".cnt"; text }
    scores (array)float { css-all ".score"; text }
    has_hubs bool { css ".hubs" }
    views int { css ".views"; text; fallback 0 }
    url { css "a.title"; attr "href" }
}

(raw)struct RawFeed {
    source str value="{{SOURCE}}"
    seeded str value="{{SOURCE}}" { !Feed.gen_id }
    token str { trim }
}

(flat)struct FlatFeed {
    header str value="{{SOURCE}}"
    links (array)str { css-all "a"; attr "href" }
}

json ReleaseCard {
    source str value="{{SOURCE}}"
    channel str value="{{SOURCE}}"
    raw_source str from="source"
    version int value=1
    schema_version int value=1
    score float value=9.5
    is_live bool value=#false
    id str from="node_id"
}
"#;
    let res_ok = check_source(valid_src, Some("valid_types_val.kdl"));
    assert!(
        !res_ok.has_errors(),
        "expected no errors, got: {:?}",
        res_ok.diagnostics
    );

    // 2. Prefix type annotation on struct or @init field -> E001 with postfix hint
    let res_prefix = check_source(
        "struct S {\n    (str)title { css \"h1\"; text }\n}\n",
        Some("prefix.kdl"),
    );
    let d_prefix = res_prefix
        .errors()
        .find(|d| d.code == DiagnosticCode::E001)
        .expect("expected E001 for (str)title");
    assert!(d_prefix.message.contains("prefix type annotation '(str)'"));
    assert!(d_prefix.hint.as_deref().unwrap_or("").contains("title str"));

    let res_init_prefix = check_source(
        "struct S {\n    @init { (int)ver value=1 }\n    f { @ver }\n}\n",
        Some("init_prefix.kdl"),
    );
    let d_init_prefix = res_init_prefix
        .errors()
        .find(|d| d.code == DiagnosticCode::E001)
        .expect("expected E001 for (int)ver in @init");
    assert!(
        d_init_prefix
            .message
            .contains("prefix type annotation '(int)'")
    );
    assert!(
        d_init_prefix
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("ver int")
    );

    // 3. Invalid field arguments / properties / empty block alongside value= -> E001
    let e001_cases = [
        "struct S { title }",
        "struct S { title str }",
        "struct S { title str int { css \"a\"; text } }",
        "struct S { title unknown_type { css \"a\"; text } }",
        "struct S { title (tuple)str { css \"a\"; text } }",
        "struct S { title 123 { css \"a\"; text } }",
        "struct S { title foo=\"bar\" { css \"a\"; text } }",
        "struct S { source value=\"habr\" {} }",
        "struct S { @init { x value=\"a\" {} }; f { @x } }",
        "json J { a str? value=\"x\" }",
        "json J { a? str value=\"x\" }",
        "json J { a (array)str value=\"x\" }",
        "json J { (array)a str value=\"x\" }",
        "json J { a str @omitempty value=\"x\" }",
        "json J { a str @skip value=\"x\" }",
        "json J { a str from=\"wire\" value=\"x\" }",
        "json J { a str path=\"wire\" value=\"x\" }",
        "json J { a str \"wire\" value=\"x\" }",
        "json J { a value=\"x\" { id int } }",
        "json J {\n    a (dict) value=\"x\" { @key str; @value str }\n}",
    ];
    for src in e001_cases {
        let codes = error_codes(src);
        assert!(
            codes.contains(&DiagnosticCode::E001),
            "expected E001 for: {src}, got: {codes:?}"
        );
    }

    // 4. value=#null or block define interpolation in value= -> E002
    let e002_cases = [
        "struct S { f value=#null }",
        "json J { f str value=#null }",
        "define BLK { css \"a\"; text }\nstruct S { f value=\"{{BLK}}\" }",
        "define BLK { id int }\njson J { f str value=\"{{BLK}}\" }",
    ];
    for src in e002_cases {
        let codes = error_codes(src);
        assert!(
            codes.contains(&DiagnosticCode::E002),
            "expected E002 for: {src}, got: {codes:?}"
        );
    }

    // 5. Undefined {{DEFINE}} in value= -> E300
    assert!(
        error_codes("struct S { f value=\"{{MISSING_DEF}}\" }").contains(&DiagnosticCode::E300)
    );
    assert!(
        error_codes("json J { f str value=\"{{MISSING_DEF}}\" }").contains(&DiagnosticCode::E300)
    );

    // 6. Standalone value= type mismatch on struct or json field -> E100
    let e100_cases = [
        "struct S { version int value=\"one\" }",
        "struct S { tags (array)str value=\"a\" }",
        "struct S { opt str? value=\"a\" }",
        "json J { version int value=\"one\" }",
        "json J { name str value=42 }",
        "json Sub { id int }\njson J { sub Sub value=1 }",
    ];
    for src in e100_cases {
        let codes = error_codes(src);
        assert!(
            codes.contains(&DiagnosticCode::E100),
            "expected E100 for: {src}, got: {codes:?}"
        );
    }

    // 7. value= on (table)struct regular field -> E203
    let table_val_src = r#"
(table)struct T {
    @table { css "table" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }
    col value="const"
}
"#;
    assert!(error_codes(table_val_src).contains(&DiagnosticCode::E203));
}
