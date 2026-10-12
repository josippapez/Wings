/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { tabTitles } from "./tab-names.ts";

const t = (id: string, title: string, detail: string, name?: string) => ({ id, title, detail, name });

test("a tab nobody shares a title with stays as is", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/x"), t("b", "Claude", "wings")]), { a: "zsh", b: "Claude" });
});

test("same title and same folder: the first stays plain, the rest are numbered", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/Desktop/wings"), t("b", "zsh", "~/Desktop/wings"), t("c", "zsh", "~/Desktop/wings")]), { a: "zsh", b: "zsh 2", c: "zsh 3" });
});

test("same title, different folders: the shortest tail that differs", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/Desktop/wings"), t("b", "zsh", "~/Desktop/site")]), { a: "zsh · wings", b: "zsh · site" });
});

test("folders with the same last name need a longer tail", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/a/app"), t("b", "zsh", "~/b/app")]), { a: "zsh · a/app", b: "zsh · b/app" });
});

test("two in one folder and one elsewhere: the folder tail, then a number", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/x/wings"), t("b", "zsh", "~/x/wings"), t("c", "zsh", "~/x/site")]), { a: "zsh · wings", b: "zsh · wings 2", c: "zsh · site" });
});

test("a name you gave is never changed, and doesn't count as a clash", () => {
  assert.deepEqual(tabTitles([t("a", "zsh", "~/x", "build"), t("b", "zsh", "~/x"), t("c", "zsh", "~/x", "build")]), { a: "build", b: "zsh", c: "build" });
});
