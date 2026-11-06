// The work item page's own boxes: its HTML tags in the browser, styled by item.css, and wings-plugin-ui's in the
// native app, which draws no HTML.
import { createElement, type HTMLAttributes, type ButtonHTMLAttributes, type ReactNode } from "react";
import * as native from "./layout-native";

function wrapper(tag: "div" | "header" | "article" | "section" | "ul" | "ol" | "li" | "span" | "strong" | "em" | "h1" | "h3") {
  return function Layout(props: HTMLAttributes<HTMLElement>) {
    return WINGS_NATIVE ? <native.Layout tag={tag} {...props} /> : createElement(tag, props);
  };
}

export const Box = wrapper("div");
export const HeaderBox = wrapper("header");
export const Article = wrapper("article");
export const SectionBox = wrapper("section");
export const Unordered = wrapper("ul");
export const Ordered = wrapper("ol");
export const Item = wrapper("li");
export const Inline = wrapper("span");
export const Strong = wrapper("strong");
export const Em = wrapper("em");
export const H1 = wrapper("h1");
export const H3 = wrapper("h3");

/** A row that opens something: its click handler gets no event. */
export type LinkButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick"> & { onClick?: () => void; children?: ReactNode };

export function LinkButton(props: LinkButtonProps) {
  return WINGS_NATIVE ? <native.LinkButton {...props} /> : createElement("button", props);
}
