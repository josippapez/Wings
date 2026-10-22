// The Sidebar order setting: how the project section sorts the issues no agent is working on. Saved by the settings page, read by main.

import { setting } from "wings-plugin-sdk/settings";

export type SectionOrder = "status" | "date";
export const SECTION_ORDERS: { value: SectionOrder; label: string }[] = [
  { value: "status", label: "Status" },
  { value: "date", label: "Latest change" },
];

export const sectionOrder = setting<SectionOrder>("sectionOrder", { default: "status", check: (saved) => SECTION_ORDERS.find((o) => o.value === saved)?.value });
