// Where a gauge or a figure turns amber, then red. The strip under a pane, Wings' limit alerts and the sidebar's gauges
// all read these, so they can't drift apart.

/** The context window, as claude-hud has it. */
export const CONTEXT_WARN = 70;
export const CONTEXT_CRITICAL = 85;
/** Usage limits and memory. */
export const LIMIT_WARN = 75;
export const LIMIT_CRITICAL = 90;
