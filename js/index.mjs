import cjs from "./index.cjs";

export const {
  __version__,
  version,
  check,
  generate,
  inspectSchema,
  inspect_schema,
  jsonToKdl,
  json_to_kdl,
  Diagnostic,
  CheckResult,
  CodegenOptions,
  GenerateResult,
  TypeInfo,
  FieldInfo,
  InitFieldInfo,
  CheckMethodInfo,
  PlaceholderInfo,
  RequestInfo,
  RestEndpointInfo,
  ErrorVariantInfo,
  StructInfo,
  FnParamInfo,
  FunctionInfo,
  JsonFieldInfo,
  JsonDefInfo,
  ExtensionOpInfo,
  ExtensionInfo,
  ModuleInfo,
} = cjs;

export default cjs;
