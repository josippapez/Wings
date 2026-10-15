// What the tests share: an empty store, and a create operation applied to one.
import { applyOp } from "./ops.ts";
import { emptyStore } from "./storage.ts";

export const fresh = (name = "web") => emptyStore(name);
export const create = (store, title, extra = {}, by = "you", now = 1) => applyOp(store, { kind: "create", title, ...extra }, by, now);
