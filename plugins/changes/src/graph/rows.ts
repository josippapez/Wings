import { h } from "./dom";
import type { Commit } from "./layout";

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const REF_KIND = { H: "HEAD", b: "Branch", r: "Remote branch", t: "Tag", s: "Stash" };

/** @param ref `kind:name` */
function refPill(ref: string) {
  const kind = ref[0];
  const name = ref.slice(2);
  const what = REF_KIND[kind as keyof typeof REF_KIND] ?? "Ref";
  return h("span", { class: `ref ref-${kind}`, title: kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${what} ${name}` }, kind === "t" ? `◆ ${name}` : name);
}

/** An empty row button, in its `li`. */
export function makeRow() {
  const b = h("button", {}, h("span", { class: "lane-gap" }), h("span", { class: "refs" }), h("span", { class: "grow ellipsis" }), h("span", { class: "hash muted" }), h("span", { class: "author ellipsis muted" }), h("span", { class: "date muted" })) as HTMLButtonElement;
  return { b, li: h("li", {}, b) };
}

/** Shows commit `i` in a row button, unless it already does. */
export function fill(b: HTMLButtonElement, i: number, c: Commit, selected: number) {
  b.classList.toggle("on", i === selected);
  if (b.dataset.i === String(i)) return;
  const [, refs, subject, hash, author, date] = [...b.children] as HTMLElement[];
  b.dataset.i = String(i);
  b.title = `${c.subject}\n${c.hash}\n${c.author}, ${full.format(c.at * 1000)}`;
  refs.replaceChildren(...c.refs.map(refPill));
  subject.textContent = c.subject;
  hash.textContent = c.hash.slice(0, 7);
  author.textContent = c.author;
  date.textContent = day.format(c.at * 1000);
}
