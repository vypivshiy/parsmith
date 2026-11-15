use parsmith::kdl::{KdlDocument, KdlNode, KdlValue, NodeRef, Selector, parse};

const KDL_TEST_DOC: &str = r#"/- kdl-version 2

app "my-service" version="1.0.0" {
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
        route "GET" "/api/health" handler="health.check" auth=#false
        route "GET" "/static/*" handler="static.serve" auth=#false
    }

    plugins {
        plugin "auth" enabled=#true {
            (jwt)secret "hs256" key=(regex)"hs(256|512)"
            expires (i32)3600
        }
        plugin "cache" enabled=#true {
            backend "redis" host="cache.local" port=(u16)6379
        }
        plugin "debug" enabled=#false
    }

    (i32)workers 4
    (i32)timeout 30
    limits max-conn=(u32)1000 max-req=(u32)500
}
"#;

const KDL_QUOTED_SELECTORS_DOC: &str = r#"/- kdl-version 2

"service:web" "frontend" {
    "a>b" "ok" status="ok" {
        "child>item" "v1" {
            "deep+node" "leaf1"
        }
        "child_plain" "v2"
    }
    "c+d" "pending" status="pending" {
        "sub" "v3"
    }
    "x~y" "ok" status="ok"
    "a,b" "special" status="special"
    "a<b" "angle" status="angle"
    "spaced name" "val"
    "has'single" "single_quote"
    "has\"double" "double_quote"
}

"service:api" "backend" {
    "c+d" "api_cd" status="api_cd"
    "isolated" "none"
}

"top>a" "sibling1"
"top+b" "sibling2"
"top~c" "sibling3"
"#;

const KDL_ESCAPED_DOC: &str = r#"/- kdl-version 2

"a>b" 10 key="val1" {
    "c+d" 20 {
        "nested:item" 21
    }
    "e~f" 30
    "g,h" 40
}

">foo" 50
"+bar" 60
"~baz" 70
",qux" 80
"ns:service" 90 active=#true {
    "leaf" 91
}
"complex>a+b~c,d:e" 100
"has\\backslash" 110
"has\"quote" 120
"end>" 130
"hello world" 140
"#;

const KDL_TYPED_DOC: &str = r#"/- kdl-version 2

("my/custom:type")service "srv_quoted" port=("u:16")8080 active=#true {
    ("child/nested:type")worker "w1"
    (nested:type2)worker "w2"
}

(custom:v1)service "srv_unquoted" port=(u:16)8081 active=#false {
    ("child/nested:type")worker "w3"
}

("ns:type")app "app1" ("u:16")100 (custom:v1)200 {
    (custom:v1)setting "s1"
}

("a>b")node_gt "gt_node"
("a+b")node_plus "plus_node"
("a~b")node_tilde "tilde_node"
("a,b")node_comma "comma_node"
("spaced type")node_space "space_node"

untyped_node "no_type" port=8082
"#;

const KDL_ATTR_KEYS_DOC: &str = r#"/- kdl-version 2

service "my-app" "backend" "0"="prop_zero" "1"="prop_one" "app.name"="api-gateway" "foo:bar"="baz" "a>b"="gt_val" "c+d"="plus_val" "x~y"="tilde_val" "a,b"="comma_val" "nested[key]"="bracket_val" "spaced key"="space_val" "has'single"="single_quote_val" "has\"double"="double_quote_val" port=(u16)8080 {
    route "/users" "GET" "api.version"="v1" auth=#true "0"="route_zero"
    route "/items" "POST" "api.version"="v2" auth=#false "0"="item_zero"
    config "db.host"="localhost" "foo.bar"="cfg_val"
}

service "secondary" "frontend" "0"="sec_zero" "app.name"="web-client" "foo:bar"="qux" {
    route "/dashboard" "GET" "api.version"="v1" auth=#true
}

worker "worker-1" "active" 100 "0"="w0" "1"="w1" "2"="w2"
"#;

const KDL_LITERALS_DOC: &str = r#"/- kdl-version 2

node "neg_int" -5 offset=-5 step=-10
node "neg_float" -0.5 delta=-0.5 ratio=-3.14
node "pos_num" 5 offset=5 delta=0.5
node "typed_neg" (f64)-0.5 offset=(i32)-5
node "null_val" #null val=#null
node "inf_val" #inf val=#inf
node "ninf_val" #-inf val=#-inf
node "nan_val" #nan val=#nan
"#;

fn test_doc() -> KdlDocument {
    parse(KDL_TEST_DOC).expect("KDL_TEST_DOC should parse")
}

fn quoted_doc() -> KdlDocument {
    parse(KDL_QUOTED_SELECTORS_DOC).expect("KDL_QUOTED_SELECTORS_DOC should parse")
}

fn escaped_doc() -> KdlDocument {
    parse(KDL_ESCAPED_DOC).expect("KDL_ESCAPED_DOC should parse")
}

fn typed_doc() -> KdlDocument {
    parse(KDL_TYPED_DOC).expect("KDL_TYPED_DOC should parse")
}

fn attr_doc() -> KdlDocument {
    parse(KDL_ATTR_KEYS_DOC).expect("KDL_ATTR_KEYS_DOC should parse")
}

fn lit_doc() -> KdlDocument {
    parse(KDL_LITERALS_DOC).expect("KDL_LITERALS_DOC should parse")
}

fn names<'a>(results: &[NodeRef<'a>]) -> Vec<&'a str> {
    results.iter().map(|n| n.name()).collect()
}

fn first_str_args<'a>(results: &[NodeRef<'a>]) -> Vec<&'a str> {
    results
        .iter()
        .map(|n| n.arg(0).unwrap().as_str().unwrap())
        .collect()
}

