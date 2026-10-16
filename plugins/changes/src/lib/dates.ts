/** Dates as the History list and the graph show them. Every `at` is Unix seconds, as git prints it. */

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const dayTime = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
/** Like "15 Oct 2026". */
export const dateOf = (at: number) => day.format(at * 1000);
/** Like "15 Oct 2026, 14:03". */
export const dateTimeOf = (at: number) => dayTime.format(at * 1000);
