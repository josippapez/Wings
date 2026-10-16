/**
 * Whether a plugin page in the right sidebar is on screen. The sidebar keeps showing its last page while it is
 * closed (so the slide has content), so the page counts only while the sidebar is docked or peeking.
 */
export const sidebarPageShown = (showing: boolean, key: string, currentKey: string | undefined) => showing && key === currentKey;
