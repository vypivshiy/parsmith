use parsmith::kdl::{KdlDocument, parse};
use parsmith::{JsonToKdlError, json_to_kdl};

fn assert_valid_kdl(json_input: &str, name: &str) -> (String, KdlDocument) {
    let kdl = json_to_kdl(json_input, name).expect("json_to_kdl should succeed");
    let doc = parse(&kdl)
        .unwrap_or_else(|err| panic!("generated KDL failed to parse: {err}\n---\n{kdl}"));
    (kdl, doc)
}

#[test]
fn generates_nested_definitions_and_preserves_key_order() {
    let raw = r#"{
        "user": {"id": 1, "profile": {"display-name": "Ann"}},
        "tags": ["one", "two"]
    }"#;

    let (source, doc) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
json JsonResponse {
    user {
        id int
        profile {
            display_name str from=\"display-name\"
        }
    }
    tags (array)str
}
";
    assert_eq!(source, expected);
    assert_eq!(doc.len(), 1);
    let root = doc.roots()[0];
    assert_eq!(root.name(), "json");
    assert_eq!(root.ty(), None);
    assert_eq!(root.arg(0).unwrap().as_str(), Some("JsonResponse"));
}

#[test]
fn marks_unknown_and_empty_arrays_and_null_scalars() {
    let raw = r#"{"tags": [1, "x"], "empty": [], "value": null}"#;
    let (source, _) = assert_valid_kdl(raw, "JsonResponse");

    let expected = "\
json JsonResponse {
    tags (array)nil @skip // int, str
    empty (array)nil @skip // empty array
    value nil // unknown real type
}
";
    assert_eq!(source, expected);
}

#[test]
fn canonical_from_remapping_and_reserved_keywords() {
    let raw = r#"{
        "kebab-case": "val1",
        "@context": "https://schema.org",
        "123_num": 42,
        "class": "keyword",
        "for": "keyword",
        "null": 0,
        "type": "article",
        "fn": "callback",
        "nested-obj": {
            "inner-field": "inner_val"
        }
    }"#;

    let (source, doc) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
json JsonResponse {
    kebab_case str from=\"kebab-case\"
    context str from=\"@context\"
    k_123_num int from=\"123_num\"
    k_class str from=\"class\"
    k_for str from=\"for\"
    k_null int from=\"null\"
    k_type str from=\"type\"
    k_fn str from=\"fn\"
    nested_obj from=\"nested-obj\" {
        inner_field str from=\"inner-field\"
    }
}
";
    assert_eq!(source, expected);

    let root = doc.roots()[0];
    let children = root.children();
    assert_eq!(
        children[0].prop("from").unwrap().as_str(),
        Some("kebab-case")
    );
    assert_eq!(children[1].prop("from").unwrap().as_str(), Some("@context"));
    assert_eq!(children[2].prop("from").unwrap().as_str(), Some("123_num"));
    assert_eq!(
        children[8].prop("from").unwrap().as_str(),
        Some("nested-obj")
    );
}

#[test]
fn disambiguates_duplicate_normalized_field_names_deterministically() {
    let raw1 = r#"{"foo_bar": 1, "foo-bar": 2}"#;
    let (src1, _) = assert_valid_kdl(raw1, "JsonResponse");
    assert_eq!(
        src1,
        "\
json JsonResponse {
    foo_bar int
    foo_bar_2 int from=\"foo-bar\"
}
"
    );

    // Even when the non-identifier key appears before the exact identifier key,
    // the exact key keeps its bare name and the non-identifier key gets `_2`.
    let raw2 = r#"{"@id": "urn:1", "id": 1, "foo-bar": "a", "foo.bar": "b"}"#;
    let (src2, _) = assert_valid_kdl(raw2, "JsonResponse");
    assert_eq!(
        src2,
        "\
json JsonResponse {
    id_2 str from=\"@id\"
    id int
    foo_bar str from=\"foo-bar\"
    foo_bar_2 str from=\"foo.bar\"
}
"
    );
}

#[test]
fn empty_objects_emitted_as_skip_scalar() {
    let raw = r#"{
        "empty": {},
        "empty-remapped": {},
        "nested": {
            "inner_empty": {}
        }
    }"#;
    let (source, _) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
json JsonResponse {
    empty @skip // empty object
    empty_remapped @skip from=\"empty-remapped\" // empty object
    nested {
        inner_empty @skip // empty object
    }
}
";
    assert_eq!(source, expected);
}

