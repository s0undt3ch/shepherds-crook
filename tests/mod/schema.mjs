// A small JSON Schema checker for the status schema: type, const, enum, required, properties,
// additionalProperties, items, pattern, minimum, minLength, anyOf, oneOf and local $ref. A keyword it does not
// know throws, so a schema using more than this cannot pass by accident.
const KNOWN = new Set([
  "$schema", "$id", "$defs", "$ref", "title", "description", "type", "const", "enum", "required", "properties",
  "additionalProperties", "items", "pattern", "minimum", "minLength", "minItems", "anyOf", "oneOf", "default", "examples",
]);

const typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : Number.isInteger(v) ? "integer" : typeof v);
const isType = (v, t) => (t === "number" ? typeof v === "number" : typeOf(v) === t || (t === "integer" && typeOf(v) === "integer"));

export function validate(schema, value, node = schema, path = "$") {
  const errors = [];
  if (node === true) return errors;
  if (node === false) return [`${path}: not allowed`];
  for (const key of Object.keys(node)) if (!KNOWN.has(key)) throw new Error(`unsupported schema keyword: ${key}`);
  if (node.$ref !== undefined) {
    const target = node.$ref.replace(/^#\//, "").split("/").reduce((n, k) => n?.[k], schema);
    if (target === undefined) throw new Error(`unresolved $ref: ${node.$ref}`);
    return validate(schema, value, target, path);
  }
  if (node.type !== undefined) {
    const types = [].concat(node.type);
    if (!types.some((t) => isType(value, t))) return [`${path}: expected ${types.join("|")}, got ${typeOf(value)}`];
  }
  if (node.const !== undefined && JSON.stringify(node.const) !== JSON.stringify(value)) errors.push(`${path}: not ${JSON.stringify(node.const)}`);
  if (node.enum && !node.enum.some((e) => JSON.stringify(e) === JSON.stringify(value))) errors.push(`${path}: not one of ${JSON.stringify(node.enum)}`);
  if (typeof value === "string") {
    if (node.pattern && !new RegExp(node.pattern, "u").test(value)) errors.push(`${path}: does not match ${node.pattern}`);
    if (node.minLength !== undefined && value.length < node.minLength) errors.push(`${path}: too short`);
  }
  if (typeof value === "number" && node.minimum !== undefined && value < node.minimum) errors.push(`${path}: below ${node.minimum}`);
  if (Array.isArray(value)) {
    if (node.minItems !== undefined && value.length < node.minItems) errors.push(`${path}: too few items`);
    if (node.items) value.forEach((v, i) => errors.push(...validate(schema, v, node.items, `${path}[${i}]`)));
  }
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    for (const k of node.required ?? []) if (!(k in value)) errors.push(`${path}: missing ${k}`);
    const props = node.properties ?? {};
    for (const [k, v] of Object.entries(value)) {
      if (k in props) errors.push(...validate(schema, v, props[k], `${path}.${k}`));
      else if (node.additionalProperties === false) errors.push(`${path}: unexpected ${k}`);
      else if (typeof node.additionalProperties === "object") errors.push(...validate(schema, v, node.additionalProperties, `${path}.${k}`));
    }
  }
  if (node.anyOf && !node.anyOf.some((n) => validate(schema, value, n, path).length === 0)) errors.push(`${path}: matches no anyOf branch`);
  if (node.oneOf && node.oneOf.filter((n) => validate(schema, value, n, path).length === 0).length !== 1) errors.push(`${path}: does not match exactly one oneOf branch`);
  return errors;
}
