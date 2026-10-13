// node --test: what the ui components render, as markup.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement as h } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { Avatar, Badge, Button, Card, Page, Toolbar, Chips, Combobox, Dot, Empty, Heading, Row, Stack, Title } from "./ui.js";

const html = (element) => renderToStaticMarkup(element);

test("Dot takes a colour from data as --w-color, and only then", () => {
  assert.match(html(h(Dot, { color: "#cc293d" })), /<span class="w-dot" style="--w-color:#cc293d" data-color="" aria-hidden="true">/);
  assert.equal(html(h(Dot, { tone: "danger" })), '<span class="w-dot" data-tone="danger" aria-hidden="true"></span>');
  assert.doesNotMatch(html(h(Dot, { tone: "danger", color: "#cc293d" })), /data-tone/);
});

test("Chips pass each option's other props to its button, and mark the one that's on", () => {
  const out = html(h(Chips, { label: "Show", value: "a", onChange() {}, options: [{ value: "a", label: "A", id: "chip-a" }, { value: "b", label: "B", title: "Bee" }] }));
  assert.match(out, /<div[^>]*role="group"[^>]*aria-label="Show"/);
  assert.match(out, /<button(?=[^>]*id="chip-a")(?=[^>]*aria-pressed="true")[^>]*>A<\/button>/);
  assert.match(out, /<button(?=[^>]*title="Bee")(?=[^>]*aria-pressed="false")[^>]*>B<\/button>/);
});

test("Title and Heading pick their level", () => {
  assert.equal(html(h(Title, { size: "large" }, "Sprint 12")), '<h1 class="w-title" data-size="large">Sprint 12</h1>');
  assert.equal(html(h(Title, { as: "h2" }, "Sign in")), '<h2 class="w-title">Sign in</h2>');
  assert.equal(html(h(Heading, { level: 3 }, "Files")), '<h3 class="w-heading">Files</h3>');
});

test("Title truncate sets the data attribute the CSS cuts the line by, and leaves it off otherwise", () => {
  assert.equal(html(h(Title, { truncate: true }, "Sprint 12")), '<h1 class="w-title" data-truncate="">Sprint 12</h1>');
  assert.equal(html(h(Title, {}, "Sprint 12")), '<h1 class="w-title">Sprint 12</h1>');
});

test("Combobox is a closed combobox with its text and no list until it opens", () => {
  const out = html(h(Combobox, { id: "p", label: "Project", value: "Wings", options: [{ value: "1", label: "Wings" }], onPick() {} }));
  assert.match(out, /<input[^>]*role="combobox"[^>]*aria-expanded="false"[^>]*aria-autocomplete="list"/);
  assert.match(out, /<input[^>]*aria-label="Project"[^>]*class="w-input"[^>]*value="Wings"/);
  assert.doesNotMatch(out, /role="option"|role="listbox"/);
});

test("Card, Stack and Row can be a form", () => {
  assert.equal(html(h(Card, { as: "form", compact: true })), '<form class="w-card" data-size="compact"></form>');
  assert.equal(html(h(Stack, { as: "form", gap: "l" })), '<form class="w-stack" data-gap="l"></form>');
  assert.equal(html(h(Row, { as: "form", wrap: true })), '<form class="w-row" data-wrap=""></form>');
});

/** The `data-*` attribute `name` of the first element `out` renders, or null. */
const attr = (out, name) => out.match(new RegExp(` data-${name}="([^"]*)"`))?.[1] ?? null;

test("Button takes shadcn/ui's variants and sizes, and is the default one without", () => {
  for (const variant of ["default", "secondary", "outline", "ghost", "destructive", "link"]) assert.equal(attr(html(h(Button, { variant }, "Go")), "variant"), variant);
  assert.equal(attr(html(h(Button, null, "Go")), "variant"), "default");
  for (const size of ["default", "xs", "sm", "lg", "icon", "icon-xs", "icon-sm", "icon-lg"]) assert.equal(attr(html(h(Button, { size }, "Go")), "size"), size);
  assert.equal(attr(html(h(Button, null, "Go")), "size"), "default");
});

test("Button is a button that doesn't submit a form unless you say so", () => {
  assert.match(html(h(Button, null, "Go")), /<button[^>]*type="button"/);
  assert.match(html(h(Button, { type: "submit" }, "Go")), /<button[^>]*type="submit"/);
});

test("Badge takes shadcn/ui's variants and Wings' tones", () => {
  for (const variant of ["default", "secondary", "destructive", "outline", "ghost", "link"]) assert.equal(attr(html(h(Badge, { variant }, "x")), "variant"), variant);
  assert.equal(attr(html(h(Badge, null, "x")), "variant"), "default");
  assert.equal(html(h(Badge, { variant: "outline", tone: "info" }, "x")), '<span class="w-badge" data-variant="outline" data-tone="info">x</span>');
});

test("Empty has a title and a description, and shows no description line without one", () => {
  assert.match(html(h(Empty, { title: "Nothing", description: "Try again" })), /<p class="w-empty-title">Nothing<\/p><p class="w-empty-description">Try again<\/p>/);
  assert.doesNotMatch(html(h(Empty, { title: "Nothing" })), /w-empty-description/);
});

test("Page puts its toolbar over a body that is flush when it lays itself out", () => {
  const out = html(h(Page, { toolbar: h(Toolbar, { title: "Tasks", actions: "X" }, "filters"), flush: true }, "body"));
  assert.match(out, /^<div class="w-page"><header class="w-toolbar"><h1 class="w-title w-toolbar-title" data-truncate="">Tasks<\/h1><div class="w-toolbar-main">filters<\/div><div class="w-toolbar-actions">X<\/div><\/header><div class="w-page-body" data-flush="">body<\/div><\/div>$/);
  assert.equal(html(h(Page, {}, "body")), '<div class="w-page"><div class="w-page-body">body</div></div>');
});

test("Avatar shows initials on a colour that depends only on the name", () => {
  const out = (name, props) => html(h(Avatar, { name, ...props }));
  assert.match(out("Ada Lovelace"), /^<span class="w-avatar" style="--w-hue:\d+" aria-hidden="true">AL<\/span>$/);
  assert.match(out("ada-bot[bot]"), />AB</);
  assert.match(out("ada"), />A</);
  assert.match(out("  "), />\?</);
  assert.equal(out("Ada Lovelace"), out("Ada Lovelace"));
  assert.notEqual(out("Ada Lovelace"), out("Grace Hopper"));
  assert.match(out("Ada", { label: "Ada, author", size: "small" }), /data-size="small" role="img" aria-label="Ada, author"/);
});
