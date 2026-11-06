/** Dates as the History list and the graph show them. Every `at` is Unix seconds, as git prints it. */

const native = typeof WINGS_NATIVE !== "undefined" && WINGS_NATIVE;
const day = native ? null : new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const dayTime = native ? null : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
/** Like "15 Oct 2026". */
export const dateOf = (at: number) => day ? day.format(at * 1000) : new Date(at * 1000).toISOString().slice(0, 10);
/** Like "15 Oct 2026, 14:03". */
export const dateTimeOf = (at: number) => dayTime ? dayTime.format(at * 1000) : new Date(at * 1000).toISOString();
