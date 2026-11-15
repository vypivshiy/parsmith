import {
  CheckMethodInfo,
  CheckResult,
  CodegenOptions,
  Diagnostic,
  ErrorVariantInfo,
  ExtensionInfo,
  ExtensionOpInfo,
  FieldInfo,
  FnParamInfo,
  FunctionInfo,
  GenerateResult,
  HtmlLibMode,
  InitFieldInfo,
  JsonDefInfo,
  JsonFieldInfo,
  ModuleInfo,
  PlaceholderInfo,
  RequestInfo,
  RestEndpointInfo,
  SeverityLevel,
  StructInfo,
  TargetName,
  TypeInfo,
  __version__,
  check,
  generate,
  inspectSchema,
  inspect_schema,
  jsonToKdl,
  json_to_kdl,
  version,
} from "../index.mjs";

const ver: string = version();
const verConst: string = __version__;

const target: TargetName = "py-lxml";
const htmlLib: HtmlLibMode = "lxml";
const phpTarget: TargetName = "php-dom-crawler";
void phpTarget;

const checkRes: CheckResult = check('struct Card { title { css "h1"; text } }', {
  sources: new Map([["a.kdl", ""]]),
  targets: ["ts", "python"],
  path: "entry.kdl",
});
const ok: boolean = checkRes.ok;
const diags: Diagnostic[] = checkRes.diagnostics;
if (diags.length > 0) {
  const sev: SeverityLevel = diags[0].severity;
  const rendered: string = diags[0].render();
  void sev;
  void rendered;
}

const opts = new CodegenOptions(target, {
  htmlLib,
  models: "dataclass",
  separateRuntime: true,
  runtimeName: "rt",
  package: "main",
});

const genRes1: GenerateResult = generate("entry.kdl", opts);
const genRes2: GenerateResult = generate(new URL("file:///tmp/schema.kdl"), "ts", {
  moduleStem: "custom",
});
const code: string | null = genRes1.getFile("entry.py");
const has: boolean = genRes2.hasFile("custom.ts");

const info: ModuleInfo = inspectSchema("entry.kdl", {
  sources: { "entry.kdl": 'struct Card { title { css "h1"; text } }' },
});
const info2: ModuleInfo = inspect_schema("entry.kdl");

const structs: StructInfo[] = info.structs;
const fns: FunctionInfo[] = info.functions;
const jsons: JsonDefInfo[] = info.jsonDefs;
const exts: ExtensionInfo[] = info.extensions;

if (structs.length > 0) {
  const s = structs[0];
  const f: FieldInfo | undefined = s.fields[0];
  const initF: InitFieldInfo | undefined = s.initFields[0];
  const chk: CheckMethodInfo | undefined = s.checks[0];
  const req: RequestInfo | undefined = s.requests[0];
  const ep: RestEndpointInfo | undefined = s.restEndpoints[0];
  const err: ErrorVariantInfo | undefined = s.errors[0];
  const ph: PlaceholderInfo | undefined = req?.placeholders[0];
  const ti: TypeInfo | undefined = f?.typeInfo;
  void initF;
  void chk;
  void ep;
  void err;
  void ph;
  void ti;
}

if (fns.length > 0) {
  const p: FnParamInfo | undefined = fns[0].params[0];
  void p;
}

if (jsons.length > 0) {
  const jf: JsonFieldInfo | undefined = jsons[0].fields[0];
  void jf;
}

if (exts.length > 0) {
  const op: ExtensionOpInfo | undefined = exts[0].operations[0];
  void op;
}

const kdl1: string = jsonToKdl('{"a": 1}', "Sample");
const kdl2: string = json_to_kdl('{"a": 1}', { name: "Sample" });

void ver;
void verConst;
void ok;
void code;
void has;
void info2;
void kdl1;
void kdl2;
