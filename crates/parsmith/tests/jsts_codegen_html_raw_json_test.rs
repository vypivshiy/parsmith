mod common;

use std::fs;
use std::process::Command;

use common::*;
use parsmith::codegen::jsts::js_regex_literal;
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, JsTsEmitter, JsTsMode, MemorySourceLoader,
    ModuleBuilder, Target, drive_module, generate_path, generate_source,
    generate_source_with_loader,
};

const SCHEMA_EXTENSIONS_JSTS: &str = r####"
extension Utils {
    to-base64 {
        sig str str

        js {
            emit #"const {{out}} = btoa({{in}});"#
        }
    }

    normalize {
        sig str str

        js {
            helper normalizeValue {
                source #"""
                    function normalizeValue(value) {
                      return value.replace(/\s+/g, " ").trim();
                    }
                    """#
            }
            emit #"const {{out}} = normalizeValue({{in}});"#
        }

        ts {
            helper normalizeValue {
                source #"""
                    function normalizeValue(value: string): string {
                      return value.replace(/\s+/g, " ").trim();
                    }
                    """#
            }
            emit #"const {{out}}: {{out_type}} = normalizeValue({{in}});"#
        }
    }

    truthy {
        sig T bool
        js { emit #"const {{out}} = Boolean({{in}});"# }
    }
}

(raw)struct ExtRawParser {
    @check is-valid {
        !Utils.truthy
    }

    encoded {
        !Utils.normalize
        !Utils.to-base64
    }
}

(raw)fn encode_normalized {
    !Utils.normalize
    !Utils.to-base64
}
"####;

#[test]
fn regex_normalizer_converts_inline_flags_verbose_and_named_groups() {
    assert_eq!(
        js_regex_literal(r"(?i)^hello/world$", false),
        r"/^hello\/world$/i"
    );
    assert_eq!(js_regex_literal(r"(?ims)^a.+b$", true), r"/^a.+b$/gims");
    assert_eq!(
        js_regex_literal("(?x)\n  (\\d+)  # numeric id\n  /items\n", false),
        r"/(\d+)\/items/"
    );
}

#[test]
fn jsts_dry_run_populates_module_builder_with_zero_allocation() {
    let module = parsmith::compile_source(SCHEMA_EXTENSIONS_JSTS, Some("ext.kdl")).unwrap();
    let opts = CodegenOptions::new(Target::TypeScript);
    let mut emitter = JsTsEmitter::new(opts, JsTsMode::TypeScript);
    let mut builder = ModuleBuilder::new();
    let mut dry_writer = CodeWriter::dry_run();

    parsmith::drive_single_pass(
        &mut emitter,
        &module,
        &mut builder,
        &mut dry_writer,
        None,
        None,
    );

    assert_eq!(dry_writer.capacity(), 0);
    assert!(dry_writer.finish().is_empty());
    assert!(builder.has_runtime());
    assert_eq!(
        builder.runtime_names().collect::<Vec<_>>(),
        vec!["normalizeValue"]
    );

    let (emitted, _) = drive_module(&mut emitter, &module, None, None);
    assert!(emitted.contains("function normalizeValue(value: string): string {"));
    assert_eq!(emitted.matches("function normalizeValue").count(), 1);
}

#[test]
fn trim_chars_helpers_exported_and_declared_in_dts() {
    let src = r#"
(raw)struct TrimCustom {
    a { trim "-_" }
    b { ltrim "[" }
    c { rtrim "]" }
}
"#;
    let ts_res = generate_source(
        src,
        Some("trim_custom.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors());
    let ts = &ts_res.files["trim_custom.ts"];
    assert!(ts.contains("export function rtTrimChars(s: string, chars: string): string"));
    assert!(ts.contains("export function rtLtrimChars(s: string, chars: string): string"));
    assert!(ts.contains("export function rtRtrimChars(s: string, chars: string): string"));

    let js_res = generate_source(
        src,
        Some("trim_custom.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors());
    let js = &js_res.files["trim_custom.js"];
    let dts = &js_res.files["trim_custom.d.ts"];
    assert!(js.contains("export function rtTrimChars(s, chars)"));
    assert!(js.contains("export function rtLtrimChars(s, chars)"));
    assert!(js.contains("export function rtRtrimChars(s, chars)"));
    assert!(dts.contains("export declare function rtTrimChars(s: string, chars: string): string;"));
    assert!(
        dts.contains("export declare function rtLtrimChars(s: string, chars: string): string;")
    );
    assert!(
        dts.contains("export declare function rtRtrimChars(s: string, chars: string): string;")
    );
}

#[test]
fn snapshot_ts_and_js_item_init_check() {
    let ts_res = generate_source(
        SCHEMA_ITEM_INIT_CHECK,
        Some("page_item.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    insta::assert_snapshot!("ts_page_item", &ts_res.files["page_item.ts"]);

    let js_res = generate_source(
        SCHEMA_ITEM_INIT_CHECK,
        Some("page_item.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
    assert_eq!(js_res.files.len(), 2);
    insta::assert_snapshot!("js_page_item", &js_res.files["page_item.js"]);
    insta::assert_snapshot!("dts_page_item", &js_res.files["page_item.d.ts"]);
}

#[test]
fn snapshot_ts_list_strings_basic() {
    let res = generate_source(
        SCHEMA_01_STRINGS_BASIC,
        Some("01_strings_basic.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_list_arrays_and_conversions() {
    let res = generate_source(
        SCHEMA_02_ARRAYS_AND_CONVERSIONS,
        Some("02_arrays_and_conversions.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_list_filters_and_predicates() {
    let res = generate_source(
        SCHEMA_03_FILTERS_AND_PREDICATES,
        Some("03_filters_and_predicates.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_flat_struct() {
    let res = generate_source(
        SCHEMA_05_FLAT,
        Some("05_flat.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_dict_struct() {
    let res = generate_source(
        SCHEMA_06_DICT,
        Some("06_dict.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_table_struct() {
    let res = generate_source(
        SCHEMA_07_TABLE,
        Some("07_table.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_raw_struct() {
    let res = generate_source(
        SCHEMA_23_RAW_STRUCT,
        Some("23_raw_struct.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_fn_and_raw_fn() {
    let res = generate_source(
        SCHEMA_25_FN,
        Some("25_fn.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_ts_and_js_extensions() {
    let ts_res = generate_source(
        SCHEMA_EXTENSIONS_JSTS,
        Some("extensions.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    insta::assert_snapshot!("ts_extensions", &ts_res.files["extensions.ts"]);

    let js_res = generate_source(
        SCHEMA_EXTENSIONS_JSTS,
        Some("extensions.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
    insta::assert_snapshot!("js_extensions", &js_res.files["extensions.js"]);
    insta::assert_snapshot!("dts_extensions", &js_res.files["extensions.d.ts"]);
}

#[test]
fn snapshot_ts_and_js_json_projectors() {
    let ts_res = generate_source(
        SCHEMA_JSON_PROJECTORS,
        Some("json_proj.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    insta::assert_snapshot!("ts_json_projectors", &ts_res.files["json_proj.ts"]);

    let js_res = generate_source(
        SCHEMA_JSON_PROJECTORS,
        Some("json_proj.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
    insta::assert_snapshot!("js_json_projectors", &js_res.files["json_proj.js"]);
    insta::assert_snapshot!("dts_json_projectors", &js_res.files["json_proj.d.ts"]);
}

#[test]
fn missing_jsts_extension_target_reports_e400() {
    let mut vfs = MemorySourceLoader::new();
    vfs.insert(
        "shared/ext.kdl",
        r#"
extension OnlyPy {
    noop {
        sig str str
        py { emit "{{out}} = {{in}}" }
    }
}
"#,
    );
    let main_src = r#"
import "./shared/ext.kdl" { (extension)OnlyPy }
(raw)fn test_fn { !OnlyPy.noop }
"#;
    let res_ts = generate_source_with_loader(
        main_src,
        Some("main.kdl"),
        &vfs,
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(res_ts.has_errors());
    assert_eq!(res_ts.diagnostics[0].code, DiagnosticCode::E400);
    assert!(
        res_ts.diagnostics[0]
            .message
            .contains("extension operation 'OnlyPy.noop' has no 'ts' target")
    );

    let res_js = generate_source_with_loader(
        main_src,
        Some("main.kdl"),
        &vfs,
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(res_js.has_errors());
    assert_eq!(res_js.diagnostics[0].code, DiagnosticCode::E400);
    assert!(
        res_js.diagnostics[0]
            .message
            .contains("extension operation 'OnlyPy.noop' has no 'js' target")
    );
}

#[test]
fn generate_path_for_ts_and_js() {
    let tmp = TempTestDir::new("gen_path");
    let schema_file = tmp.write("sample.kdl", SCHEMA_25_FN);

    let ts_res = generate_path(&schema_file, &CodegenOptions::new(Target::TypeScript));
    assert!(!ts_res.has_errors());
    assert!(ts_res.files.contains_key("sample.ts"));

    let js_res = generate_path(&schema_file, &CodegenOptions::new(Target::JavaScript));
    assert!(!js_res.has_errors());
    assert!(js_res.files.contains_key("sample.js"));
    assert!(js_res.files.contains_key("sample.d.ts"));
}

#[test]
fn jsts_toolchain_node_execution_and_tsc_strict() {
    let tmp = TempTestDir::new("toolchain");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let schemas = [
        ("m_item.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("m_strings.kdl", SCHEMA_01_STRINGS_BASIC),
        ("m_arrays.kdl", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("m_filters.kdl", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("m_flat.kdl", SCHEMA_05_FLAT),
        ("m_dict.kdl", SCHEMA_06_DICT),
        ("m_table.kdl", SCHEMA_07_TABLE),
        ("m_raw.kdl", SCHEMA_23_RAW_STRUCT),
        ("m_fn.kdl", SCHEMA_25_FN),
        ("m_ext.kdl", SCHEMA_EXTENSIONS_JSTS),
        ("m_xpath.kdl", SCHEMA_XPATH_DOM),
        ("m_json.kdl", SCHEMA_JSON_PROJECTORS),
    ];

    let ts_dir = tmp.path.join("ts_out");
    let js_dir = tmp.path.join("js_out");
    fs::create_dir_all(&ts_dir).unwrap();
    fs::create_dir_all(&js_dir).unwrap();

    // 1. Generate all schemas in standalone TypeScript (`.ts`) mode
    let ts_opts = CodegenOptions::new(Target::TypeScript);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &ts_opts);
        assert!(
            !res.has_errors(),
            "TS generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            tmp.write(&format!("ts_out/{fname}"), code);
        }
    }

    // 2. Generate all schemas in JavaScript + Declaration (`.js` + `.d.ts`) mode
    let js_opts = CodegenOptions::new(Target::JavaScript);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &js_opts);
        assert!(
            !res.has_errors(),
            "JS generation failed for {name}: {:?}",
            res.diagnostics
        );
        assert_eq!(res.files.len(), 2, "Expected .js and .d.ts for {name}");
        for (fname, code) in &res.files {
            tmp.write(&format!("js_out/{fname}"), code);
        }
    }

    // 3. Write a strict TypeScript consumer in `js_out/check_dts_consumer.ts` that imports from the `.js` + `.d.ts` pairs
    let dts_consumer_ts = r#"import { PageItem, type PageItemType, SingleCard, type SingleCardType, RtAssertionError, rtAssert, rtParseHtml } from "./m_item.js";
import { StringsBasic, type StringsBasicType, rtCssRemove, rtRmPrefix, rtRmSuffix, rtNormalizeText } from "./m_strings.js";
import { ArraysAndConversions, type ArraysAndConversionsType, RtRegexError, rtReSearch, rtReplMap, rtUnescapeText } from "./m_arrays.js";
import { FiltersAndPredicates, type FiltersAndPredicatesType } from "./m_filters.js";
import { FlatCoverage, type FlatCoverageType } from "./m_flat.js";
import { MetaDict, type MetaDictType, DictRoot, type DictRootType } from "./m_dict.js";
import { TableCoverage, type TableCoverageType, RT_UNMATCHED_TABLE_ROW } from "./m_table.js";
import { RawPlayerScript, type RawPlayerScriptType, RawLineItems, type RawLineItemsType } from "./m_raw.js";
import { pageTitle, allLinks, firstLine, extractVersion } from "./m_fn.js";
import { ExtRawParser, encodeNormalized } from "./m_ext.js";
import { XpathArticle, type XpathArticleType, rtXpathFirst, rtXpathAll, rtXpathRemove } from "./m_xpath.js";
import {
  JsonBasic,
  type JsonBasicType,
  ManifestParser,
  type ReleaseManifestJson,
  TranslationsParser,
  type TranslationsJson,
  rtProjectReleaseManifest,
  RtJsonError,
  RtJsonSchemaError,
  RtJsonFieldMissingError,
  RtJsonPathError,
  rtTypeName,
  rtExpectObject,
  rtExpectArray,
  rtSplitGjsonPath,
  rtResolveDotpath,
  rtJsonField,
  rtJsonPath,
} from "./m_json.js";

export function verifyTypes(html: string, raw: string): void {
  const itemParser = new PageItem(html);
  const hasCards: boolean = itemParser.hasCards();
  const itemData: PageItemType = itemParser.parse();
  const single: SingleCardType = itemData.single;
  const _heading: string = itemData.heading;
  const _singleTitle: string = single.title;
  const _hasCards = hasCards;

  const listData: StringsBasicType[] = new StringsBasic(html).parse();
  const arrData: ArraysAndConversionsType[] = new ArraysAndConversions(html).parse();
  const filtData: FiltersAndPredicatesType[] = new FiltersAndPredicates(html).parse();
  const flatData: FlatCoverageType = new FlatCoverage(html).parse();
  const dictData: MetaDictType = new MetaDict(html).parse();
  const rootDict: DictRootType = new DictRoot(html).parse();
  const tableData: TableCoverageType = new TableCoverage(html).parse();
  const rawItem: RawPlayerScriptType = new RawPlayerScript(raw).parse();
  const rawLines: RawLineItemsType[] = new RawLineItems(raw).parse();
  const t1: string = pageTitle(html);
  const t2: string[] = allLinks(html);
  const t3: string = firstLine(raw);
  const t4: string = extractVersion(raw);
  const extParser = new ExtRawParser(raw);
  const valid: boolean = extParser.isValid();
  const encoded: string = encodeNormalized(raw);
  const xpData: XpathArticleType = new XpathArticle(html).parse();
  const jbData: JsonBasicType = new JsonBasic(html).parse();
  const mfData: ReleaseManifestJson = new ManifestParser(raw).parse().manifest;
  const trData: TranslationsJson = new TranslationsParser(raw).parse().translations;
  const projMf: ReleaseManifestJson = rtProjectReleaseManifest({});
  const _errs = [RtAssertionError, RtRegexError, RtJsonError, RtJsonSchemaError, RtJsonFieldMissingError, RtJsonPathError];
  const _rtHelpers = [
    rtAssert,
    rtParseHtml,
    rtCssRemove,
    rtRmPrefix,
    rtRmSuffix,
    rtReplMap,
    rtNormalizeText,
    rtUnescapeText,
    rtReSearch,
    RT_UNMATCHED_TABLE_ROW,
    rtXpathFirst,
    rtXpathAll,
    rtXpathRemove,
    rtTypeName,
    rtExpectObject,
    rtExpectArray,
    rtSplitGjsonPath,
    rtResolveDotpath,
    rtJsonField,
    rtJsonPath,
  ];

  void [
    _heading,
    _singleTitle,
    _hasCards,
    listData,
    arrData,
    filtData,
    flatData,
    dictData,
    rootDict,
    tableData,
    rawItem,
    rawLines,
    t1,
    t2,
    t3,
    t4,
    valid,
    encoded,
    xpData,
    jbData,
    mfData,
    trData,
    projMf,
    _errs,
    _rtHelpers,
  ];
}
"#;
    tmp.write("js_out/check_dts_consumer.ts", dts_consumer_ts);
    tmp.write("ts_out/check_ts_consumer.ts", dts_consumer_ts);

    // Write tsconfig.json for both `ts_out` and `js_out`
    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022", "DOM"],
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["./*.ts", "./*.d.ts"]
}
"#;
    let ts_config_path = tmp.write("ts_out/tsconfig.json", tsconfig);
    let js_config_path = tmp.write("js_out/tsconfig.json", tsconfig);

    // 4. Run `tsc --noEmit --strict` on both `ts_out` (`.ts` files) and `js_out` (`.js + .d.ts` files)
    let tsc_bin = if cfg!(windows) { "tsc.cmd" } else { "tsc" };

    let tsc_ts_out = Command::new(tsc_bin)
        .args(["-p", ts_config_path.to_str().unwrap()])
        .output()
        .expect("execute tsc on ts_out");
    assert!(
        tsc_ts_out.status.success(),
        "tsc --strict failed on .ts files!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tsc_ts_out.stdout),
        String::from_utf8_lossy(&tsc_ts_out.stderr)
    );

    let tsc_js_out = Command::new(tsc_bin)
        .args(["-p", js_config_path.to_str().unwrap()])
        .output()
        .expect("execute tsc on js_out");
    assert!(
        tsc_js_out.status.success(),
        "tsc --strict failed on .js + .d.ts files!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tsc_js_out.stdout),
        String::from_utf8_lossy(&tsc_js_out.stderr)
    );

    // 5. Execute both generated `.js` and `.ts` modules in Node.js with `jsdom`
    let runner_body = r##"import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(process.env.PARSMITH_JSDOM_PKG || import.meta.url);
const { JSDOM } = require("jsdom");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const html = fs.readFileSync(path.join(__dirname, "..", "dsl_coverage.html"), "utf-8");

const dom = new JSDOM(html, { url: "http://localhost" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.DOMParser = dom.window.DOMParser;
globalThis.XPathResult = dom.window.XPathResult;
globalThis.Element = dom.window.Element;
globalThis.Document = dom.window.Document;

import * as mItem from "./m_item.EXT";
import * as mStrings from "./m_strings.EXT";
import * as mArrays from "./m_arrays.EXT";
import * as mFilters from "./m_filters.EXT";
import * as mFlat from "./m_flat.EXT";
import * as mDict from "./m_dict.EXT";
import * as mTable from "./m_table.EXT";
import * as mRaw from "./m_raw.EXT";
import * as mFn from "./m_fn.EXT";
import * as mExt from "./m_ext.EXT";
import * as mXpath from "./m_xpath.EXT";
import * as mJson from "./m_json.EXT";

// 1. Item + @init + @check + @pre-validate + nested
{
  const parser = new mItem.PageItem(html);
  assert.equal(parser.hasCards(), true);
  const data = parser.parse();
  assert.equal(data.heading, "Coverage Root");
  assert.equal(data.canonical, "https://example.test/coverage");
  assert.deepEqual(data.single, { title: "Single Item Title", active: true });

  // Also test passing a parsed Document directly instead of string
  const docDirect = new globalThis.DOMParser().parseFromString(html, "text/html");
  assert.deepEqual(new mItem.PageItem(docDirect).parse(), data);

  assert.throws(
    () => new mItem.PageItem("<html><body></body></html>").parse(),
    (err) => err instanceof mItem.RtAssertionError && err.message.includes("missing single section")
  );
}

// 2. List (01_strings_basic)
{
  const res = new mStrings.StringsBasic(html).parse();
  assert.equal(res.length, 2);
  assert.equal(res[0].title, "bar Title One");
  assert.equal(res[0].link, "https://example.testguide");
  assert.equal(res[0].slug, "slug-one");
  assert.equal(res[0].normalizedCode, "foo-bar");
  assert.equal(res[0].activeFlag, true);
  assert.ok(res[0].innerHtml.includes('<h2 class="title">'));
  assert.ok(res[0].innerHtml.length < res[0].cleanHtml.length);
  assert.equal(res[1].title, "bar Title Two");
  assert.equal(res[1].normalizedCode, "code-4");
}

// 3. Arrays & conversions (02_arrays_and_conversions)
{
  const res = new mArrays.ArraysAndConversions(html).parse();
  assert.equal(res.length, 2);
  const item0 = res[0];
  assert.deepEqual(item0.tokenList, ["tag-core-alpha1-x", "item-beta2-y", "ref3"]);
  assert.equal(item0.firstToken, "tag-core-alpha1-x");
  assert.equal(item0.secondToken, "item-beta2-y");
  assert.equal(item0.lastToken, "ref3");
  assert.equal(item0.tokenWindow, "tag-core-alpha1-x|item-beta2-y");
  assert.equal(item0.stateCode, 1);
  assert.equal(item0.score, 41);
  assert.equal(item0.ratio, 0.75);
  assert.equal(item0.anyLinkCount, 2);
  assert.equal(item0.flagsJoined, "ALPHA,BETA");
}

// 4. Filters & predicates (03_filters_and_predicates)
{
  const res = new mFilters.FiltersAndPredicates(html).parse();
  assert.equal(res.length, 2);
  assert.deepEqual(res[0].filteredLinks, []);
  assert.equal(res[0].logicCheck, "prealpha");
  assert.equal(res[0].numericCheck, "");
  assert.equal(res[1].numericCheck, "code-4");

  const inlineCard =
    '<div class="card">' +
    '<a href="/guide.html" data-kind="nav" data-alt="alt-1" rel="noopener">' +
    '<span class="icon"></span>Go guide now</a>' +
    '<div class="logic">prealpha</div>' +
    '<div class="code">code-8</div>' +
    "</div>";
  const resInline = new mFilters.FiltersAndPredicates(inlineCard).parse();
  assert.deepEqual(resInline, [
    {
      filteredLinks: ["/guide.html", "alt-1"],
      logicCheck: "prealpha",
      numericCheck: "code-8",
    },
  ]);
}

// 5. Flat & Dict structs (05_flat, 06_dict)
{
  const flatRes = new mFlat.FlatCoverage(html).parse();
  assert.ok(Array.isArray(flatRes));
  assert.ok(flatRes.includes("/nested/one.html"));
  assert.ok(flatRes.includes("tag-core-alpha1-x"));

  const metaDict = new mDict.MetaDict(html).parse();
  assert.equal(metaDict.description, "Coverage description");

  const dictRoot = new mDict.DictRoot(html).parse();
  assert.equal(dictRoot.namedMeta.keywords, "alpha,beta,core");
  assert.equal(dictRoot.aliasMeta["og:title"], "Coverage OG Title");
}

// 6. Table struct (07_table)
{
  const tableRes = new mTable.TableCoverage(html).parse();
  assert.deepEqual(tableRes, {
    identifier: "ABC-123",
    codeValue: "CODE-1",
    price: 9.99,
    taxOrFee: 1.25,
    state: "active",
  });
}

// 7. Raw structs & standalone fn / (raw)fn (23_raw_struct, 25_fn)
{
  const jsText = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>';
  const urlText = "https://ru.yummyani.me/iframeCVH.html?dubbing_code=Sanae&anime_id=339&episode=1";
  const playlistText = "[480p]/v/anime/01_480p.m3u8\n[720p]/v/anime/01_720p.m3u8";

  assert.deepEqual(new mRaw.RawPlayerScript(jsText).parse(), {
    playlistUrl: "/v/list/abc.txt",
    playerId: "player",
  });
  assert.deepEqual(new mRaw.RawUrlParams(urlText).parse(), {
    dubbingCode: "Sanae",
    animeId: 339,
  });
  assert.deepEqual(new mRaw.RawLineItems(playlistText).parse(), [
    { quality: "480p", url: "/v/anime/01_480p.m3u8" },
    { quality: "720p", url: "/v/anime/01_720p.m3u8" },
  ]);
  assert.deepEqual(new mRaw.RawTextTransform("hello   world").parse(), {
    upperText: "HELLO   WORLD",
    wordCount: 2,
    firstWord: "hello",
  });

  assert.equal(mFn.pageTitle("<html><body><h1>Hello World</h1></body></html>"), "Hello World");
  assert.deepEqual(
    mFn.allLinks("<html><body><a href='/a'>A</a><a href='/b'>B</a></body></html>"),
    ["/a", "/b"]
  );
  assert.equal(mFn.firstLine("first\nsecond"), "first");
  assert.equal(mFn.extractVersion("app version=1.2.3 released"), "1.2.3");
}

// 8. Extensions & XPath over DOM
{
  const extParser = new mExt.ExtRawParser("  hello   world  ");
  assert.equal(extParser.isValid(), true);
  assert.deepEqual(extParser.parse(), { encoded: "aGVsbG8gd29ybGQ=" });
  assert.equal(mExt.encodeNormalized("  hello   world  "), "aGVsbG8gd29ybGQ=");

  const xpRes = new mXpath.XpathArticle(html).parse();
  assert.equal(xpRes.heading, "Single Item Title");
  assert.deepEqual(xpRes.nestedNames, ["Nested One", "Nested Two"]);
}

// 9. Allowlist JSON Projectors (json, (array)json, (dict)json, hoisted inline, GJSON paths, ?, @omitempty, @skip)
{
  const jb = new mJson.JsonBasic(html).parse();
  assert.equal(jb.fullPayload.length, 2);
  assert.equal(jb.fullPayload[0].text, "Quote one");
  assert.equal(jb.fullPayload[0].score, 7);
  assert.deepEqual(jb.firstAuthor, { name: "Author One", slug: "author-one" });
  assert.deepEqual(jb.secondAuthor, { name: "Author Two", slug: "author-two" });

  const manifestPayload = JSON.stringify({
    "envelope.data": {
      "os.version": "24.04",
      meta: {
        "os.version": "6.8.0-generic",
        "path\\dir": "C:\\bin",
        "#": "pound_val",
      },
      items: ["a", "b", "c"],
      tags: [{ name: "action", id: 1 }, { name: "sci-fi", id: 2 }, { other: "skipped" }],
      groups: [{ meta: { id: 10 } }, { meta: { id: 20 } }],
      inline_meta: { env: "prod", undeclared: "drop_me" },
      extra_wire_key: "dropped",
    },
  });
  const mf = new mJson.ManifestParser(manifestPayload).parse().manifest;
  assert.deepEqual(mf, {
    os_version: "24.04",
    nested_version: "6.8.0-generic",
    backslash_key: "C:\\bin",
    hash_key: "pound_val",
    item_count: 3,
    opt_count: null,
    tag_names: ["action", "sci-fi"],
    deep_ids: [10, 20],
    inline_meta: { env: "prod" },
  });
  assert.equal("legacy_id" in mf, false);
  assert.equal("channels" in mf, false);
  assert.equal("extra_wire_key" in mf, false);
  assert.equal("region" in mf.inline_meta, false);
  assert.equal("undeclared" in mf.inline_meta, false);

  const trPayload = JSON.stringify({
    rus: { display_title: "Русский", count: 12, notes: "ignore", extra: 1 },
    eng: { display_title: "English", count: 24, meta: { badge: "official" } },
  });
  const tr = new mJson.TranslationsParser(trPayload).parse().translations;
  assert.deepEqual(tr, {
    rus: { title: "Русский", count: 12 },
    eng: { title: "English", count: 24, badge: "official" },
  });

  // Error contracts on JSON projectors
  assert.throws(
    () => new mJson.TranslationsParser('["not", "an", "object"]').parse(),
    (err) =>
      err instanceof mJson.RtJsonSchemaError &&
      err.message.includes("Translations: Expected object, got array")
  );

  assert.throws(
    () => new mJson.TranslationsParser('{"rus": {"count": 1}}').parse(),
    (err) =>
      err instanceof mJson.RtJsonFieldMissingError &&
      err.message.includes("TranslationValue.title: Required JSON field 'display_title'")
  );

  const badHashPayload = JSON.stringify({
    "envelope.data": {
      "os.version": "24.04",
      meta: { "os.version": "6.8", "path\\dir": "C:\\", "#": "p" },
      items: "not_an_array",
      tags: [],
      groups: [],
      inline_meta: { env: "prod" },
    },
  });
  assert.throws(
    () => new mJson.ManifestParser(badHashPayload).parse(),
    (err) =>
      err instanceof mJson.RtJsonPathError &&
      err.message.includes("ReleaseManifest.item_count: Expected list for '#'")
  );
}
"##;

    let js_runner_path = tmp.write("js_out/run_suite.mjs", &runner_body.replace(".EXT", ".js"));
    let ts_runner_path = tmp.write("ts_out/run_suite.ts", &runner_body.replace(".EXT", ".ts"));
    let jsdom_pkg = resolve_jsdom_pkg();

    let node_js_out = Command::new("node")
        .env("PARSMITH_JSDOM_PKG", &jsdom_pkg)
        .arg(&js_runner_path)
        .output()
        .expect("execute node on js_out/run_suite.mjs");
    assert!(
        node_js_out.status.success(),
        "node execution failed on .js modules!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&node_js_out.stdout),
        String::from_utf8_lossy(&node_js_out.stderr)
    );

    let node_ts_out = Command::new("node")
        .env("PARSMITH_JSDOM_PKG", &jsdom_pkg)
        .arg(&ts_runner_path)
        .output()
        .expect("execute node on ts_out/run_suite.ts");
    assert!(
        node_ts_out.status.success(),
        "node execution failed on .ts modules!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&node_ts_out.stdout),
        String::from_utf8_lossy(&node_ts_out.stderr)
    );
}
