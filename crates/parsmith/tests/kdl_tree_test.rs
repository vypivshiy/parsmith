use parsmith::kdl::{
    KdlArg, KdlDocument, KdlNode, KdlProp, KdlValue, LineIndex, NodeRef, Span, escape_quoted,
    is_bare_ident, parse,
};

const NAVIGATION_DOC: &str = r#"app "my-service" version="1.0.0" {
    (network)server "primary" port=8080 tls=#true {
        host "localhost"
        host "127.0.0.1"
        timeout idle=30 connect=5
    }

    (network)server "replica" port=8081 tls=#false {
        host "replica.local"
        timeout idle=60 connect=5
    }

    router {
        route "GET" "/api/users" handler="users.list" auth=#true
        route "POST" "/api/users" handler="users.create" auth=#true
    }

    plugins {
        plugin "auth" enabled=#true {
            (jwt)secret "hs256" key=(regex)"hs(256|512)"
            expires (i32)3600
        }
        plugin "debug" enabled=#false
    }
}
"#;

#[test]
fn parse_valid_kdl2_features() {
    let src = r####"
// Top-level comment
/* Outer block comment
   /* Nested block comment */
*/
(item)struct Product version=2 active=#true deleted=#false fallback=#null {
    title "h1.name" {
        text
        trim
    }
    price .price-tag {
        re #"(\d+\.\d+)"#
        to-float
    }
    raw-multi ##"hello "world" #1"##
    multiline """
        first line
        second line
        """
    raw-multiline #"""
        (?xs)
        ^foo\s+bar$
        """#
    numbers 42 -17 0x10 0o10 0b10 1_000_000 3.25 -0.5 #inf #-inf #nan
    matrix 1 2 \
           3 4
    /-omitted-node "ignored" {
        child 1
    }
    kept /-omitted-arg "visible" /-omitted_prop=1 kept_prop=(u16)8080 /-{
        omitted_child
    }
    inline { first; second "2"; third key=val }
}
"####;

    let doc = parse(src).expect("valid KDL 2.0 should parse");
    assert_eq!(doc.len(), 1);

    let root = doc.roots()[0];
    assert_eq!(root.name(), "struct");
    assert_eq!(root.ty(), Some("item"));
    assert_eq!(root.arg(0).unwrap().as_str(), Some("Product"));
    assert_eq!(root.prop("version").unwrap().as_int(), Some(2));
    assert_eq!(root.prop("active").unwrap().as_bool(), Some(true));
    assert_eq!(root.prop("deleted").unwrap().as_bool(), Some(false));
    assert!(root.prop("fallback").unwrap().is_null());
    assert!(root.has_children());

    let children = root.children();
    let names: Vec<&str> = children.iter().map(|n| n.name()).collect();
    assert_eq!(
        names,
        vec![
            "title",
            "price",
            "raw-multi",
            "multiline",
            "raw-multiline",
            "numbers",
            "matrix",
            "kept",
            "inline",
        ]
    );

    let price = children[1];
    assert_eq!(price.arg(0).unwrap().as_str(), Some(".price-tag"));
    assert_eq!(
        price.children()[0].arg(0).unwrap().as_str(),
        Some(r"(\d+\.\d+)")
    );

    let raw_multi = children[2];
    assert_eq!(
        raw_multi.arg(0).unwrap().as_str(),
        Some(r#"hello "world" #1"#)
    );

    let multiline = children[3];
    assert_eq!(
        multiline.arg(0).unwrap().as_str(),
        Some("first line\nsecond line")
    );

    let raw_multiline = children[4];
    assert_eq!(
        raw_multiline.arg(0).unwrap().as_str(),
        Some("(?xs)\n^foo\\s+bar$")
    );

    let numbers = children[5];
    assert_eq!(numbers.arg(0).unwrap().as_int(), Some(42));
    assert_eq!(numbers.arg(1).unwrap().as_int(), Some(-17));
    assert_eq!(numbers.arg(2).unwrap().as_int(), Some(16));
    assert_eq!(numbers.arg(3).unwrap().as_int(), Some(8));
    assert_eq!(numbers.arg(4).unwrap().as_int(), Some(2));
    assert_eq!(numbers.arg(5).unwrap().as_int(), Some(1_000_000));
    assert_eq!(numbers.arg(6).unwrap().as_float(), Some(3.25));
    assert_eq!(numbers.arg(7).unwrap().as_float(), Some(-0.5));
    assert_eq!(numbers.arg(8).unwrap().as_float(), Some(f64::INFINITY));
    assert_eq!(numbers.arg(9).unwrap().as_float(), Some(f64::NEG_INFINITY));
    assert!(numbers.arg(10).unwrap().as_float().unwrap().is_nan());
    // NaN equality in KdlValue works reflexively
    assert_eq!(numbers.arg(10).unwrap().value(), &KdlValue::Float(f64::NAN));

    let matrix = children[6];
    let matrix_vals: Vec<i128> = matrix.args().iter().map(|a| a.as_int().unwrap()).collect();
    assert_eq!(matrix_vals, vec![1, 2, 3, 4]);

    let kept = children[7];
    assert_eq!(kept.args().len(), 1);
    assert_eq!(kept.arg(0).unwrap().as_str(), Some("visible"));
    assert_eq!(kept.props().len(), 1);
    let kept_prop = kept.prop("kept_prop").unwrap();
    assert_eq!(kept_prop.ty(), Some("u16"));
    assert_eq!(kept_prop.as_int(), Some(8080));
    assert!(!kept.has_children());

    let inline = children[8];
    assert_eq!(inline.children().len(), 3);
    assert_eq!(inline.children()[0].name(), "first");
    assert_eq!(inline.children()[1].name(), "second");
    assert_eq!(
        inline.children()[2].prop("key").unwrap().as_str(),
        Some("val")
    );
}

#[test]
fn exact_spans_across_multiline_document() {
    let src = "(item)struct Product {\n    field (str)\"val\" from=(gjson)\"a.b\"\n}\n";
    let doc = parse(src).expect("should parse");

    let root = doc.roots()[0];
    // Line 1: `(item)struct Product {` -> closes on Line 3 `}`
    assert_eq!(
        root.type_span(),
        Some(Span {
            offset: 0,
            len: 6,
            line: 1,
            col: 1,
            end_line: 1,
            end_col: 7,
        })
    );
    assert_eq!(&src[root.type_span().unwrap().byte_range()], "(item)");

    assert_eq!(
        root.name_span(),
        Span {
            offset: 6,
            len: 6,
            line: 1,
            col: 7,
            end_line: 1,
            end_col: 13,
        }
    );
    assert_eq!(&src[root.name_span().byte_range()], "struct");

    let prod_arg = root.arg(0).unwrap();
    assert_eq!(
        prod_arg.span(),
        Span {
            offset: 13,
            len: 7,
            line: 1,
            col: 14,
            end_line: 1,
            end_col: 21,
        }
    );
    assert_eq!(prod_arg.value_span(), prod_arg.span());
    assert_eq!(prod_arg.type_span(), None);
    assert_eq!(&src[prod_arg.span().byte_range()], "Product");

    assert_eq!(root.span().line, 1);
    assert_eq!(root.span().col, 1);
    assert_eq!(root.span().end_line, 3);
    assert_eq!(root.span().end_col, 2);

    // Line 2: `    field (str)"val" from=(gjson)"a.b"`
    let field = root.children()[0];
    assert_eq!(field.name(), "field");
    assert_eq!(field.name_span().line, 2);
    assert_eq!(field.name_span().col, 5);
    assert_eq!(field.name_span().end_line, 2);
    assert_eq!(field.name_span().end_col, 10);
    assert_eq!(&src[field.name_span().byte_range()], "field");

    let f_arg = field.arg(0).unwrap();
    assert_eq!(f_arg.ty(), Some("str"));
    assert_eq!(f_arg.as_str(), Some("val"));
    assert_eq!(&src[f_arg.span().byte_range()], "(str)\"val\"");
    assert_eq!(&src[f_arg.type_span().unwrap().byte_range()], "(str)");
    assert_eq!(&src[f_arg.value_span().byte_range()], "\"val\"");
    assert_eq!(f_arg.type_span().unwrap().line, 2);
    assert_eq!(f_arg.type_span().unwrap().col, 11);
    assert_eq!(f_arg.type_span().unwrap().end_col, 16);
    assert_eq!(f_arg.value_span().line, 2);
    assert_eq!(f_arg.value_span().col, 16);
    assert_eq!(f_arg.value_span().end_col, 21);

    let f_prop = field.prop("from").unwrap();
    assert_eq!(f_prop.key(), "from");
    assert_eq!(f_prop.ty(), Some("gjson"));
    assert_eq!(f_prop.as_str(), Some("a.b"));
    assert_eq!(&src[f_prop.span().byte_range()], "from=(gjson)\"a.b\"");
    assert_eq!(&src[f_prop.key_span().byte_range()], "from");
    assert_eq!(&src[f_prop.type_span().unwrap().byte_range()], "(gjson)");
    assert_eq!(&src[f_prop.value_span().byte_range()], "\"a.b\"");
    assert_eq!(f_prop.key_span().line, 2);
    assert_eq!(f_prop.key_span().col, 22);
    assert_eq!(f_prop.key_span().end_col, 26);
    assert_eq!(f_prop.type_span().unwrap().col, 27);
    assert_eq!(f_prop.type_span().unwrap().end_col, 34);
    assert_eq!(f_prop.value_span().col, 34);
    assert_eq!(f_prop.value_span().end_col, 39);
}

#[test]
fn line_index_handles_crlf_and_unicode_columns() {
    let src = "café 1\r\nemoji \"💯\" key=2\r\n";
    let doc = parse(src).expect("should parse unicode and CRLF");
    let idx = doc.line_index();
    assert_eq!(idx.line_count(), 3);
    assert_eq!(idx.line_text(1), Some("café 1"));
    assert_eq!(idx.line_text(2), Some("emoji \"💯\" key=2"));

    let cafe = doc.roots()[0];
    assert_eq!(cafe.name(), "café");
    assert_eq!(cafe.name_span().line, 1);
    assert_eq!(cafe.name_span().col, 1);
    // "café" is 4 chars (5 UTF-8 bytes) -> end_col is 5, arg `1` is at col 6
    assert_eq!(cafe.name_span().end_col, 5);
    assert_eq!(cafe.arg(0).unwrap().span().col, 6);

    let emoji = doc.roots()[1];
    assert_eq!(emoji.name_span().line, 2);
    assert_eq!(emoji.name_span().col, 1);
    // `emoji "💯" key=2`: "💯" is at col 7..10, `key=2` starts at col 11
    assert_eq!(emoji.arg(0).unwrap().span().col, 7);
    assert_eq!(emoji.arg(0).unwrap().span().end_col, 10);
    assert_eq!(emoji.prop("key").unwrap().key_span().col, 11);
}

#[test]
fn syntax_errors_carry_spans_and_hints() {
    // 1. Unclosed children block
    let err = parse("struct Broken {\n    field 1\n").unwrap_err();
    assert_eq!(err.line(), 1);
    assert_eq!(err.col(), 15);
    assert!(!err.message().is_empty());
    assert!(err.hint().is_some());

    // 2. Reserved bare keyword `true` in KDL 2.0
    let err_kw = parse("node flag=true\n").unwrap_err();
    assert_eq!(err_kw.line(), 1);
    assert!(err_kw.hint().unwrap().contains("#true"));

    // 3. Unterminated string on line 2
    let err_str = parse("ok 1\nbad \"unterminated").unwrap_err();
    assert_eq!(err_str.line(), 2);
    assert!(err_str.col() >= 5);
    assert!(err_str.hint().is_some());

    // 4. Unterminated raw string
    let err_raw = parse("re #\"(\\d+)\"").unwrap_err();
    assert_eq!(err_raw.line(), 1);
    assert!(err_raw.hint().is_some());

    // 5. Invalid float
    let err_float = parse("foo 1.").unwrap_err();
    assert_eq!(err_float.line(), 1);
    assert_eq!(err_float.col(), 5);
    assert!(err_float.hint().is_some());

    // 6. Escline followed by single-line `//` comment across lines
    let err_esc = parse(
        "endpoint name=\"service\" \\\n        target=\"replica\" \\\n        // explanatory comment\n        \"https://example.com\"\n",
    )
    .unwrap_err();
    assert_eq!(err_esc.line(), 3);
    assert_eq!(err_esc.col(), 9);
    assert!(err_esc.hint().unwrap().contains("/* ... */"));

    // Escline with blank line before `//` comment also fails
    let err_esc_blank = parse("node \\\n\n    // comment\n    arg\n").unwrap_err();
    assert_eq!(err_esc_blank.line(), 3);
    assert_eq!(err_esc_blank.col(), 5);

    // Block comment across escline is valid
    let ok_block = parse("node \\\n    /* comment */ \\\n    arg\n").expect("valid");
    assert_eq!(ok_block.roots().len(), 1);
    assert_eq!(ok_block.roots()[0].args().len(), 1);
}

#[test]
fn navigation_methods_work_across_tree() {
    let doc = parse(NAVIGATION_DOC).expect("navigation doc should parse");
    assert_eq!(doc.len(), 1);

    let app = doc.roots()[0];
    assert_eq!(app.name(), "app");
    assert_eq!(app.parent(), None);
    assert_eq!(app.root(), app);
    assert_eq!(app.depth(), 0);
    assert_eq!(app.index(), 0);
    assert_eq!(app.siblings(), vec![app]);
    assert_eq!(app.prev_sibling(), None);
    assert_eq!(app.next_sibling(), None);
    assert!(app.parents().is_empty());

    let app_children = app.children();
    assert_eq!(app_children.len(), 4);
    let server_primary = app_children[0];
    let server_replica = app_children[1];
    let router = app_children[2];
    let plugins = app_children[3];

    assert_eq!(server_primary.name(), "server");
    assert_eq!(server_primary.ty(), Some("network"));
    assert_eq!(server_primary.parent(), Some(app));
    assert_eq!(server_primary.root(), app);
    assert_eq!(server_primary.depth(), 1);
    assert_eq!(server_primary.index(), 0);
    assert_eq!(server_primary.prev_sibling(), None);
    assert_eq!(server_primary.next_sibling(), Some(server_replica));

    assert_eq!(server_replica.index(), 1);
    assert_eq!(server_replica.prev_sibling(), Some(server_primary));
    assert_eq!(server_replica.next_sibling(), Some(router));

    assert_eq!(router.index(), 2);
    assert_eq!(router.prev_sibling(), Some(server_replica));
    assert_eq!(router.next_sibling(), Some(plugins));

    assert_eq!(plugins.index(), 3);
    assert_eq!(plugins.prev_sibling(), Some(router));
    assert_eq!(plugins.next_sibling(), None);
    assert_eq!(plugins.siblings(), app_children);

    // Deep grandchild navigation
    let auth_plugin = plugins.children()[0];
    let secret = auth_plugin.children()[0];
    assert_eq!(secret.name(), "secret");
    assert_eq!(secret.ty(), Some("jwt"));
    assert_eq!(secret.depth(), 3);
    assert_eq!(secret.root(), app);
    assert_eq!(secret.parent(), Some(auth_plugin));
    assert_eq!(secret.parents(), vec![auth_plugin, plugins, app]);

    // `closest` with closure predicates and `closest_by`
    assert_eq!(
        secret.closest(|n: NodeRef| n.name() == "secret"),
        Some(secret)
    );
    assert_eq!(
        secret.closest_by(|n| n.name() == "plugin"),
        Some(auth_plugin)
    );
    assert_eq!(secret.closest_by(|n| n.name() == "plugins"), Some(plugins));
    assert_eq!(secret.closest(|n: NodeRef| n.name() == "app"), Some(app));
    assert_eq!(secret.closest_by(|n| n.name() == "nonexistent"), None);

    // `closest` with selector strings
    assert_eq!(secret.closest("secret"), Some(secret));
    assert_eq!(secret.closest("(jwt)secret"), Some(secret));
    assert_eq!(secret.closest("(other)secret"), None);
    assert_eq!(secret.closest("plugin"), Some(auth_plugin));
    assert_eq!(secret.closest("app"), Some(app));
    assert_eq!(
        server_primary.children()[0].closest("(network)server"),
        Some(server_primary)
    );

    // Pre-order iteration
    let all_names: Vec<&str> = doc.iter_nodes().map(|n| n.name()).collect();
    assert_eq!(
        all_names,
        vec![
            "app", "server", "host", "host", "timeout", "server", "host", "timeout", "router",
            "route", "route", "plugins", "plugin", "secret", "expires", "plugin",
        ]
    );
}

#[test]
fn serializer_bare_ident_and_escaping_rules() {
    for valid in [
        "simple",
        "kebab-case_123",
        "--flag",
        "+plus-ident",
        ".dot-ident",
        "+.plus-dot-ident",
        "-.minus-dot-ident",
        "café",
        "日本語",
        "@request",
        "@error:rate_limit",
    ] {
        assert!(is_bare_ident(valid), "expected {valid:?} to be bare ident");
    }

    for invalid in [
        "",
        "true",
        "false",
        "null",
        "inf",
        "-inf",
        "nan",
        "0",
        "42",
        "-1",
        "+1",
        ".5",
        "-.5",
        "+.5",
        "-9abc",
        "with space",
        "a/b",
        "a\\b",
        "a(b)",
        "a{b}",
        "a[b]",
        "a;b",
        "a\"b",
        "a#b",
        "a=b",
        "a\nb",
        "a\tb",
    ] {
        assert!(
            !is_bare_ident(invalid),
            "expected {invalid:?} to NOT be bare ident"
        );
    }

    assert_eq!(escape_quoted("hello world"), "\"hello world\"");
    assert_eq!(
        escape_quoted("a\"b\\c\nd\re\tf\u{08}g\u{0c}h"),
        r#""a\"b\\c\nd\re\tf\bg\fh""#
    );
    assert_eq!(
        escape_quoted("\u{0000}\u{0007}\u{001F}\u{007F}\u{200E}\u{202E}\u{FEFF}"),
        r#""\u{0}\u{7}\u{1F}\u{7F}\u{200E}\u{202E}\u{FEFF}""#
    );
}

#[test]
fn programmatic_construction_to_kdl_and_roundtrip() {
    let mut doc = KdlDocument::new();
    assert_eq!(doc.to_kdl(), "");

    let struct_id = doc.add_node(
        KdlNode::new("struct")
            .with_ty("item")
            .with_arg("Product")
            .with_prop("version", 2),
    );
    doc.add_child_node(
        struct_id,
        KdlNode::new("@request").with_arg("GET https://example.com"),
    );
    let check_id = doc.add_child_node(struct_id, KdlNode::new("@check").with_arg("exists"));
    doc.add_child_node(check_id, KdlNode::new("css").with_arg(".foo"));
    doc.add_child_node(check_id, KdlNode::new("to-bool"));
    doc.add_child_node(check_id, KdlNode::new("fallback").with_arg(false));

    let expected = "\
(item)struct Product version=2 {
    @request \"GET https://example.com\"
    @check exists {
        css .foo
        to-bool
        fallback #false
    }
}
";
    assert_eq!(doc.to_kdl(), expected);

    // Round-trip through `parse`
    let reparsed = parse(&doc.to_kdl()).expect("emitted KDL must round-trip");
    assert_eq!(reparsed.to_kdl(), expected);

    // Round-trip of NAVIGATION_DOC
    let nav_doc = parse(NAVIGATION_DOC).unwrap();
    let nav_kdl = nav_doc.to_kdl();
    let nav_reparsed = parse(&nav_kdl).unwrap();
    assert_eq!(nav_reparsed.to_kdl(), nav_kdl);

    // Standalone node formatting options
    let empty_node = KdlNode::new("empty");
    assert_eq!(empty_node.to_kdl(), "empty");
    assert_eq!(empty_node.to_kdl_with(0, "    ", true), "empty {}");

    // Trailing comment & multiline empty children block (used by json-to-kdl)
    let skip_node = KdlNode::new("meta")
        .with_arg("@skip")
        .with_trailing_comment("empty object");
    assert_eq!(skip_node.to_kdl(), "meta @skip // empty object");

    let empty_json = KdlNode::new("json")
        .with_arg("JsonResponse")
        .with_empty_children_multiline(true);
    assert_eq!(empty_json.to_kdl(), "json JsonResponse {\n}");

    // Verify property override semantics (last matching property wins in `prop`/`get`)
    let dup = parse("node key=1 key=2\n").unwrap();
    let r = dup.roots()[0];
    assert_eq!(r.props().len(), 2);
    assert_eq!(r.get("key").unwrap().as_int(), Some(2));

    // LineIndex default
    let empty_idx = LineIndex::default();
    assert_eq!(empty_idx.line_count(), 1);
    assert_eq!(empty_idx.line_col(0), (1, 1));

    // KdlArg and KdlProp direct constructors
    let a = KdlArg::new("x").with_ty("(u8)");
    assert_eq!(a.ty(), Some("u8"));
    assert_eq!(a.to_kdl(), "(u8)x");

    let p = KdlProp::new("true", 10).with_ty("i32");
    assert_eq!(p.to_kdl(), "\"true\"=(i32)10");
}

#[test]
fn multi_root_navigation_and_document_level_methods() {
    let src = "\
root {
    child-a {
        grandchild 1
    }
    child-b \"val\"
}
top-level 42
third-root #true
";
    let doc = parse(src).expect("should parse");
    let roots = doc.roots();
    assert_eq!(roots.len(), 3);

    let r0 = roots[0];
    let r1 = roots[1];
    let r2 = roots[2];

    assert_eq!(r0.index(), 0);
    assert_eq!(r1.index(), 1);
    assert_eq!(r2.index(), 2);

    assert_eq!(r0.prev_sibling(), None);
    assert_eq!(r0.next_sibling(), Some(r1));
    assert_eq!(r1.prev_sibling(), Some(r0));
    assert_eq!(r1.next_sibling(), Some(r2));
    assert_eq!(r2.prev_sibling(), Some(r1));
    assert_eq!(r2.next_sibling(), None);

    assert_eq!(r0.siblings(), vec![r0, r1, r2]);
    assert_eq!(r1.siblings(), vec![r0, r1, r2]);
    assert_eq!(r2.siblings(), vec![r0, r1, r2]);

    assert_eq!(r0.root(), r0);
    assert_eq!(r1.root(), r1);
    assert_eq!(r2.root(), r2);

    let child_a = r0.children()[0];
    let child_b = r0.children()[1];
    let grandchild = child_a.children()[0];

    let desc_names: Vec<&str> = r0.descendants().into_iter().map(|n| n.name()).collect();
    assert_eq!(desc_names, vec!["child-a", "grandchild", "child-b"]);
    assert!(grandchild.descendants().is_empty());

    // Verify document-level methods agree with NodeRef methods
    for node in doc.iter_nodes() {
        let id = node.id();
        assert_eq!(doc.parent(id), node.parent());
        assert_eq!(doc.root(id), node.root());
        assert_eq!(doc.depth(id), node.depth());
        assert_eq!(doc.index(id), node.index());
        assert_eq!(doc.children(id), node.children());
        assert_eq!(doc.siblings(id), node.siblings());
        assert_eq!(doc.prev_sibling(id), node.prev_sibling());
        assert_eq!(doc.next_sibling(id), node.next_sibling());
        assert_eq!(doc.parents(id), node.parents());
        assert_eq!(doc.closest(id, "root"), node.closest("root"));
    }

    assert_eq!(child_b.prev_sibling(), Some(child_a));
    assert_eq!(child_a.next_sibling(), Some(child_b));
}

#[test]
fn mutation_insert_and_special_values_roundtrip() {
    let mut doc = parse("a\nc\n").unwrap();
    let b_id = doc.insert_node(
        1,
        KdlNode::new("b")
            .with_ty("a/b")
            .with_arg(f64::INFINITY)
            .with_arg(f64::NEG_INFINITY)
            .with_arg(f64::NAN)
            .with_arg(1.0f64)
            .with_typed_prop("quoted key", "my/type", "hello\nworld"),
    );

    let roots = doc.roots();
    assert_eq!(
        roots.iter().map(|n| n.name()).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(roots[0].index(), 0);
    assert_eq!(roots[1].index(), 1);
    assert_eq!(roots[2].index(), 2);
    assert_eq!(roots[1].prev_sibling(), Some(roots[0]));
    assert_eq!(roots[1].next_sibling(), Some(roots[2]));

    doc.add_child(b_id, "first");
    doc.add_child(b_id, "third");
    doc.insert_child_node(b_id, 1, KdlNode::new("second").with_arg(2));

    let b_children = doc.node(b_id).children();
    assert_eq!(
        b_children.iter().map(|n| n.name()).collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
    assert_eq!(b_children[0].index(), 0);
    assert_eq!(b_children[1].index(), 1);
    assert_eq!(b_children[2].index(), 2);
    assert_eq!(b_children[1].prev_sibling(), Some(b_children[0]));
    assert_eq!(b_children[1].next_sibling(), Some(b_children[2]));

    let serialized = doc.to_kdl();
    let expected = "\
a
(\"a/b\")b #inf #-inf #nan 1.0 \"quoted key\"=(\"my/type\")\"hello\\nworld\" {
    first
    second 2
    third
}
c
";
    assert_eq!(serialized, expected);

    let reparsed = parse(&serialized).expect("roundtrip parse should succeed");
    assert_eq!(reparsed.to_kdl(), expected);
    let rb = reparsed.roots()[1];
    assert_eq!(rb.ty(), Some("a/b"));
    assert_eq!(rb.arg(0).unwrap().as_float(), Some(f64::INFINITY));
    assert_eq!(rb.arg(1).unwrap().as_float(), Some(f64::NEG_INFINITY));
    assert!(rb.arg(2).unwrap().as_float().unwrap().is_nan());
    assert_eq!(rb.arg(3).unwrap().as_float(), Some(1.0));
    let rprop = rb.prop("quoted key").unwrap();
    assert_eq!(rprop.ty(), Some("my/type"));
    assert_eq!(rprop.as_str(), Some("hello\nworld"));

    // Custom tab indentation
    assert_eq!(
        doc.node(b_id).to_kdl_with(0, "\t", false),
        "(\"a/b\")b #inf #-inf #nan 1.0 \"quoted key\"=(\"my/type\")\"hello\\nworld\" {\n\tfirst\n\tsecond 2\n\tthird\n}"
    );
}
