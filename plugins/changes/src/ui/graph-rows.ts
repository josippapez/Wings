import { dateOf, dateTimeOf } from "../lib/dates";
import type { Placed } from "../lib/graph-layout";
import { describeRef } from "../lib/log";
import { h } from "./graph-dom";
import "./ref-pill.css";

/** A ref as plugin-ui's `Badge` draws it, built without React. @param ref `kind:name` */
function refPill(ref: string) {
  const { head, label, title, tone } = describeRef(ref);
  return h("span", { class: head ? "w-badge ref-head" : "w-badge", "data-variant": "outline", "data-tone": tone, title }, label);
}

/** An empty row button, in its `li`. */
export function makeRow() {
  const b = h(
    "button",
    {},
    h("span", { class: "lane-gap" }),
    h("span", { class: "refs" }),
    h("span", { class: "w-text subject", "data-grow": true, "data-truncate": true }),
    h("span", { class: "w-text hash", "data-tone": "muted", "data-mono": true, "data-size": "tiny" }),
    h("span", { class: "w-text author", "data-tone": "muted", "data-truncate": true }),
    h("span", { class: "w-text date", "data-tone": "muted", "data-numeric": true }),
  ) as HTMLButtonElement;
  return { b, li: h("li", {}, b) };
}

/** Shows commit `i` in a row button, unless it already does. */
export function fill(b: HTMLButtonElement, i: number, c: Placed, selected: number) {
  b.classList.toggle("on", i === selected);
  if (b.dataset.i === String(i)) return;
  const [, refs, subject, hash, author, date] = [...b.children] as HTMLElement[];
  b.dataset.i = String(i);
  b.title = `${c.subject}\n${c.hash}\n${c.author}, ${dateTimeOf(c.at)}`;
  refs.replaceChildren(...c.refs.map(refPill));
  subject.textContent = c.subject;
  hash.textContent = c.hash.slice(0, 7);
  author.textContent = c.author;
  date.textContent = dateOf(c.at);
}