#[test]
fn empty_root_object_and_custom_names() {
    let (empty_src, _) = assert_valid_kdl("{}", "JsonResponse");
    assert_eq!(empty_src, "json JsonResponse {\n}\n");

    let (custom_obj, _) = assert_valid_kdl(r#"{"name": "foo"}"#, "CustomModel");
    assert_eq!(custom_obj, "json CustomModel {\n    name str\n}\n");

    let (custom_arr, _) = assert_valid_kdl(r#"[{"name": "foo"}]"#, "Catalog");
    assert_eq!(custom_arr, "(array)json CatalogItem {\n    name str\n}\n");
}

#[test]
fn primitive_arrays_all_types_and_int_float_widening() {
    let raw = r#"{
        "strings": ["a", "b"],
        "ints": [1, 2],
        "floats": [1.5, 2.5],
        "mixed_numbers": [1, 2.5, 3],
        "nullable_strings": ["a", null],
        "bools": [true, false],
        "remap-list": ["x"]
    }"#;
    let (source, _) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
json JsonResponse {
    strings (array)str
    ints (array)int
    floats (array)float
    mixed_numbers (array)float
    nullable_strings (array)str?
    bools (array)bool
    remap_list (array)str from=\"remap-list\"
}
";
    assert_eq!(source, expected);
}

#[test]
fn root_array_with_omitempty_nullability_and_widening() {
    let raw = r#"[
        {"id": 1, "score": 10, "nickname": "Alice", "extra": "foo", "empty-sub": {}, "meta": {"ok": true}, "tags": ["a"]},
        {"id": 2, "score": 12.5, "nickname": null, "meta": null, "tags": null}
    ]"#;
    let (source, _) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
(array)json JsonResponseItem {
    id int
    score float
    nickname str?
    extra str @omitempty
    empty_sub @skip @omitempty from=\"empty-sub\" // empty object
    meta? {
        ok bool
    }
    tags (array)str?
}
";
    assert_eq!(source, expected);
}

#[test]
fn inline_array_of_objects_and_aliasing() {
    let raw = r#"{
        "users": [{"id": 1, "username": "alice", "active": true}],
        "settings": {
            "options": [{"key": "theme", "value": "dark"}]
        },
        "user-items": [
            {"item-id": 10, "label": "first"},
            {"item-id": 20}
        ]
    }"#;
    let (source, _) = assert_valid_kdl(raw, "JsonResponse");
    let expected = "\
json JsonResponse {
    users (array)UsersItem {
        id int
        username str
        active bool
    }
    settings {
        options (array)OptionsItem {
            key str
            value str
        }
    }
    user_items (array)UserItemsItem from=\"user-items\" {
        item_id int from=\"item-id\"
        label str @omitempty
    }
}
";
    assert_eq!(source, expected);
}

#[test]
fn collision_disambiguation_multi_level_ancestors_and_numeric_suffix() {
    let raw_ancestors = r#"{
        "company": {
            "order": {
                "items": [{"id": 1}]
            }
        },
        "store": {
            "order": {
                "items": [{"sku": "SKU-1"}]
            }
        },
        "warehouse": {
            "order": {
                "items": [{"qty": 50}]
            }
        }
    }"#;
    let (src_ancestors, _) = assert_valid_kdl(raw_ancestors, "JsonResponse");
    let expected_ancestors = "\
json JsonResponse {
    company {
        order {
            items (array)ItemsItem {
                id int
            }
        }
    }
    store {
        order {
            items (array)OrderItemsItem {
                sku str
            }
        }
    }
    warehouse {
        order {
            items (array)WarehouseOrderItemsItem {
                qty int
            }
        }
    }
}
";
    assert_eq!(src_ancestors, expected_ancestors);

    let raw_suffix = r#"{
        "items": [{"id": 1}],
        "profile_items": [{"id": 2}],
        "profile": {
            "items": [{"name": "a"}]
        }
    }"#;
    let (src_suffix, _) = assert_valid_kdl(raw_suffix, "JsonResponse");
    let expected_suffix = "\
json JsonResponse {
    items (array)ItemsItem {
        id int
    }
    profile_items (array)ProfileItemsItem {
        id int
    }
    profile {
        items (array)ProfileItemsItem2 {
            name str
        }
    }
}
";
    assert_eq!(src_suffix, expected_suffix);

    // Top-level array collision with root schema name
    let (src_root_arr, _) = assert_valid_kdl(r#"[{"json_response": [{"id": 1}]}]"#, "JsonResponse");
    assert_eq!(
        src_root_arr,
        "\
(array)json JsonResponseItem {
    json_response (array)JsonResponseItem2 {
        id int
    }
}
"
    );

    // Top-level object collision with root name
    let (src_root_obj, _) = assert_valid_kdl(r#"{"custom": [{"id": 1}]}"#, "CustomItem");
    assert_eq!(
        src_root_obj,
        "\
json CustomItem {
    custom (array)CustomItem2 {
        id int
    }
}
"
    );
}

