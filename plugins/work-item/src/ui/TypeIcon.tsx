import { TypeIcon as BrowserTypeIcon } from "./type-icon-browser";
import { TypeIcon as NativeTypeIcon } from "./type-icon-native";

export const TypeIcon = WINGS_NATIVE ? NativeTypeIcon : BrowserTypeIcon;
