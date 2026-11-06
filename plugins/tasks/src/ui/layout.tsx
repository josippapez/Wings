// The Issues pages' own boxes: their HTML tags in the browser, styled by tasks.css, and wings-plugin-ui's in the native
// app, which draws no HTML.
import { createElement, type HTMLAttributes, type Ref } from "react";
import { Layout } from "./layout-native";

type Tag = "div" | "span" | "p" | "section" | "nav" | "aside" | "header" | "ul" | "li" | "form";
type Props = HTMLAttributes<HTMLElement> & { ref?: Ref<HTMLDivElement> };

function wrapper(tag: Tag) {
  return function Box(props: Props) {
    return WINGS_NATIVE ? <Layout tag={tag} className={props.className}>{props.children}</Layout> : createElement(tag, props);
  };
}

export const Box = wrapper("div");
export const Inline = wrapper("span");
export const Para = wrapper("p");
export const SectionBox = wrapper("section");
export const Nav = wrapper("nav");
export const Aside = wrapper("aside");
export const HeaderBox = wrapper("header");
export const Unordered = wrapper("ul");
export const Item = wrapper("li");
export const Form = wrapper("form");
