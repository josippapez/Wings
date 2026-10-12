/**
 * Commands other parts of the UI (the command palette, a menu, a hotkey) call to open the notification list or
 * the review's Send to agent picker. The components that own them listen for these events.
 */
export const OPEN_NOTIFICATIONS = "wings:open-notifications";
export const OPEN_REVIEW_SEND = "wings:open-review-send";

/** Opens the bell's list of agent events. */
export const openNotifications = () => window.dispatchEvent(new Event(OPEN_NOTIFICATIONS));

/** Opens the diff viewer's Send to agent picker; does nothing unless a diff with review comments is open. */
export const openReviewSend = () => window.dispatchEvent(new Event(OPEN_REVIEW_SEND));
