// Renders the HTML Azure DevOps keeps for a description or a comment, without ever putting that HTML in the page.
// DOMParser makes an inert document (no scripts run, nothing loads), and this walks it and builds React elements
// from a short list of tags. Attributes aren't copied, so there is no `style`, `on*`, `src` or `srcdoc` to carry
// anything in. The one attribute that is read is a link's `href`, and only to open an Azure DevOps address in the
// browser from a click. Text goes in as React text, which React escapes.

import { createElement, type ReactNode } from "react";
import "./html.css";

/** Tags drawn as themselves. */
const KEEP = new Set(["p", "div", "ul", "ol", "li", "blockquote", "pre", "code", "b", "strong", "i", "em", "u", "s", "strike", "del", "thead", "tbody", "tr", "td", "th", "hr"]);
/** Headings are drawn as a bold paragraph: the page decides how big a heading is, not the comment. */
const HEADINGS = new Set(["h1", "h2", "h3", "h4", "h5", "h6"]);
/** Tags dropped with everything inside them. */
const DROP = new Set(["script", "style", "iframe", "object", "embed", "link", "meta", "svg", "math", "template", "noscript", "form", "input", "button", "select", "textarea", "head", "title"]);
/** What `permissions.openUrl` allows, so a link to anywhere else stays text. */
const OPENABLE = "https://dev.azure.com/";
const MAX_DEPTH = 40;

function link(el: Element, children: ReactNode[], key: number): ReactNode {
  let href: string | null = null;
  try {
    const url = new URL(el.getAttribute("href") ?? "");
    if (url.protocol === "https:" && url.href.startsWith(OPENABLE)) href = url.href;
  } catch {
    // Not a URL: a mention's "#", or a relative link.
  }
  if (!href) return createElement("span", { key }, ...children);
  return createElement(
    "a",
    {
      key,
      href,
      onClick: (e: { preventDefault(): void }) => {
        e.preventDefault();
        void wings.openUrl(href).catch(() => {});
      },
    },
    ...children,
  );
}

function convert(node: Node, key: number, depth: number): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (node.nodeType !== Node.ELEMENT_NODE || depth > MAX_DEPTH) return null;
  const el = node as Element;
  const tag = el.localName;
  if (DROP.has(tag)) return null;
  if (tag === "br") return createElement("br", { key });
  if (tag === "img") return createElement("span", { key, className: "html-image" }, `[image${el.getAttribute("alt") ? `: ${el.getAttribute("alt")}` : ""}]`);
  const children = [...el.childNodes].map((child, i) => convert(child, i, depth + 1));
  if (tag === "a") return link(el, children, key);
  if (HEADINGS.has(tag)) return createElement("p", { key, className: "html-heading" }, ...children);
  // A wide table scrolls inside its own box instead of pushing the page sideways.
  if (tag === "table") return createElement("div", { key, className: "html-table" }, createElement("table", null, ...children));
  if (KEEP.has(tag)) return createElement(tag, { key }, ...children);
  // Anything else (span, font, section, an unknown tag) keeps its text and loses its markup.
  return createElement("span", { key }, ...children);
}

export function Html({ html }: { html: string }) {
  const doc = new DOMParser().parseFromString(html, "text/html");
  return <div className="html">{[...doc.body.childNodes].map((child, i) => convert(child, i, 0))}</div>;
}