#[test]
fn recursive_nested_objects_and_deep_arrays_merging() {
    let raw_partial_blocks = r#"[
        {
            "user": {
                "profile": {"display_name": "Alice", "avatar": "a.png"}
            }
        },
        {
            "user": {
                "profile": {"display_name": "Bob"}
            }
        },
        {
            "user": {}
        },
        {}
    ]"#;
    let (src_partial, _) = assert_valid_kdl(raw_partial_blocks, "JsonResponse");
    assert_eq!(
        src_partial,
        "\
(array)json JsonResponseItem {
    user @omitempty {
        profile @omitempty {
            display_name str
            avatar str @omitempty
        }
    }
}
"
    );

    let raw_deep = r#"[
        {
            "company": "Acme",
            "departments": [
                {
                    "name": "Engineering",
                    "teams": [
                        {"lead": "Alice", "size": 10}
                    ]
                }
            ]
        },
        {
            "company": "Globex",
            "departments": [
                {
                    "name": "Design",
                    "budget": 20000,
                    "teams": [
                        {"lead": "Bob", "remote": true},
                        {"lead": "Charlie", "size": 5}
                    ]
                },
                {
                    "name": "Sales",
                    "teams": [
                        {"lead": "David"}
                    ]
                }
            ]
        }
    ]"#;
    let (src_deep, _) = assert_valid_kdl(raw_deep, "JsonResponse");
    assert_eq!(
        src_deep,
        "\
(array)json JsonResponseItem {
    company str
    departments (array)DepartmentsItem {
        name str
        teams (array)TeamsItem {
            lead str
            size int @omitempty
            remote bool @omitempty
        }
        budget int @omitempty
    }
}
"
    );
}

#[test]
fn heterogeneous_types_and_empty_containers_merging() {
    let raw_het = r#"[
        {
            "scalar_mix": 42,
            "obj_or_scalar": {"foo": "bar"},
            "obj_or_arr": {"key": 1},
            "arr_or_scalar": ["item"]
        },
        {
            "scalar_mix": "forty-two",
            "obj_or_scalar": "plain text",
            "obj_or_arr": [1, 2],
            "arr_or_scalar": 100
        }
    ]"#;
    let (src_het, _) = assert_valid_kdl(raw_het, "JsonResponse");
    assert_eq!(
        src_het,
        "\
(array)json JsonResponseItem {
    scalar_mix @skip // int, str
    obj_or_scalar @skip // object, str
    obj_or_arr @skip // object, array
    arr_or_scalar @skip // array, int
}
"
    );

    let raw_arr_het = r#"[
        {"arr_mix": ["str_val"], "obj_arr_mix": [{"id": 1}]},
        {"arr_mix": [999], "obj_arr_mix": ["string_item"]}
    ]"#;
    let (src_arr_het, _) = assert_valid_kdl(raw_arr_het, "JsonResponse");
    assert!(src_arr_het.contains("arr_mix (array)nil @skip // str, int"));
    assert!(src_arr_het.contains("obj_arr_mix (array)nil @skip // object, str"));

    // Some empty object, some populated, some missing
    let (src_mixed_meta, _) = assert_valid_kdl(
        r#"[{"meta": {}}, {"meta": {"version": 1}}, {}]"#,
        "JsonResponse",
    );
    assert_eq!(
        src_mixed_meta,
        "\
(array)json JsonResponseItem {
    meta @omitempty {
        version int @omitempty
    }
}
"
    );

    // Array of empty objects vs mixed empty and populated objects
    let (src_arr_empty, _) =
        assert_valid_kdl(r#"[{"items": [{}]}, {"items": [{}]}]"#, "JsonResponse");
    assert_eq!(
        src_arr_empty,
        "\
(array)json JsonResponseItem {
    items (array)nil @skip // empty objects
}
"
    );

    let (src_arr_mixed, _) = assert_valid_kdl(
        r#"[{"items": [{}]}, {"items": [{"id": 1}]}]"#,
        "JsonResponse",
    );
    assert_eq!(
        src_arr_mixed,
        "\
(array)json JsonResponseItem {
    items (array)ItemsItem {
        id int @omitempty
    }
}
"
    );

    // Empty and populated primitive arrays
    let (src_pop, _) = assert_valid_kdl(r#"[{"tags": []}, {"tags": ["a", "b"]}]"#, "JsonResponse");
    assert_eq!(
        src_pop,
        "(array)json JsonResponseItem {\n    tags (array)str\n}\n"
    );
}

#[test]
fn invalid_json_and_unsupported_roots_return_errors() {
    let err_syntax = json_to_kdl("{not valid json", "JsonResponse").unwrap_err();
    assert!(matches!(err_syntax, JsonToKdlError::InvalidJson(_)));

    for invalid_root in ["[]", "[1, 2]", "\"scalar\"", "123", "null", "true"] {
        let err = json_to_kdl(invalid_root, "JsonResponse").unwrap_err();
        assert_eq!(err, JsonToKdlError::InvalidRoot);
        assert_eq!(
            err.to_string(),
            "JSON root must be an object or a non-empty array of objects"
        );
    }

    let err_empty_name = json_to_kdl(r#"{"id": 1}"#, "   ").unwrap_err();
    assert_eq!(err_empty_name, JsonToKdlError::EmptySchemaName);
}
