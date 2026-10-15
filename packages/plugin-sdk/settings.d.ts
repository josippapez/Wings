/// <reference path="./wings.d.ts" />

export interface SettingOptions<T> {
  /** What `get` gives while nothing valid is stored. */
  default: T;
  /**
   * Gets the value out of whatever storage holds: return it as `T`, or `undefined` when it isn't one, and the default is
   * used. Leave it out and a stored value is trusted as it is.
   */
  check?: (value: unknown) => T | undefined;
  /** The `wings` object to use. Tests pass a fake frame's; a plugin leaves it out. */
  wings?: Pick<Wings, "storage" | "broadcast" | "onBroadcast">;
}

/** A setting in the plugin's storage, that every frame of the plugin can read and hear change. */
export interface Setting<T> {
  /** The storage key. */
  readonly key: string;
  /** The stored value, or the default when none is stored or `check` didn't accept it. A stored `null` counts as none. */
  get(): Promise<T>;
  /** Stores `value`, then calls this frame's `onChange` listeners with it and tells the other frames, which read it again. */
  set(value: T): Promise<void>;
  /** Calls `listener` with the new value each time any frame sets it. Returns a function that removes it. */
  onChange(listener: (value: T) => void): () => void;
}

/**
 * A setting, declared once with its type and default in a module that main, the pages and the settings page all import.
 * Keys are `wings.storage`'s. Give the type for a union, as `setting<"board" | "list">(...)`: a default alone makes it `string`.
 */
export function setting<T>(key: string, options: SettingOptions<T>): Setting<T>;
