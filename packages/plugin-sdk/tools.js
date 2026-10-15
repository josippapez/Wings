// MCP tools written once, in TypeScript. A plugin's src/tools.ts exports `tools`, an object of tool() definitions
// keyed by tool name (lowercase letters, digits and _), and its main script calls registerTools(tools) to answer
// Claude. wings-plugin build writes the same definitions into `contributes.mcpTools` of wings-plugin.json, so the
// manifest and the code can't differ. A tool's input is a schema built with `s`, plain JSON Schema underneath. A
// call is checked against it before the handler runs, and the handler's input type is inferred from it.

/** A schema node: the JSON Schema in `schema`, and whether an object that holds it must have it. */
const node = (schema, required = true) => ({
  schema,
  required,
  optional: () => node(schema, false),
  describe: (description) => node({ ...schema, description }, required),
});

/** `object` without the keys whose value is undefined, so a limit that wasn't given isn't in the schema. */
const given = (object) => Object.fromEntries(Object.entries(object).filter(([, value]) => value !== undefined));

/** Builds the JSON Schema of a tool's input, and the type of what it accepts. */
export const s = {
  string: ({ minLength, maxLength } = {}) => node({ type: "string", ...given({ minLength, maxLength }) }),
  enum: (values) => node({ type: "string", enum: [...values] }),
  integer: ({ minimum, maximum } = {}) => node({ type: "integer", ...given({ minimum, maximum }) }),
  number: ({ minimum, maximum } = {}) => node({ type: "number", ...given({ minimum, maximum }) }),
  boolean: () => node({ type: "boolean" }),
  array: (item, { maxItems } = {}) => node({ type: "array", items: item.schema, ...given({ maxItems }) }),
  object(shape) {
    const names = Object.keys(shape);
    const required = names.filter((name) => shape[name].required);
    const properties = Object.fromEntries(names.map((name) => [name, shape[name].schema]));
    return node({ type: "object", properties, ...(required.length ? { required } : {}), additionalProperties: false });
  },
};

/** Defines one tool. A tool that takes nothing leaves out `input`. */
export const tool = (definition) => ({ ...definition, input: definition.input ?? s.object({}) });

const IS = {
  string: (value) => typeof value === "string",
  integer: Number.isInteger,
  number: Number.isFinite,
  boolean: (value) => typeof value === "boolean",
  array: Array.isArray,
  object: (value) => value !== null && typeof value === "object" && !Array.isArray(value),
};
const KIND = { string: "text", integer: "a whole number", number: "a number", boolean: "true or false", array: "a list", object: "an object" };
const num = (n) => n.toLocaleString("en");
/** JSON Schema counts characters, not UTF-16 units, so an emoji is one. */
const chars = (text) => [...text].length;

/**
 * Throws an Error that names the field and what it allows, unless `value` fits `schema`. `path` is where the value
 * sits, like `status` or `keys[1]`. `owner` is what an unknown field is said not to belong to: the tool at the top.
 */
function check(schema, value, path, owner = path) {
  const fail = (message) => {
    throw new Error(message);
  };
  if (schema.enum && !schema.enum.includes(value)) fail(`${path} must be one of ${schema.enum.join(", ")}.`);
  if (!IS[schema.type](value)) fail(path ? `${path} must be ${KIND[schema.type]}.` : "The input must be an object.");
  if (schema.type === "string") {
    const length = chars(value);
    if (length < schema.minLength) fail(`${path} needs at least ${num(schema.minLength)} character${schema.minLength === 1 ? "" : "s"}, this one has ${num(length)}.`);
    if (length > schema.maxLength) fail(`${path} is limited to ${num(schema.maxLength)} characters, this one has ${num(length)}.`);
  } else if (schema.type === "integer" || schema.type === "number") {
    if (value < schema.minimum) fail(`${path} must be at least ${num(schema.minimum)}, this one is ${num(value)}.`);
    if (value > schema.maximum) fail(`${path} must be at most ${num(schema.maximum)}, this one is ${num(value)}.`);
  } else if (schema.type === "array") {
    if (value.length > schema.maxItems) fail(`${path} takes up to ${num(schema.maxItems)} items, this one has ${num(value.length)}.`);
    value.forEach((item, i) => check(schema.items, item, `${path}[${i}]`));
  } else if (schema.type === "object") {
    const at = (name) => (path ? `${path}.${name}` : name);
    const names = Object.keys(schema.properties);
    const stray = Object.keys(value).filter((name) => !names.includes(name));
    if (stray.length) fail(`Unknown ${stray.length > 1 ? "fields" : "field"}: ${stray.map(at).join(", ")}. ${owner} takes ${names.join(", ") || "no input"}.`);
    for (const name of names) {
      if (value[name] !== undefined) check(schema.properties[name], value[name], at(name));
      else if (schema.required?.includes(name)) fail(`${at(name)} is required.`);
    }
  }
}

/**
 * Answers Claude's calls to each tool in `tools`, by name, with `wings.onTool`. The input is checked first, so a wrong
 * one throws an Error that tells Claude which field to fix, and `run` only ever sees an input that fits its schema.
 */
export function registerTools(tools, w = globalThis.wings) {
  for (const [name, { input, run }] of Object.entries(tools)) {
    w.onTool(name, (value, context) => {
      check(input.schema, value, "", name);
      return run(value, context);
    });
  }
}