fn first_int_args(results: &[NodeRef<'_>]) -> Vec<i128> {
    results
        .iter()
        .map(|n| n.arg(0).unwrap().as_int().unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// Node by name and wildcard
// ---------------------------------------------------------------------------

#[test]
fn test_node_selector() {
    let doc = test_doc();

    let r = doc.select("app").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select("server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("*:root").unwrap();
    assert_eq!(names(&r), vec!["app"]);
}

// ---------------------------------------------------------------------------
// Type annotation on node
// ---------------------------------------------------------------------------

#[test]
fn test_type_annotation() {
    let doc = test_doc();

    let r = doc.select("(network)").unwrap();
    assert_eq!(names(&r), vec!["server", "server"]);

    let r = doc.select("(network)server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("(i32)").unwrap();
    assert_eq!(names(&r), vec!["workers", "timeout"]);

    let r = doc.select("(i32)workers").unwrap();
    assert_eq!(names(&r), vec!["workers"]);

    let r = doc.select("(jwt)").unwrap();
    assert_eq!(names(&r), vec!["secret"]);

    let r = doc.select("(jwt)secret").unwrap();
    assert_eq!(first_str_args(&r), vec!["hs256"]);
}

// ---------------------------------------------------------------------------
// Properties — basic
// ---------------------------------------------------------------------------

#[test]
fn test_property_basic() {
    let doc = test_doc();

    let r = doc.select("server[tls]").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("server[tls=#true]").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r = doc.select("server[tls=#false]").unwrap();
    assert_eq!(first_str_args(&r), vec!["replica"]);

    let r = doc.select("server[port=8080]").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r = doc.select("route[auth=#true]").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST"]);

    let r = doc.select(r#"route[handler^="users"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST"]);

    let r = doc.select(r#"route[handler$="check"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select(r#"route[handler~="serve"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select("app[version]").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select(r#"app[version="1.0.0"]"#).unwrap();
    assert_eq!(names(&r), vec!["app"]);
}

// ---------------------------------------------------------------------------
// Properties — type annotation on value
// ---------------------------------------------------------------------------

#[test]
fn test_property_type_annotation() {
    let doc = test_doc();

    let r = doc.select("[(u16)port]").unwrap();
    assert_eq!(names(&r), vec!["backend"]);

    let r = doc.select("backend[(u16)port]").unwrap();
    assert_eq!(names(&r), vec!["backend"]);

    let r = doc.select("backend[(u16)port=6379]").unwrap();
    assert_eq!(names(&r), vec!["backend"]);

    let r = doc.select("[(u32)max-conn]").unwrap();
    assert_eq!(names(&r), vec!["limits"]);

    let r = doc.select("limits[(u32)max-conn]").unwrap();
    assert_eq!(names(&r), vec!["limits"]);

    let r = doc.select("limits[(u32)max-conn=1000]").unwrap();
    assert_eq!(names(&r), vec!["limits"]);

    let r = doc.select("limits[(u32)max-req=500]").unwrap();
    assert_eq!(names(&r), vec!["limits"]);

    let r = doc.select("[(regex)key]").unwrap();
    assert_eq!(names(&r), vec!["secret"]);

    let r = doc.select("secret[(regex)key]").unwrap();
    assert_eq!(names(&r), vec!["secret"]);

    let r = doc.select(r#"secret[(regex)key~="hs"]"#).unwrap();
    assert_eq!(names(&r), vec!["secret"]);

    let r = doc.select("backend[(u32)port]").unwrap();
    assert!(r.is_empty());

    let r = doc.select("secret[(jwt)key]").unwrap();
    assert!(r.is_empty());
}

// ---------------------------------------------------------------------------
// Arguments — basic
// ---------------------------------------------------------------------------

#[test]
fn test_argument_basic() {
    let doc = test_doc();

    let r = doc.select("server[0]").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select(r#"server[0="primary"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r = doc.select(r#"server[0="replica"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["replica"]);

    let r = doc.select(r#"route[0="GET"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET", "GET"]);

    let r = doc.select(r#"route[1="/api/users"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST"]);

    let r = doc.select(r#"route[1^="/api"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST", "GET"]);

    let r = doc.select(r#"route[1^="/static"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select(r#"route[1$="/users"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST"]);

    let r = doc.select(r#"route[1~="health"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select(r#"route[*="POST"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["POST"]);

    let r = doc.select("plugin[0]").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache", "debug"]);

    let r = doc.select("plugin[1]").unwrap();
    assert!(r.is_empty());
}

// ---------------------------------------------------------------------------
// Arguments — type annotation
// ---------------------------------------------------------------------------

#[test]
fn test_argument_type_annotation() {
    let doc = test_doc();

    let r = doc.select("expires[(i32)0]").unwrap();
    assert_eq!(names(&r), vec!["expires"]);

    let r = doc.select("expires[(i32)0=3600]").unwrap();
    assert_eq!(names(&r), vec!["expires"]);

    let r = doc.select("expires[(u32)0]").unwrap();
    assert!(r.is_empty());

    let r = doc.select("(i32)workers[(i32)0]").unwrap();
    assert!(r.is_empty());

    let r = doc.select("*[(i32)0]").unwrap();
    assert_eq!(names(&r), vec!["expires"]);
}

// ---------------------------------------------------------------------------
// Combinators
// ---------------------------------------------------------------------------

#[test]
fn test_combinators() {
    let doc = test_doc();

    let r = doc.select("app > server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("app > router > route").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST", "GET", "GET"]);

    let r = doc.select("app > route").unwrap();
    assert!(r.is_empty());

    let r = doc.select("plugins > plugin").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache", "debug"]);

    let r = doc.select("plugin > backend").unwrap();
    assert_eq!(names(&r), vec!["backend"]);

    let r = doc.select("(network)server[tls=#true] > host").unwrap();
    assert_eq!(first_str_args(&r), vec!["localhost", "127.0.0.1"]);

    let r = doc.select("(network)server[tls=#false] > host").unwrap();
    assert_eq!(first_str_args(&r), vec!["replica.local"]);

    let r = doc.select("app server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("app route").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST", "GET", "GET"]);

    let r = doc
        .select("plugins plugin[enabled=#true] > backend")
        .unwrap();
    assert_eq!(names(&r), vec!["backend"]);
}

// ---------------------------------------------------------------------------
// Siblings
// ---------------------------------------------------------------------------

#[test]
fn test_siblings() {
    let doc = test_doc();

    let r = doc.select(r#"route[0="GET"] + route"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["POST", "GET"]);

    let r = doc.select(r#"route[0="POST"] + route"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc
        .select(r#"route[0="GET"][1$="/users"] ~ route"#)
        .unwrap();
    assert_eq!(first_str_args(&r), vec!["POST", "GET", "GET"]);

    let r = doc.select(r#"plugin[0="auth"] + plugin"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["cache"]);

    let r = doc.select(r#"plugin[0="auth"] ~ plugin"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["cache", "debug"]);

    let r = doc.select(r#"plugin[0="debug"] + plugin"#).unwrap();
    assert!(r.is_empty());
}

// ---------------------------------------------------------------------------
// Pseudo-classes
// ---------------------------------------------------------------------------

#[test]
fn test_pseudo_classes() {
    let doc = test_doc();

    let r = doc.select("route:first-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select("route:last-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = doc.select("route:nth-child(2)").unwrap();
    assert_eq!(first_str_args(&r), vec!["POST"]);

    let r = doc.select("route:nth-child(2n)").unwrap();
    assert_eq!(first_str_args(&r), vec!["POST", "GET"]);

    let r = doc.select("route:nth-child(2n+1)").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET"]);

    let r = doc.select("plugin:only-child").unwrap();
    assert!(r.is_empty());

    let r = doc.select("host:only-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["replica.local"]);

    let r = doc.select(r#"plugin[0="debug"]:last-child"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["debug"]);

    let r = doc.select(r#"plugin[0="debug"]:empty"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["debug"]);

    let r = doc.select("backend:empty").unwrap();
    assert_eq!(names(&r), vec!["backend"]);

    let r = doc.select("expires:empty").unwrap();
    assert_eq!(names(&r), vec!["expires"]);

    let r = doc.select("server:first-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r = doc.select("(i32):last-child").unwrap();
    assert_eq!(names(&r), vec!["timeout"]);

    let r = doc.select("app:root").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select("server:root").unwrap();
    assert!(r.is_empty());
}

// ---------------------------------------------------------------------------
// Combined — regressions
// ---------------------------------------------------------------------------

#[test]
fn test_combined() {
    let doc = test_doc();

    let r = doc.select("(network)server[tls=#true] host").unwrap();
    assert_eq!(first_str_args(&r), vec!["localhost", "127.0.0.1"]);

    let r = doc.select("app > *[(u32)max-conn]").unwrap();
    assert_eq!(names(&r), vec!["limits"]);

    let r = doc.select("*[(i32)0]:empty").unwrap();
    assert_eq!(names(&r), vec!["expires"]);

    let r = doc.select(r#"route[auth=#false][0="GET"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET"]);

    let r = doc.select("(jwt)secret[(regex)key]").unwrap();
    assert_eq!(first_str_args(&r), vec!["hs256"]);

    let r = doc.select("plugins plugin[(u16)port]").unwrap();
    assert!(r.is_empty());

    let r = doc.select("app backend[(u16)port]").unwrap();
    assert_eq!(names(&r), vec!["backend"]);
}

// ---------------------------------------------------------------------------
// :not()
// ---------------------------------------------------------------------------

#[test]
fn test_not() {
    let doc = test_doc();

    let r = doc.select("server:not([tls])").unwrap();
    assert!(r.is_empty());

    let r = doc.select("server:not([port=8080])").unwrap();
    assert_eq!(first_str_args(&r), vec!["replica"]);

    let r = doc.select("route:not([auth=#true])").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET"]);

    let r = doc.select("plugin:not(:empty)").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache"]);

    let r = doc.select("*:not(app):root").unwrap();
    assert!(r.is_empty());

    let r = doc.select("*:not((i32))").unwrap();
    let r_names = names(&r);
    assert!(!r_names.contains(&"workers"));
    assert_eq!(r_names.iter().filter(|&&n| n == "timeout").count(), 2);

    let r = doc.select(r#"route:not([0="GET"])"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["POST"]);

    let r = doc.select("app > *:not(server, router)").unwrap();
    let r_names = names(&r);
    assert!(!r_names.contains(&"server"));
    assert!(!r_names.contains(&"router"));

    let r = doc.select("plugin:not(:empty, [enabled=#false])").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache"]);
}

// ---------------------------------------------------------------------------
// :has()
// ---------------------------------------------------------------------------

#[test]
fn test_has() {
    let doc = test_doc();

    let r = doc.select("plugin:has(backend)").unwrap();
    assert_eq!(first_str_args(&r), vec!["cache"]);

    let r = doc.select("server:has(host)").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("app:has(> router)").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select("plugin:has(> secret)").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth"]);

    let r = doc.select("plugin:has(> secret[(regex)key])").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth"]);

    let r = doc.select("server:not(:has(host))").unwrap();
    assert!(r.is_empty());

    let r = doc.select("app:has(backend)").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select("plugin:has(> expires)").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth"]);

    let r = doc.select("server:has(timeout)").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("router:has(> route[auth=#true])").unwrap();
    assert_eq!(names(&r), vec!["router"]);

    let r = doc.select("server:has(host, timeout)").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("app:has(> server, > router)").unwrap();
    assert_eq!(names(&r), vec!["app"]);

    let r = doc.select("plugin:has(> secret, backend)").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache"]);
}

// ---------------------------------------------------------------------------
// Comma (union)
// ---------------------------------------------------------------------------

#[test]
fn test_comma() {
    let doc = test_doc();

    let r = doc.select("app, router").unwrap();
    assert_eq!(names(&r), vec!["app", "router"]);

    let r = doc.select("host, backend, expires").unwrap();
    assert_eq!(
        names(&r),
        vec!["host", "host", "host", "expires", "backend"]
    );

    let r = doc.select("server, server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select("server, (network)server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = doc.select(r#"app, route[0="POST"]"#).unwrap();
    assert_eq!(names(&r), vec!["app", "route"]);

    let r = doc.select("app > server, app > router").unwrap();
    assert_eq!(names(&r), vec!["server", "server", "router"]);

    let r = doc.select("*:root, backend:empty").unwrap();
    assert_eq!(names(&r), vec!["app", "backend"]);

    let r = doc
        .select("plugin:not(:empty), plugin:has(> secret)")
        .unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache"]);
}

// ---------------------------------------------------------------------------
// select_first / select_one
// ---------------------------------------------------------------------------

#[test]
fn test_select_first_and_select_one() {
    let doc = test_doc();

    let node = doc.select_first("server").unwrap().unwrap();
    assert_eq!(node.arg(0).unwrap().as_str(), Some("primary"));
    assert_eq!(doc.select_one("server").unwrap(), Some(node));

    assert!(doc.select_first("nonexistent").unwrap().is_none());

    let node = doc.select_first("*:root").unwrap().unwrap();
    assert_eq!(node.name(), "app");

    let node = doc.select_first("server[tls=#true]").unwrap().unwrap();
    assert_eq!(node.arg(0).unwrap().as_str(), Some("primary"));

    let node = doc.select_first("backend, host").unwrap().unwrap();
    assert_eq!(node.name(), "host");

    let node = doc.select_first("app > server > host").unwrap().unwrap();
    assert_eq!(node.arg(0).unwrap().as_str(), Some("localhost"));

    assert!(doc.select_first("nonexist1, nonexist2").unwrap().is_none());
}

// ---------------------------------------------------------------------------
// NodeRef.select / NodeRef.select_first (Subtree scoping)
// ---------------------------------------------------------------------------

#[test]
fn test_node_select_subtree() {
    let doc = test_doc();
    let app = doc.select_first("app").unwrap().unwrap();

    let r = app.select("server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = app.select("host").unwrap();
    assert_eq!(
        first_str_args(&r),
        vec!["localhost", "127.0.0.1", "replica.local"]
    );

    let r = app.select("server > host").unwrap();
    assert_eq!(
        first_str_args(&r),
        vec!["localhost", "127.0.0.1", "replica.local"]
    );

    let r = app.select("router > route").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "POST", "GET", "GET"]);

    let r = app.select("(network)server").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary", "replica"]);

    let r = app.select("server[tls=#true]").unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r = app.select(r#"route[0="POST"]"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["POST"]);

    let router = doc.select_first("router").unwrap().unwrap();
    let r = router.select("route:first-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = router.select("route:last-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET"]);

    let r = router.select("route:nth-child(2)").unwrap();
    assert_eq!(first_str_args(&r), vec!["POST"]);

    let debug = doc.select_first(r#"plugin[0="debug"]"#).unwrap().unwrap();
    let r = debug.select("*:empty").unwrap();
    assert!(r.is_empty());

    let replica = doc.select_first(r#"server[0="replica"]"#).unwrap().unwrap();
    let r = replica.select("host:only-child").unwrap();
    assert_eq!(first_str_args(&r), vec!["replica.local"]);

    let plugins = doc.select_first("plugins").unwrap().unwrap();
    let r = plugins.select("plugin:has(> backend)").unwrap();
    assert_eq!(first_str_args(&r), vec!["cache"]);

    let r = router.select("route:not([auth=#true])").unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET"]);

    let r = router.select(r#"route[0="GET"] + route"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["POST", "GET"]);

    let r = router.select(r#"route[0="POST"] ~ route"#).unwrap();
    assert_eq!(first_str_args(&r), vec!["GET", "GET"]);

    // :root never matches inside a node subtree
    let r = app.select("*:root").unwrap();
    assert!(r.is_empty());

    let r = app.select("*").unwrap();
    assert!(!r.contains(&app));
    assert!(!r.is_empty());

    assert!(debug.select("*").unwrap().is_empty());

    let r = app.select("host, backend").unwrap();
    assert_eq!(names(&r), vec!["host", "host", "host", "backend"]);

    let s1 = app.select_first("server").unwrap().unwrap();
    assert_eq!(s1.arg(0).unwrap().as_str(), Some("primary"));
    assert_eq!(app.select_one("server").unwrap(), Some(s1));

    assert!(debug.select_first("anything").unwrap().is_none());

    let primary = doc.select_first(r#"server[0="primary"]"#).unwrap().unwrap();
    let r = primary.select("host").unwrap();
    assert_eq!(first_str_args(&r), vec!["localhost", "127.0.0.1"]);
}

// ---------------------------------------------------------------------------
// Quoted node selectors and descendant combinators
// ---------------------------------------------------------------------------

#[test]
fn test_quoted_node_selectors() {
    let qdoc = quoted_doc();
    let doc = test_doc();

    let r_double = qdoc.select(r#""service:web""#).unwrap();
    let r_single = qdoc.select("'service:web'").unwrap();
    assert_eq!(first_str_args(&r_double), vec!["frontend"]);
    assert_eq!(first_str_args(&r_single), vec!["frontend"]);

    let r_double_api = qdoc.select(r#""service:api""#).unwrap();
    let r_single_api = qdoc.select("'service:api'").unwrap();
    assert_eq!(first_str_args(&r_double_api), vec!["backend"]);
    assert_eq!(first_str_args(&r_single_api), vec!["backend"]);

    // Standard unquoted identifiers also match when quoted
    assert_eq!(names(&doc.select(r#""app""#).unwrap()), vec!["app"]);
    assert_eq!(names(&doc.select("'app'").unwrap()), vec!["app"]);
    assert_eq!(
        first_str_args(&doc.select(r#""server""#).unwrap()),
        vec!["primary", "replica"]
    );
    assert_eq!(
        first_str_args(&doc.select("'server'").unwrap()),
        vec!["primary", "replica"]
    );

    // Special characters in node names
    assert_eq!(
        first_str_args(&qdoc.select(r#""a>b""#).unwrap()),
        vec!["ok"]
    );
    assert_eq!(first_str_args(&qdoc.select("'a>b'").unwrap()), vec!["ok"]);
    assert_eq!(
        first_str_args(&qdoc.select(r#""c+d""#).unwrap()),
        vec!["pending", "api_cd"]
    );
    assert_eq!(
        first_str_args(&qdoc.select("'c+d'").unwrap()),
        vec!["pending", "api_cd"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""x~y""#).unwrap()),
        vec!["ok"]
    );
    assert_eq!(first_str_args(&qdoc.select("'x~y'").unwrap()), vec!["ok"]);
    assert_eq!(
        first_str_args(&qdoc.select(r#""a,b""#).unwrap()),
        vec!["special"]
    );
    assert_eq!(
        first_str_args(&qdoc.select("'a,b'").unwrap()),
        vec!["special"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""a<b""#).unwrap()),
        vec!["angle"]
    );
    assert_eq!(
        first_str_args(&qdoc.select("'a<b'").unwrap()),
        vec!["angle"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""spaced name""#).unwrap()),
        vec!["val"]
    );
    assert_eq!(
        first_str_args(&qdoc.select("'spaced name'").unwrap()),
        vec!["val"]
    );

    // Nested quotes in node names
    assert_eq!(
        first_str_args(&qdoc.select(r#""has'single""#).unwrap()),
        vec!["single_quote"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#"'has"double'"#).unwrap()),
        vec!["double_quote"]
    );

    // Combinators and selector lists with quotes
    assert_eq!(
        first_str_args(&qdoc.select(r#""service:web" > "a>b""#).unwrap()),
        vec!["ok"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""service:web">"a>b""#).unwrap()),
        vec!["ok"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""a>b" + "c+d""#).unwrap()),
        vec!["pending"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""a>b" ~ "x~y""#).unwrap()),
        vec!["ok"]
    );
    let r1 = qdoc.select(r#""a>b", "x~y""#).unwrap();
    assert_eq!(first_str_args(&r1), vec!["ok", "ok"]);
    let r2 = qdoc.select(r#""a,b""#).unwrap();
    assert_eq!(names(&r2), vec!["a,b"]);
    assert_eq!(first_str_args(&r2), vec!["special"]);

    // Pseudo-classes with quotes
    assert_eq!(
        first_str_args(&qdoc.select(r#""service:web":not("service:api")"#).unwrap()),
        vec!["frontend"]
    );
    assert!(
        qdoc.select(r#""service:api":not("service:api")"#)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""service:web":has("child>item")"#).unwrap()),
        vec!["frontend"]
    );
    assert_eq!(
        first_str_args(&qdoc.select(r#""service:web":has(> "a>b")"#).unwrap()),
        vec!["frontend"]
    );
    assert!(
        qdoc.select(r#""service:web":has(> "child>item")"#)
            .unwrap()
            .is_empty()
    );

    // Public API with quotes
    let node = qdoc.select_first(r#""service:web""#).unwrap().unwrap();
    assert_eq!(node.name(), "service:web");
    assert!(qdoc.select_first(r#""nonexistent""#).unwrap().is_none());
    assert!(node.matches(r#""service:web""#));
    assert!(node.matches("'service:web'"));
    assert!(!node.matches(r#""service:api""#));
    assert_eq!(
        first_str_args(&node.select(r#""a>b""#).unwrap()),
        vec!["ok"]
    );
    assert!(node.select_first(r#""child>item""#).unwrap().is_some());

    // Unterminated quotes raise SelectorError
    for pattern in [
        "\"unterminated",
        "'unterminated",
        "\"escaped\\\"",
        "'escaped\\'",
    ] {
        let err = qdoc.select(pattern).unwrap_err();
        assert!(err.is_selector_error());
        assert!(
            err.message().contains("Unterminated string"),
            "unexpected message for {pattern:?}: {}",
            err.message()
        );
    }
}

// ---------------------------------------------------------------------------
// Backslash-escaped identifiers
// ---------------------------------------------------------------------------

#[test]
fn test_escaped_identifiers() {
    let doc = escaped_doc();

    let cases: [(&str, &str, i128); 14] = [
        (r"a\>b", "a>b", 10),
        (r"\>foo", ">foo", 50),
        (r"end\>", "end>", 130),
        (r"c\+d", "c+d", 20),
        (r"\+bar", "+bar", 60),
        (r"e\~f", "e~f", 30),
        (r"\~baz", "~baz", 70),
        (r"g\,h", "g,h", 40),
        (r"\,qux", ",qux", 80),
        (r"ns\:service", "ns:service", 90),
        (r"complex\>a\+b\~c\,d\:e", "complex>a+b~c,d:e", 100),
        (r"has\\backslash", r"has\backslash", 110),
        (r#"has\"quote"#, "has\"quote", 120),
        (r"hello\ world", "hello world", 140),
    ];

    for (pattern, expected_name, expected_first_arg) in cases {
        let r = doc.select(pattern).unwrap();
        assert_eq!(names(&r), vec![expected_name], "pattern: {pattern}");
        assert_eq!(
            first_int_args(&r),
            vec![expected_first_arg],
            "pattern: {pattern}"
        );
    }

    // Escaped with attribute filter
    let r = doc.select(r#"a\>b[key="val1"]"#).unwrap();
    assert_eq!(names(&r), vec!["a>b"]);
    let r2 = doc.select(r"a\>b[0=10]").unwrap();
    assert_eq!(names(&r2), vec!["a>b"]);

    // Escaped in combinators and pseudos
    assert_eq!(
        first_int_args(&doc.select(r"a\>b > c\+d").unwrap()),
        vec![20]
    );
    assert_eq!(first_int_args(&doc.select(r"a\>b>c\+d").unwrap()), vec![20]);
    assert_eq!(
        first_int_args(&doc.select(r"c\+d + e\~f").unwrap()),
        vec![30]
    );
    assert_eq!(first_int_args(&doc.select(r"c\+d+e\~f").unwrap()), vec![30]);
    assert_eq!(
        first_int_args(&doc.select(r"c\+d ~ g\,h").unwrap()),
        vec![40]
    );
    assert_eq!(
        names(&doc.select(r"a\>b > *:not(c\+d)").unwrap()),
        vec!["e~f", "g,h"]
    );
    assert_eq!(
        names(&doc.select(r"a\>b:has(> c\+d)").unwrap()),
        vec!["a>b"]
    );
    assert_eq!(
        names(&doc.select(r"a\>b, \>foo").unwrap()),
        vec!["a>b", ">foo"]
    );
    assert_eq!(
        names(&doc.select(r"g\,h, a\>b").unwrap()),
        vec!["a>b", "g,h"]
    );

    // Public API with escapes
    let node = doc.select_first(r"a\>b").unwrap().unwrap();
    assert_eq!(node.name(), "a>b");
    assert!(node.matches(r"a\>b"));
    assert!(!node.matches(r"\>foo"));
    assert_eq!(names(&node.select(r"c\+d").unwrap()), vec!["c+d"]);
    assert!(node.select_first(r"nested\:item").unwrap().is_some());

    // Unterminated escape
    for pattern in ["foo\\", "\\"] {
        let err = doc.select(pattern).unwrap_err();
        assert!(err.message().contains("Unterminated escape sequence"));
    }
}

// ---------------------------------------------------------------------------
// Disambiguated type annotations & untyped matchers
// ---------------------------------------------------------------------------

#[test]
fn test_disambiguated_type_annotations() {
    let doc = typed_doc();

    // Quoted type on node
    assert_eq!(
        first_str_args(&doc.select(r#"("my/custom:type")service"#).unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select("('my/custom:type')service").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("my/custom:type")"#).unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("my/custom:type")*"#).unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("custom:v1")service"#).unwrap()),
        vec!["srv_unquoted"]
    );
    assert_eq!(
        first_str_args(&doc.select("('custom:v1')service").unwrap()),
        vec!["srv_unquoted"]
    );

    // Escaped type on node
    assert_eq!(
        first_str_args(&doc.select(r"(my\/custom\:type)service").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"(my\/custom\:type)").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"(my\/custom\:type)*").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"(custom\:v1)service").unwrap()),
        vec!["srv_unquoted"]
    );

    // Special characters in type
    assert_eq!(
        first_str_args(&doc.select(r#"("a>b")node_gt"#).unwrap()),
        vec!["gt_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("a+b")node_plus"#).unwrap()),
        vec!["plus_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("a~b")node_tilde"#).unwrap()),
        vec!["tilde_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("a,b")node_comma"#).unwrap()),
        vec!["comma_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"("spaced type")node_space"#).unwrap()),
        vec!["space_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"(a\>b)node_gt").unwrap()),
        vec!["gt_node"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"(spaced\ type)node_space").unwrap()),
        vec!["space_node"]
    );

    // Property type filters
    assert_eq!(
        first_str_args(&doc.select(r#"service[("u:16")port=8080]"#).unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select("service[('u:16')port=8080]").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"service[(u\:16)port=8080]").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[("u:16")port]"#).unwrap()),
        vec!["srv_quoted", "srv_unquoted"]
    );
    assert!(doc.select(r#"service[("i32")port]"#).unwrap().is_empty());
    assert!(
        doc.select(r#"untyped_node[("u:16")port]"#)
            .unwrap()
            .is_empty()
    );

    // Argument type filters
    assert_eq!(
        first_str_args(&doc.select(r#"app[("u:16")1=100]"#).unwrap()),
        vec!["app1"]
    );
    assert_eq!(
        first_str_args(&doc.select("app[('u:16')1=100]").unwrap()),
        vec!["app1"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"app[(u\:16)1=100]").unwrap()),
        vec!["app1"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"app[("custom:v1")2=200]"#).unwrap()),
        vec!["app1"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"app[("u:16")1]"#).unwrap()),
        vec!["app1"]
    );
    assert!(doc.select(r#"app[("i32")1=100]"#).unwrap().is_empty());

    // Wildcard typed argument filters
    assert_eq!(
        first_str_args(&doc.select(r#"app[("u:16")*=100]"#).unwrap()),
        vec!["app1"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"app[("custom:v1")*=200]"#).unwrap()),
        vec!["app1"]
    );
    assert!(doc.select(r#"app[("i32")*=100]"#).unwrap().is_empty());

    // Untyped node and value matchers: `()node`, `()*`, `[()key=val]`, `[()*=val]`
    assert_eq!(
        first_str_args(&doc.select("()untyped_node").unwrap()),
        vec!["no_type"]
    );
    assert!(doc.select("()service").unwrap().is_empty());
    assert_eq!(names(&doc.select("()*").unwrap()), vec!["untyped_node"]);
    assert_eq!(names(&doc.select("()").unwrap()), vec!["untyped_node"]);
    assert_eq!(
        first_str_args(&doc.select("untyped_node[()port=8082]").unwrap()),
        vec!["no_type"]
    );
    assert!(doc.select("service[()port=8080]").unwrap().is_empty());
    assert_eq!(
        first_str_args(&doc.select("service[()active=#true]").unwrap()),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"app[()0="app1"]"#).unwrap()),
        vec!["app1"]
    );
    assert!(doc.select("app[()1=100]").unwrap().is_empty());
    assert_eq!(
        first_str_args(&doc.select(r#"app[()*="app1"]"#).unwrap()),
        vec!["app1"]
    );
    assert!(doc.select("app[()*=100]").unwrap().is_empty());

    // Combinators and pseudos with types
    assert_eq!(
        first_str_args(
            &doc.select(r#"("my/custom:type")service > ("child/nested:type")worker"#)
                .unwrap()
        ),
        vec!["w1"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"("my/custom:type")service:has(("child/nested:type")worker)"#)
                .unwrap()
        ),
        vec!["srv_quoted"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"("my/custom:type")service:not(("custom:v1")service)"#)
                .unwrap()
        ),
        vec!["srv_quoted"]
    );

    // Synthetic standalone KdlNode matching
    let n1 = KdlNode::new("item").with_ty("my/type");
    assert!(n1.matches(r#"("my/type")item"#));
    assert!(n1.matches(r"(my\/type)item"));
    assert!(n1.matches("('my/type')item"));
    assert!(!n1.matches(r#"("other/type")item"#));

    let n2 = KdlNode::new("item").with_ty("(my/type)");
    assert!(n2.matches(r#"("my/type")item"#));
    assert!(n2.matches(r"(my\/type)item"));
    assert!(n2.matches("('my/type')item"));

    let n3 = KdlNode::new("item").with_ty(r#"("my/type")"#);
    assert!(n3.matches(r#"("my/type")item"#));
    assert!(n3.matches(r"(my\/type)item"));
}

#[test]
fn test_disambiguated_type_annotations_errors() {
    let doc = typed_doc();

    for pattern in [
        r#"("unterminated)node"#,
        "('unterminated)node",
        r#"node[("unterminated)port=8080]"#,
    ] {
        let err = doc.select(pattern).unwrap_err();
        assert!(err.message().contains("Unterminated string"));
    }

    for pattern in ["(foo\\", "node[(foo\\"] {
        let err = doc.select(pattern).unwrap_err();
        assert!(err.message().contains("Unterminated escape sequence"));
    }

    let err = doc.select(r"(unterminated\)node").unwrap_err();
    assert!(err.message().contains("Expected RPAREN"));

    for pattern in ["(=)node", "(>)node", "(123)node", "node[(=)port=8080]"] {
        let err = doc.select(pattern).unwrap_err();
        assert!(
            err.message()
                .contains("Expected type annotation identifier or string")
        );
    }

    let err = doc.select(r#"("my/type"node"#).unwrap_err();
    assert!(err.message().contains("Expected RPAREN"));
}

// ---------------------------------------------------------------------------
// Disambiguated property filter keys and positional argument differentiation
// ---------------------------------------------------------------------------

#[test]
fn test_disambiguated_attribute_keys() {
    let doc = attr_doc();

    // Quoted property keys
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"="api-gateway"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select("service['app.name'='api-gateway']").unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service['app.name'="api-gateway"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"='api-gateway']"#).unwrap()),
        vec!["my-app"]
    );

    // Special characters in quoted keys
    assert_eq!(
        first_str_args(&doc.select(r#"service["foo:bar"="baz"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["a>b"="gt_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["c+d"="plus_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["x~y"="tilde_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["a,b"="comma_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"service["nested[key]"="bracket_val"]"#)
                .unwrap()
        ),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["spaced key"="space_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"service["has'single"="single_quote_val"]"#)
                .unwrap()
        ),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"service['has"double'='double_quote_val']"#)
                .unwrap()
        ),
        vec!["my-app"]
    );

    // Operators and existence
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"^="api"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"$="client"]"#).unwrap()),
        vec!["secondary"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"~="gate"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["app.name"]"#).unwrap()),
        vec!["my-app", "secondary"]
    );
    assert!(doc.select(r#"service["nonexistent"]"#).unwrap().is_empty());

    // Escaped property keys
    assert_eq!(
        first_str_args(&doc.select(r#"service[app\.name="api-gateway"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[foo\:bar="baz"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[a\>b="gt_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[c\+d="plus_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[x\~y="tilde_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[a\,b="comma_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(
            &doc.select(r#"service[nested\[key\]="bracket_val"]"#)
                .unwrap()
        ),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[spaced\ key="space_val"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[\0="prop_zero"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[app\.name^="api"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r"service[app\.name]").unwrap()),
        vec!["my-app", "secondary"]
    );

    // Combinators and pseudos with disambiguated keys
    let r1 = doc
        .select(r#"service["app.name"="api-gateway"] > route["api.version"="v1"]"#)
        .unwrap();
    assert_eq!(first_str_args(&r1), vec!["/users"]);

    let r2 = doc.select(r#"service:not([foo\:bar="qux"])"#).unwrap();
    assert_eq!(first_str_args(&r2), vec!["my-app"]);

    let r3 = doc
        .select(r#"service:has(> route["api.version"="v2"])"#)
        .unwrap();
    assert_eq!(first_str_args(&r3), vec!["my-app"]);

    // NodeRef.matches with disambiguated keys
    let srv = doc.select_first(r#"service[0="my-app"]"#).unwrap().unwrap();
    assert!(srv.matches(r#"service[0="my-app"]"#));
    assert!(!srv.matches(r#"service[0="prop_zero"]"#));
    assert!(srv.matches(r#"service["0"="prop_zero"]"#));
    assert!(!srv.matches(r#"service["0"="my-app"]"#));
    assert!(srv.matches(r#"service["app.name"="api-gateway"]"#));
    assert!(srv.matches(r#"service[app\.name="api-gateway"]"#));
}

#[test]
fn test_positional_argument_vs_numeric_property_key() {
    let doc = attr_doc();

    // Unquoted number matches positional arg 0
    assert_eq!(
        first_str_args(&doc.select(r#"service[0="my-app"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[0="secondary"]"#).unwrap()),
        vec!["secondary"]
    );
    assert!(doc.select(r#"service[0="prop_zero"]"#).unwrap().is_empty());

    // Quoted number matches property with string key
    assert_eq!(
        first_str_args(&doc.select(r#"service["0"="prop_zero"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select("service['0'='prop_zero']").unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["0"="sec_zero"]"#).unwrap()),
        vec!["secondary"]
    );
    assert!(doc.select(r#"service["0"="my-app"]"#).unwrap().is_empty());
    assert!(doc.select("service['0'='my-app']").unwrap().is_empty());

    // Unquoted vs quoted number at index 1
    assert_eq!(
        first_str_args(&doc.select(r#"service[1="backend"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service[1="frontend"]"#).unwrap()),
        vec!["secondary"]
    );
    assert_eq!(
        first_str_args(&doc.select(r#"service["1"="prop_one"]"#).unwrap()),
        vec!["my-app"]
    );
    assert_eq!(
        first_str_args(&doc.select("service['1'='prop_one']").unwrap()),
        vec!["my-app"]
    );
    assert!(doc.select(r#"service[1="prop_one"]"#).unwrap().is_empty());
    assert!(doc.select(r#"service["1"="backend"]"#).unwrap().is_empty());

    // Existence unquoted vs quoted number
    assert_eq!(doc.select("worker[0]").unwrap().len(), 1);
    assert_eq!(doc.select("worker[1]").unwrap().len(), 1);
    assert_eq!(doc.select("worker[2]").unwrap().len(), 1);
    assert_eq!(doc.select("worker[3]").unwrap().len(), 0);

    assert_eq!(doc.select(r#"worker["0"]"#).unwrap().len(), 1);
    assert_eq!(doc.select("worker['0']").unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker["1"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker["2"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker["3"]"#).unwrap().len(), 0);

    // Operators on numeric keys
    assert_eq!(doc.select(r#"worker["0"^="w"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker["0"$="0"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker["0"~="w"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker[1^="act"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker[1$="ive"]"#).unwrap().len(), 1);
    assert_eq!(doc.select(r#"worker[1~="cti"]"#).unwrap().len(), 1);
}

#[test]
fn test_disambiguated_attribute_keys_errors() {
    let doc = attr_doc();

    for pattern in [r#"service["unterminated]"#, "service['unterminated]"] {
        let err = doc.select(pattern).unwrap_err();
        assert!(err.message().contains("Unterminated string"));
    }

    let err = doc.select("service[app\\").unwrap_err();
    assert!(err.message().contains("Unterminated escape sequence"));

    let err = doc.select(r#"service[1.5="val"]"#).unwrap_err();
    assert!(err.message().contains("Expected integer index"));

    let err = doc.select(r#"service[#true="val"]"#).unwrap_err();
    assert!(err.message().contains("Expected key name or index"));
}

// ---------------------------------------------------------------------------
// :has() complex selectors, boundary enforcement & leading sibling combinators
// ---------------------------------------------------------------------------

#[test]
fn test_has_complex_and_siblings() {
    let doc = test_doc();
    let qdoc = quoted_doc();

    // Leading child chain
    assert_eq!(
        names(&doc.select("app:has(> router > route)").unwrap()),
        vec!["app"]
    );
    assert_eq!(
        names(
            &doc.select(r#"app:has(> router > route[0="POST"])"#)
                .unwrap()
        ),
        vec!["app"]
    );
    assert!(
        doc.select("app:has(> router > nonexistent)")
            .unwrap()
            .is_empty()
    );
    assert!(doc.select("app:has(> server > route)").unwrap().is_empty());

    // Descendant complex chain
    assert_eq!(
        names(&doc.select("app:has(server > host)").unwrap()),
        vec!["app"]
    );
    assert_eq!(
        names(&doc.select("app:has(plugin > backend)").unwrap()),
        vec!["app"]
    );
    assert_eq!(
        names(
            &doc.select(r#"plugins:has(plugin[0="cache"] > backend)"#)
                .unwrap()
        ),
        vec!["plugins"]
    );
    assert!(
        doc.select(r#"plugins:has(plugin[0="auth"] > backend)"#)
            .unwrap()
            .is_empty()
    );

    // Subtree boundary excludes self and ancestors
    assert!(doc.select("server:has(server > host)").unwrap().is_empty());
    assert!(
        doc.select("server:has(app > server > host)")
            .unwrap()
            .is_empty()
    );
    assert!(
        doc.select("router:has(app > router > route)")
            .unwrap()
            .is_empty()
    );

    // Nested same-name boundary
    let nested_doc = parse(
        r#"
        box "outer" {
            box "inner" {
                item "leaf"
            }
        }
        "#,
    )
    .unwrap();
    assert_eq!(
        first_str_args(&nested_doc.select("box:has(box > item)").unwrap()),
        vec!["outer"]
    );
    assert_eq!(
        first_str_args(&nested_doc.select("box:has(> box > item)").unwrap()),
        vec!["outer"]
    );
    assert_eq!(
        first_str_args(&nested_doc.select("box:has(item)").unwrap()),
        vec!["outer", "inner"]
    );

    // Internal sibling combinators
    assert_eq!(
        names(
            &doc.select(r#"router:has(route[0="GET"] + route[0="POST"])"#)
                .unwrap()
        ),
        vec!["router"]
    );
    assert_eq!(
        names(
            &doc.select(r#"router:has(> route[0="GET"] + route[0="POST"])"#)
                .unwrap()
        ),
        vec!["router"]
    );
    assert!(
        doc.select(r#"router:has(route[0="POST"] + route[0="POST"])"#)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        names(
            &doc.select(r#"plugins:has(> plugin[0="auth"] ~ plugin[enabled=#false])"#)
                .unwrap()
        ),
        vec!["plugins"]
    );
    assert_eq!(
        first_str_args(&doc.select("server:has(host + host)").unwrap()),
        vec!["primary"]
    );
    assert_eq!(
        first_str_args(&doc.select("server:has(> host + host)").unwrap()),
        vec!["primary"]
    );
    assert_eq!(
        first_str_args(&doc.select("server:has(host + timeout)").unwrap()),
        vec!["primary", "replica"]
    );

    // Leading adjacent sibling
    let r = doc.select(r#"route:has(+ route[0="POST"])"#).unwrap();
    let pairs: Vec<(&str, &str)> = r
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(pairs, vec![("GET", "/api/users")]);

    let r2 = doc.select("plugin:has(+ plugin[enabled=#false])").unwrap();
    assert_eq!(first_str_args(&r2), vec!["cache"]);

    let r3 = doc.select("server:has(+ router)").unwrap();
    assert_eq!(first_str_args(&r3), vec!["replica"]);

    // Leading general sibling
    let r = doc.select("plugin:has(~ plugin[enabled=#false])").unwrap();
    assert_eq!(first_str_args(&r), vec!["auth", "cache"]);

    let r2 = doc.select(r#"route:has(~ route[1="/static/*"])"#).unwrap();
    let paths: Vec<&str> = r2
        .iter()
        .map(|n| n.arg(1).unwrap().as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["/api/users", "/api/users", "/api/health"]);

    // Leading sibling with complex subselector
    let r = doc
        .select(r#"server:has(+ server[0="replica"] > host[0="replica.local"])"#)
        .unwrap();
    assert_eq!(first_str_args(&r), vec!["primary"]);

    let r2 = doc
        .select(r#"server:has(~ plugins > plugin[0="cache"] > backend)"#)
        .unwrap();
    assert_eq!(first_str_args(&r2), vec!["primary", "replica"]);

    let r3 = doc
        .select(r#"plugin:has(+ plugin + plugin[0="debug"])"#)
        .unwrap();
    assert_eq!(first_str_args(&r3), vec!["auth"]);

    // :has on root siblings and NodeRef.matches
    let r_adj = qdoc.select(r#""top>a":has(+ "top+b")"#).unwrap();
    assert_eq!(first_str_args(&r_adj), vec!["sibling1"]);

    let r_gen = qdoc.select(r#""top>a":has(~ "top~c")"#).unwrap();
    assert_eq!(first_str_args(&r_gen), vec!["sibling1"]);

    let top_a = qdoc.select_first(r#""top>a""#).unwrap().unwrap();
    assert!(top_a.matches(r#""top>a":has(+ "top+b")"#));
    assert!(!top_a.matches(r#""top>a":has(+ "top~c")"#));
    assert!(top_a.matches(r#""top>a":has(~ "top~c")"#));
}

// ---------------------------------------------------------------------------
// Negative numbers & KDL 2.0 keyword literals (#null, #inf, #-inf, #nan)
// ---------------------------------------------------------------------------

#[test]
fn test_literals_and_signed_numbers() {
    let doc = lit_doc();

    // Negative int property and arg
    assert_eq!(
        first_str_args(&doc.select("node[offset=-5]").unwrap()),
        vec!["neg_int", "typed_neg"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[step=-10]").unwrap()),
        vec!["neg_int"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=-5]").unwrap()),
        vec!["neg_int"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=-5]").unwrap()),
        vec!["neg_int"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[(i32)offset=-5]").unwrap()),
        vec!["typed_neg"]
    );

    // Negative float property and arg
    assert_eq!(
        first_str_args(&doc.select("node[delta=-0.5]").unwrap()),
        vec!["neg_float"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[ratio=-3.14]").unwrap()),
        vec!["neg_float"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=-0.5]").unwrap()),
        vec!["neg_float", "typed_neg"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=-0.5]").unwrap()),
        vec!["neg_float", "typed_neg"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[(f64)1=-0.5]").unwrap()),
        vec!["typed_neg"]
    );

    // Explicit + number property
    assert_eq!(
        first_str_args(&doc.select("node[offset=+5]").unwrap()),
        vec!["pos_num"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[delta=+0.5]").unwrap()),
        vec!["pos_num"]
    );

    // #null literal
    assert_eq!(
        first_str_args(&doc.select("node[val=#null]").unwrap()),
        vec!["null_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=#null]").unwrap()),
        vec!["null_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=#null]").unwrap()),
        vec!["null_val"]
    );

    // #inf and #-inf literals
    assert_eq!(
        first_str_args(&doc.select("node[val=#inf]").unwrap()),
        vec!["inf_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=#inf]").unwrap()),
        vec!["inf_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=#inf]").unwrap()),
        vec!["inf_val"]
    );

    assert_eq!(
        first_str_args(&doc.select("node[val=#-inf]").unwrap()),
        vec!["ninf_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=#-inf]").unwrap()),
        vec!["ninf_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=#-inf]").unwrap()),
        vec!["ninf_val"]
    );

    // #nan literal
    assert_eq!(
        first_str_args(&doc.select("node[val=#nan]").unwrap()),
        vec!["nan_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[1=#nan]").unwrap()),
        vec!["nan_val"]
    );
    assert_eq!(
        first_str_args(&doc.select("node[*=#nan]").unwrap()),
        vec!["nan_val"]
    );

    // Invalid hash keyword raises error
    for pattern in [
        "node[val=#invalid]",
        "node[val=#nullish]",
        "node[val=#infinity]",
    ] {
        assert!(doc.select(pattern).is_err());
    }
}

// ---------------------------------------------------------------------------
// Extended :nth-child() expressions (odd, even, negative step)
// ---------------------------------------------------------------------------

#[test]
fn test_nth_child_extended() {
    let doc = test_doc();

    let r = doc.select("route:nth-child(odd)").unwrap();
    let second_args: Vec<&str> = r
        .iter()
        .map(|n| n.arg(1).unwrap().as_str().unwrap())
        .collect();
    assert_eq!(second_args, vec!["/api/users", "/api/health"]);

    let r = doc.select("route:nth-child(even)").unwrap();
    let second_args: Vec<&str> = r
        .iter()
        .map(|n| n.arg(1).unwrap().as_str().unwrap())
        .collect();
    assert_eq!(second_args, vec!["/api/users", "/static/*"]);
    assert_eq!(first_str_args(&r), vec!["POST", "GET"]);

    // -n+b
    let r3 = doc.select("route:nth-child(-n+3)").unwrap();
    let pairs3: Vec<(&str, &str)> = r3
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs3,
        vec![
            ("GET", "/api/users"),
            ("POST", "/api/users"),
            ("GET", "/api/health"),
        ]
    );

    let r2 = doc.select("route:nth-child(-n+2)").unwrap();
    let pairs2: Vec<(&str, &str)> = r2
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(pairs2, vec![("GET", "/api/users"), ("POST", "/api/users")]);

    let r1 = doc.select("route:nth-child(-n+1)").unwrap();
    let pairs1: Vec<(&str, &str)> = r1
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(pairs1, vec![("GET", "/api/users")]);

    // -2n+b
    let r_even_first4 = doc.select("route:nth-child(-2n+4)").unwrap();
    let pairs_even: Vec<(&str, &str)> = r_even_first4
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs_even,
        vec![("POST", "/api/users"), ("GET", "/static/*")]
    );

    let r_odd_first3 = doc.select("route:nth-child(-2n+3)").unwrap();
    let pairs_odd: Vec<(&str, &str)> = r_odd_first3
        .iter()
        .map(|n| {
            (
                n.arg(0).unwrap().as_str().unwrap(),
                n.arg(1).unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs_odd,
        vec![("GET", "/api/users"), ("GET", "/api/health")]
    );

    // Spaced and negative offsets
    assert_eq!(
        doc.select("route:nth-child(-n + 3)").unwrap(),
        doc.select("route:nth-child(-n+3)").unwrap()
    );
    assert_eq!(
        doc.select("route:nth-child(2n-1)").unwrap(),
        doc.select("route:nth-child(odd)").unwrap()
    );
    assert_eq!(
        doc.select("route:nth-child(2n - 1)").unwrap(),
        doc.select("route:nth-child(odd)").unwrap()
    );
    assert!(doc.select("route:nth-child(-n)").unwrap().is_empty());
    assert!(doc.select("route:nth-child(-n-1)").unwrap().is_empty());

    // Invalid :nth-child raises error
    for pattern in [
        "route:nth-child(foo)",
        "route:nth-child(2.5)",
        "route:nth-child(2n 1)",
    ] {
        assert!(doc.select(pattern).is_err(), "expected error for {pattern}");
    }
}

// ---------------------------------------------------------------------------
// NodeRef.matches & NodeRef.closest (Full Selector Parity)
// ---------------------------------------------------------------------------

#[test]
fn test_node_matches_and_closest() {
    let nav_doc = test_doc();

    let server = nav_doc.select_first("server").unwrap().unwrap();
    assert!(server.matches("server"));
    assert!(!server.matches("host"));
    assert!(server.matches("(network)server"));
    assert!(!server.matches("(i32)server"));
    assert!(server.matches("*"));
    assert!(server.matches("server[tls=#true]"));
    assert!(!server.matches("server[tls=#false]"));
    assert!(server.matches("app > server"));
    assert!(!server.matches("router > server"));

    let app = nav_doc.select_first("app").unwrap().unwrap();
    assert!(app.matches("*:root"));

    let host = nav_doc
        .select_first(r#"host[0="localhost"]"#)
        .unwrap()
        .unwrap();
    assert!(host.matches("app host"));

    let debug = nav_doc
        .select_first(r#"plugin[0="debug"]"#)
        .unwrap()
        .unwrap();
    assert!(!debug.matches("plugin[enabled=#true]"));
    assert!(!debug.matches("nonexistent"));

    // closest()
    assert_eq!(server.closest("server"), Some(server));
    assert_eq!(host.closest("server"), Some(server));
    assert_eq!(host.closest("app"), Some(app));
    assert_eq!(host.closest("nonexistent"), None);

    let closest_tls = host.closest("server[tls=#true]").unwrap();
    assert_eq!(closest_tls.arg(0).unwrap().as_str(), Some("primary"));

    let closest_typed = host.closest("(network)server").unwrap();
    assert_eq!(closest_typed.name(), "server");
    assert_eq!(closest_typed.ty(), Some("network"));

    assert_eq!(app.closest("app"), Some(app));
    assert_eq!(app.closest("nonexistent"), None);

    // Precompiled Selector with NodeMatcher, select, and select_in
    let compiled = Selector::parse("(network)server[tls=#true]").unwrap();
    assert!(server.matches(&compiled));
    assert_eq!(host.closest(&compiled), Some(server));
    assert_eq!(compiled.select(&nav_doc), vec![server]);
    assert_eq!(compiled.select_first(&nav_doc), Some(server));
    assert_eq!(compiled.select_in(app), vec![server]);
    assert_eq!(compiled.select_first_in(app), Some(server));

    // try_matches and try_closest surface KdlError on invalid selectors
    assert!(host.try_matches("app > server > host").unwrap());
    assert_eq!(
        host.try_closest("app > (network)server[tls=#true]")
            .unwrap(),
        Some(server)
    );
    assert!(host.try_matches("invalid[unclosed").is_err());
    assert!(host.try_closest("invalid[unclosed").is_err());
    assert!(!host.matches("invalid[unclosed"));
    assert_eq!(host.closest("invalid[unclosed"), None);

    // Document-level select_ids and select_first_id
    let ids = nav_doc.select_ids("server").unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(nav_doc.select_first_id("server").unwrap(), Some(ids[0]));
    assert_eq!(app.select_ids("server").unwrap(), ids);
    assert_eq!(app.select_first_id("server").unwrap(), Some(ids[0]));
    assert!(nav_doc.matches(server.id(), "(network)server"));
    assert!(nav_doc.try_matches(server.id(), "(network)server").unwrap());
    assert_eq!(
        nav_doc.try_closest(host.id(), "(network)server").unwrap(),
        Some(server)
    );

    // Value equality with KdlValue::Float(NaN)
    let ldoc = lit_doc();
    let nan_node = ldoc.select_first("node[val=#nan]").unwrap().unwrap();
    assert_eq!(nan_node.get("val"), Some(&KdlValue::Float(f64::NAN)));
}

#[test]
fn test_dsl_v2_1_linter_selector_patterns() {
    let schema = r##"
@doc "Sample schema"
import "./shared.kdl" {
    (struct)SharedItem
}
json ApiPayload {
    id int from="data.id"
    tags (array)str path="wrong.attr"
}
struct DefaultItem {
    title "h1" { text }
}
(list)struct Catalog {
    @split-doc "div.card"
    nested "items" SharedItem
}
(raw)struct RawExtractor {
    code { re #"(\d+)"# }
}
unknown-top-level "bad"
"##;
    let doc = parse(schema).expect("schema should parse");

    // Top-level unknown node check used in Pass 3
    let unknown = doc
        .select(":root:not(@doc, json, struct, fn, define, extension, import)")
        .unwrap();
    assert_eq!(names(&unknown), vec!["unknown-top-level"]);

    // Untyped vs typed struct selection
    let untyped_structs = doc.select("()struct:root").unwrap();
    assert_eq!(first_str_args(&untyped_structs), vec!["DefaultItem"]);

    let list_structs = doc.select("(list)struct:root").unwrap();
    assert_eq!(first_str_args(&list_structs), vec!["Catalog"]);

    let raw_structs = doc.select("(raw)struct:root").unwrap();
    assert_eq!(first_str_args(&raw_structs), vec!["RawExtractor"]);

    // Structs containing `nested`
    let callers = doc.select("struct:root:has(nested)").unwrap();
    assert_eq!(first_str_args(&callers), vec!["Catalog"]);
    let nested_ops = callers[0].select("nested").unwrap();
    assert_eq!(nested_ops.len(), 1);
    assert_eq!(nested_ops[0].arg(1).unwrap().as_str(), Some("SharedItem"));

    // JSON fields with `from` or `path`
    let json_node = doc.select_first("json:root").unwrap().unwrap();
    let from_fields = json_node.select("*[from]").unwrap();
    assert_eq!(names(&from_fields), vec!["id"]);
    let path_fields = json_node.select("*[path]").unwrap();
    assert_eq!(names(&path_fields), vec!["tags"]);
    let array_fields = json_node.select("*[(array)0]").unwrap();
    assert_eq!(names(&array_fields), vec!["tags"]);
}

#[test]
fn test_malformed_selector_syntax_errors() {
    let doc = test_doc();

    let invalid_selectors = [
        "",
        "   ",
        "app >",
        "app +",
        "app ~",
        "app ,",
        "> server",
        "+ route",
        "~ route",
        ", app",
        "server[port != 8080]",
        "server[0!8080]",
        "server[port^8080]",
        "server[port$8080]",
        "server[port=]",
        "server[port^=]",
        "server[port=8080",
        "server:hover",
        "server:unknown(1)",
        "server:not",
        "server:has",
        "route:nth-child",
        "app:root()",
        "backend:empty()",
        "app ]",
        "app )",
        "node[offset=+-5]",
        "node[offset=+foo]",
        "node[offset=1.2.3]",
        "route:nth-child(+ -2)",
        "route:nth-child(+-n)",
    ];

    for pattern in invalid_selectors {
        let err = doc
            .select(pattern)
            .expect_err(&format!("expected error for selector {pattern:?}"));
        assert!(err.is_selector_error(), "pattern: {pattern:?}");
        assert!(err.as_selector_error().is_some(), "pattern: {pattern:?}");
        assert!(!err.message().is_empty(), "pattern: {pattern:?}");
    }
}
