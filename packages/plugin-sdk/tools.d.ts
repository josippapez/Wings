/// <reference path="./wings.d.ts" />
/** What a tool's handler is told besides its input: who called, as `wings.onTool` gives it. */
export type ToolContext = WingsToolContext;

/**
 * A schema built with `s`: plain JSON Schema in `schema`, and in `T` the type of a value that fits it. `R` is whether
 * an object that holds it as a property must have it.
 */
export interface Schema<T = unknown, R extends boolean = true> {
  /** The JSON Schema, as `inputSchema` in the manifest holds it. */
  readonly schema: Record<string, unknown>;
  readonly required: R;
  /** Type only: it is never set, and carries `T` for `Infer`. */
  readonly _type?: T;
  /** A property the caller may leave out. */
  optional(): Schema<T, false>;
  /** The text Claude reads about this value. */
  describe(text: string): Schema<T, R>;
}

/** The type of a value that fits `S`, a schema built with `s`. */
export type Infer<S> = S extends Schema<infer T, boolean> ? T : never;

/** The `format` names `s.string` takes. */
export type StringFormat = "uuid" | "date" | "date-time";

type Shape = Record<string, Schema<unknown, boolean>>;
type ObjectOf<S extends Shape> = {
  [K in keyof S as S[K] extends Schema<unknown, false> ? never : K]: Infer<S[K]>;
} & {
  [K in keyof S as S[K] extends Schema<unknown, false> ? K : never]?: Infer<S[K]>;
} extends infer O
  ? { [K in keyof O]: O[K] }
  : never;

/** Builds the JSON Schema of a tool's input, and the type of what it accepts. */
export const s: {
  /**
   * Text. Limits count characters, as JSON Schema does: an emoji is one. `format` is a JSON Schema format name that the
   * value must be: `uuid`, `date` (YYYY-MM-DD) or `date-time` (RFC 3339). `pattern` is the source of a regular
   * expression, run as a Unicode RegExp, that the value must match; anchor it with ^ and $ to match all of it.
   */
  string(options?: { minLength?: number; maxLength?: number; format?: StringFormat; pattern?: string }): Schema<string>;
  /** One of `values`. */
  enum<const V extends readonly string[]>(values: V): Schema<V[number]>;
  integer(limits?: { minimum?: number; maximum?: number }): Schema<number>;
  number(limits?: { minimum?: number; maximum?: number }): Schema<number>;
  boolean(): Schema<boolean>;
  array<T>(item: Schema<T, boolean>, limits?: { maxItems?: number }): Schema<T[]>;
  /** An object with exactly these properties. Each is required unless it says `optional()`. */
  object<S extends Shape>(shape: S): Schema<ObjectOf<S>>;
};

export interface Tool<I extends object = object> {
  /** What Claude reads to decide when to call the tool. */
  readonly description: string;
  readonly input: Schema<I>;
  run(input: I, context: ToolContext): unknown;
}

/**
 * Defines a tool. `run` gets the input typed from `input`, once it has been checked against it, and returns a string
 * or a JSON value, or a promise of one; a thrown error is sent back to Claude as a failed call. A tool that takes
 * nothing leaves out `input`.
 */
export function tool<I extends object>(definition: { description: string; input: Schema<I>; run(input: I, context: ToolContext): unknown }): Tool<I>;
export function tool(definition: { description: string; run(input: {}, context: ToolContext): unknown }): Tool<{}>;

/**
 * Answers Claude's calls to each tool in `tools`, by name, with `wings.onTool`. The input is checked first, so a wrong
 * one throws an Error that tells Claude which field to fix, and `run` only ever sees an input that fits its schema.
 * Call it in `main` with the `tools` that `src/tools.ts` exports.
 */
export function registerTools(tools: Record<string, Tool>, w?: Pick<Wings, "onTool">): void;
