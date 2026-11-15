export class RtJsonError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RtJsonError";
  }
}

export class RtJsonPathError extends RtJsonError {
  constructor(message: string) {
    super(message);
    this.name = "RtJsonPathError";
  }
}

export class RtJsonFieldMissingError extends RtJsonError {
  constructor(message: string) {
    super(message);
    this.name = "RtJsonFieldMissingError";
  }
}

export class RtJsonSchemaError extends RtJsonError {
  constructor(message: string) {
    super(message);
    this.name = "RtJsonSchemaError";
  }
}

export function rtTypeName(v: unknown): string {
  return v === null ? "null" : Array.isArray(v) ? "array" : typeof v;
}

export function rtExpectObject(data: unknown, target: string = ""): Record<string, any> {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    const prefix = target ? `${target}: ` : "";
    throw new RtJsonSchemaError(`${prefix}Expected object, got ${rtTypeName(data)}`);
  }
  return data as Record<string, any>;
}

export function rtExpectArray(data: unknown, target: string = ""): any[] {
  if (!Array.isArray(data)) {
    const prefix = target ? `${target}: ` : "";
    throw new RtJsonSchemaError(`${prefix}Expected array, got ${rtTypeName(data)}`);
  }
  return data;
}

export function rtSplitGjsonPath(path: string): Array<{ raw: string; key: string; escaped: boolean }> {
  const segs: Array<{ raw: string; key: string; escaped: boolean }> = [];
  let raw = "";
  let key = "";
  let escaped = false;
  for (let i = 0; i < path.length; i++) {
    const ch = path[i];
    if (
      ch === "\\" &&
      i + 1 < path.length &&
      (path[i + 1] === "." || path[i + 1] === "\\" || path[i + 1] === "#")
    ) {
      raw += ch + path[i + 1];
      key += path[i + 1];
      escaped = true;
      i++;
    } else if (ch === ".") {
      segs.push({ raw, key, escaped });
      raw = "";
      key = "";
      escaped = false;
    } else {
      raw += ch;
      key += ch;
    }
  }
  segs.push({ raw, key, escaped });
  return segs;
}

export function rtResolveDotpath(
  data: any,
  path: string,
  isOptional: boolean = false,
  target: string = "",
): any {
  const prefix = target ? `${target}: ` : "";
  let current: any = data;
  const segs = rtSplitGjsonPath(path);
  for (let i = 0; i < segs.length; i++) {
    const seg = segs[i];
    if (current == null) {
      if (isOptional) return null;
      throw new RtJsonPathError(
        `${prefix}Cannot traverse segment '${seg.key}' on null object in path '${path}'`,
      );
    }
    if (!seg.escaped && seg.raw === "#") {
      if (!Array.isArray(current)) {
        if (isOptional) return null;
        throw new RtJsonPathError(
          `${prefix}Expected list for '#' in path '${path}', got ${rtTypeName(current)}`,
        );
      }
      if (i === segs.length - 1) return current.length;
      const subpath = segs
        .slice(i + 1)
        .map((s) => s.raw)
        .join(".");
      const out: any[] = [];
      for (const item of current) {
        const v = rtResolveDotpath(item, subpath, true, target);
        if (v != null) out.push(v);
      }
      return out;
    }
    if (!seg.escaped && /^\d+$/.test(seg.raw)) {
      const idx = parseInt(seg.raw, 10);
      if (!Array.isArray(current)) {
        if (isOptional) return null;
        throw new RtJsonPathError(
          `${prefix}Expected list for index '${idx}' in path '${path}', got ${rtTypeName(current)}`,
        );
      }
      if (idx < 0 || idx >= current.length) {
        if (isOptional) return null;
        throw new RtJsonPathError(
          `${prefix}Index ${idx} out of bounds (len=${current.length}) in path '${path}'`,
        );
      }
      current = current[idx];
    } else if (typeof current === "object" && !Array.isArray(current)) {
      if (!Object.prototype.hasOwnProperty.call(current, seg.key)) {
        if (isOptional) return null;
        throw new RtJsonPathError(`${prefix}Missing key '${seg.key}' in path '${path}'`);
      }
      current = current[seg.key];
    } else {
      if (isOptional) return null;
      throw new RtJsonPathError(
        `${prefix}Cannot access key '${seg.key}' on non-dict ${rtTypeName(current)} in path '${path}'`,
      );
    }
  }
  return current;
}

export function rtJsonField(
  data: any,
  key: string,
  target: string,
  isOptional: boolean = false,
  isOmitempty: boolean = false,
): any {
  if (!Object.prototype.hasOwnProperty.call(data, key)) {
    if (isOmitempty) return undefined;
    if (isOptional) return null;
    const canonical = target.includes(".") ? target.slice(target.indexOf(".") + 1) : key;
    const qual = target.includes(".") ? target : `${target}.${canonical}`;
    throw new RtJsonFieldMissingError(
      `${qual}: Required JSON field '${key}' (mapped to '${canonical}') is missing`,
    );
  }
  const val = data[key];
  if (val == null) {
    if (isOmitempty) return undefined;
    if (isOptional) return null;
    const canonical = target.includes(".") ? target.slice(target.indexOf(".") + 1) : key;
    const qual = target.includes(".") ? target : `${target}.${canonical}`;
    throw new RtJsonFieldMissingError(
      `${qual}: Field '${key}' is null, but '${canonical}' is not nullable`,
    );
  }
  return val;
}

export function rtJsonPath(
  data: any,
  path: string,
  target: string,
  isOptional: boolean = false,
  isOmitempty: boolean = false,
): any {
  const val = rtResolveDotpath(data, path, isOptional || isOmitempty, target);
  if (val == null) {
    if (isOmitempty) return undefined;
    if (isOptional) return null;
    const canonical = target.slice(target.indexOf(".") + 1);
    throw new RtJsonFieldMissingError(
      `${target}: Field '${path}' is null, but '${canonical}' is not nullable`,
    );
  }
  return val;
}
