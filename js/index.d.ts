export type TargetName =
  | "python"
  | "py"
  | "py-bs4"
  | "python-bs4"
  | "py-lxml"
  | "python-lxml"
  | "py-parsel"
  | "python-parsel"
  | "py-selectolax"
  | "python-selectolax"
  | "py-slax"
  | "python-slax"
  | "ts"
  | "typescript"
  | "js"
  | "javascript"
  | "go"
  | "golang"
  | "rust"
  | "rs"
  | "elixir"
  | "ex"
  | "php"
  | "php-native"
  | "php-ext-dom"
  | "php-dom"
  | "php-dom-crawler"
  | "php-dom_crawler"
  | "php-crawler"
  | "php-symfony"
  | "csharp"
  | "cs"
  | "kotlin"
  | "kt";

export type HtmlLibMode = "bs4" | "lxml" | "parsel" | "selectolax" | "slax";
export type PhpHtmlLibMode =
  | "native"
  | "ext-dom"
  | "dom"
  | "dom-crawler"
  | "dom_crawler"
  | "crawler"
  | "symfony";
export type PhpHtmlLibName = PhpHtmlLibMode;
export type ModelsMode = "typeddict" | "dataclass";
export type Bs4ParserMode = "lxml" | "html.parser" | "html5lib";
export type HttpIoMode = "both" | "sync" | "async";
export type SeverityLevel = "error" | "warning";

export type PathInput = string | URL;
export type BatchPathInput = PathInput | readonly PathInput[];
export type SourcesInput = Record<string, string> | Map<string | URL, string>;

export interface CheckOptionsInput {
  sources?: SourcesInput | null;
  targets?: string | readonly string[] | null;
  path?: string | null;
}

export interface CodegenOptionsInput {
  target?: TargetName | (string & {});
  htmlLib?: HtmlLibMode | PhpHtmlLibMode | (string & {});
  html_lib?: HtmlLibMode | PhpHtmlLibMode | (string & {});
  "html-lib"?: HtmlLibMode | PhpHtmlLibMode | (string & {});
  lib?: HtmlLibMode | PhpHtmlLibMode | (string & {});
  phpHtmlLib?: PhpHtmlLibMode | (string & {});
  php_html_lib?: PhpHtmlLibMode | (string & {});
  "php-html-lib"?: PhpHtmlLibMode | (string & {});
  models?: ModelsMode | (string & {});
  bs4Parser?: Bs4ParserMode | (string & {});
  bs4_parser?: Bs4ParserMode | (string & {});
  "bs4-parser"?: Bs4ParserMode | (string & {});
  httpIo?: HttpIoMode | (string & {});
  http_io?: HttpIoMode | (string & {});
  "http-io"?: HttpIoMode | (string & {});
  separateRuntime?: boolean;
  separate_runtime?: boolean;
  "separate-runtime"?: boolean;
  runtimeName?: string;
  runtime_name?: string;
  "runtime-name"?: string;
  package?: string;
  namespace?: string;
  moduleStem?: string | null;
  module_stem?: string | null;
  "module-stem"?: string | null;
  prologue?: string | null;
  epilogue?: string | null;
}

export interface GenerateCallOptionsInput extends CodegenOptionsInput {
  sources?: SourcesInput | null;
  path?: string | null;
  options?: CodegenOptions | CodegenOptionsInput | null;
}

export interface InspectOptionsInput {
  sources?: SourcesInput | null;
  path?: string | null;
}

export interface JsonToKdlOptionsInput {
  name?: string | null;
}

export interface DiagnosticObject {
  code: string;
  codeName: string;
  severity: SeverityLevel;
  message: string;
  path: string | null;
  line: number;
  col: number;
  endLine: number;
  endCol: number;
  hint: string | null;
  notes: string[];
  label: string | null;
}

export interface CheckResultObject {
  ok: boolean;
  diagnostics: DiagnosticObject[];
}

