// The browser parses the HTML with DOMParser; the native app, which has no DOM, tokenizes it.
import { Html as BrowserHtml } from "./html-browser";
import { Html as NativeHtml } from "./html-native";

export const Html = WINGS_NATIVE ? NativeHtml : BrowserHtml;