export interface CodegenOptionsObject {
  target: string;
  htmlLib: string;
  phpHtmlLib: string;
  models: string;
  bs4Parser: string;
  httpIo: string;
  separateRuntime: boolean;
  runtimeName: string;
  package: string;
  moduleStem: string | null;
  prologue: string | null;
  epilogue: string | null;
}

export interface GenerateResultObject {
  ok: boolean;
  files: Record<string, string>;
  diagnostics: DiagnosticObject[];
}

export declare const __version__: string;

export declare function version(): string;

export declare class Diagnostic {
  readonly code: string;
  readonly codeName: string;
  readonly severity: SeverityLevel;
  readonly message: string;
  readonly path: string | null;
  readonly line: number;
  readonly col: number;
  readonly endLine: number;
  readonly endCol: number;
  readonly hint: string | null;
  readonly notes: string[];
  readonly label: string | null;
  readonly isError: boolean;
  readonly isWarning: boolean;

  render(source?: string | null): string;
  toObject(): DiagnosticObject;
  toJSON(): DiagnosticObject;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class CheckResult {
  readonly diagnostics: Diagnostic[];
  readonly errors: Diagnostic[];
  readonly warnings: Diagnostic[];
  readonly ok: boolean;
  readonly errorCount: number;
  readonly warningCount: number;

  hasErrors(): boolean;
  hasWarnings(): boolean;
  isOk(): boolean;
  formatText(): string;
  formatJson(): string;
  toObject(): CheckResultObject;
  toJSON(): CheckResultObject;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class CodegenOptions {
  target: string;
  htmlLib: string;
  phpHtmlLib: string;
  php_html_lib: string;
  models: string;
  bs4Parser: string;
  httpIo: string;
  separateRuntime: boolean;
  runtimeName: string;
  package: string;
  moduleStem: string | null;
  prologue: string | null;
  epilogue: string | null;

  constructor(
    targetOrOptions?: TargetName | (string & {}) | CodegenOptionsInput | null,
    options?: CodegenOptionsInput | null
  );
  toObject(): CodegenOptionsObject;
  toJSON(): CodegenOptionsObject;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class GenerateResult {
  readonly files: Record<string, string>;
  readonly diagnostics: Diagnostic[];
  readonly errors: Diagnostic[];
  readonly warnings: Diagnostic[];
  readonly ok: boolean;
  readonly code: string | null;

  primaryCode(): string | null;
  getFile(name: string): string | null;
  hasFile(name: string): boolean;
  hasErrors(): boolean;
  hasWarnings(): boolean;
  isOk(): boolean;
  formatText(): string;
  formatJson(): string;
  toObject(): GenerateResultObject;
  toJSON(): GenerateResultObject;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class TypeInfo {
  readonly base: string;
  readonly dslName: string;
  readonly isArray: boolean;
  readonly isOptional: boolean;
  readonly isDict: boolean;
  readonly reference: string | null;
  readonly keyType: TypeInfo | null;
  readonly valueType: TypeInfo | null;
  readonly omitempty: boolean;
  readonly skip: boolean;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class FieldInfo {
  readonly name: string;
  readonly typeInfo: TypeInfo;
  readonly hasFallback: boolean;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class InitFieldInfo {
  readonly name: string;
  readonly typeInfo: TypeInfo;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class CheckMethodInfo {
  readonly name: string;
  readonly returnType: TypeInfo;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class PlaceholderInfo {
  readonly name: string;
  readonly typeName: string;
  readonly isArray: boolean;
  readonly isOptional: boolean;
  readonly style: string | null;
  readonly token: string;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class RequestInfo {
  readonly name: string;
  readonly method: string;
  readonly url: string;
  readonly doc: string | null;
  readonly responseSchema: string | null;
  readonly responsePath: string | null;
  readonly responseJoin: string | null;
  readonly resultAliasName: string | null;
  readonly bodyKind: string;
  readonly placeholders: PlaceholderInfo[];

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class RestEndpointInfo {
  readonly name: string;
  readonly method: string;
  readonly url: string;
  readonly doc: string | null;
  readonly responseSchema: string | null;
  readonly responsePath: string | null;
  readonly responseIsArray: boolean;
  readonly resultAliasName: string;
  readonly bodyKind: string;
  readonly placeholders: PlaceholderInfo[];

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class ErrorVariantInfo {
  readonly variantName: string;
  readonly name: string;
  readonly status: number;
  readonly schemaName: string;
  readonly suffix: string;
  readonly requiredKeys: string[];
  readonly conditions: Record<string, string | number | boolean | null>;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class StructInfo {
  readonly name: string;
  readonly kind: string;
  readonly doc: string | null;
  readonly keepOrder: boolean;
  readonly fields: FieldInfo[];
  readonly initFields: InitFieldInfo[];
  readonly checks: CheckMethodInfo[];
  readonly hasPreValidate: boolean;
  readonly keyType: TypeInfo | null;
  readonly valueType: TypeInfo | null;
  readonly requests: RequestInfo[];
  readonly restEndpoints: RestEndpointInfo[];
  readonly errors: ErrorVariantInfo[];

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class FnParamInfo {
  readonly name: string;
  readonly typeInfo: TypeInfo;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class FunctionInfo {
  readonly name: string;
  readonly isRaw: boolean;
  readonly doc: string | null;
  readonly params: FnParamInfo[];
  readonly returnType: TypeInfo;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class JsonFieldInfo {
  readonly name: string;
  readonly typeInfo: TypeInfo;
  readonly alias: string | null;
  readonly doc: string | null;
  readonly isOptional: boolean;
  readonly omitempty: boolean;
  readonly skip: boolean;
  readonly isDict: boolean;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class JsonDefInfo {
  readonly name: string;
  readonly kind: string;
  readonly doc: string | null;
  readonly isArray: boolean;
  readonly isDict: boolean;
  readonly isHoisted: boolean;
  readonly path: string | null;
  readonly fields: JsonFieldInfo[];
  readonly keyType: TypeInfo | null;
  readonly valueType: TypeInfo | null;

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class ExtensionOpInfo {
  readonly name: string;
  readonly qualifiedName: string;
  readonly inputType: string;
  readonly outputType: string;
  readonly isGeneric: boolean;
  readonly targets: string[];

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class ExtensionInfo {
  readonly namespace: string;
  readonly name: string;
  readonly operations: ExtensionOpInfo[];

  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toString(): string;
}

export declare class ModuleInfo {
  readonly doc: string | null;
  readonly sourceFile: string;
  readonly structKinds: Record<string, string>;
  readonly structs: StructInfo[];
  readonly functions: FunctionInfo[];
  readonly jsonDefs: JsonDefInfo[];
  readonly extensions: ExtensionInfo[];
  readonly diagnostics: Diagnostic[];
  readonly errors: Diagnostic[];
  readonly warnings: Diagnostic[];
  readonly ok: boolean;

  hasErrors(): boolean;
  hasWarnings(): boolean;
  isOk(): boolean;
  toObject(): Record<string, any>;
  toJSON(): Record<string, any>;
  toDict(): Record<string, any>;
  toJson(): string;
  formatText(): string;
  formatJson(): string;
  toString(): string;
}

export declare function check(
  sourceOrPath: BatchPathInput,
  options?: CheckOptionsInput | null
): CheckResult;

export declare function generate(
  sourceOrPath: BatchPathInput,
  targetOrOptions?:
    | TargetName
    | (string & {})
    | CodegenOptions
    | GenerateCallOptionsInput
    | null,
  options?: CodegenOptions | GenerateCallOptionsInput | null
): GenerateResult;

export declare function inspectSchema(
  sourceOrPath: PathInput,
  options?: InspectOptionsInput | null
): ModuleInfo;

export declare function inspect_schema(
  sourceOrPath: PathInput,
  options?: InspectOptionsInput | null
): ModuleInfo;

export declare function jsonToKdl(
  jsonInput: string,
  nameOrOptions?: string | JsonToKdlOptionsInput | null
): string;

export declare function json_to_kdl(
  jsonInput: string,
  nameOrOptions?: string | JsonToKdlOptionsInput | null
): string;
