/** Only persisted assistant facts produce desktop alerts; ordinary TurnEnded is not completion. */
export function assistantNotification(payload: unknown) {
  if (payload == null || typeof payload !== "object") return null;
  if (
    !("event" in payload) ||
    payload.event !== "assistant_alert" ||
    !("id" in payload) ||
    typeof payload.id !== "string" ||
    !("title" in payload) ||
    typeof payload.title !== "string" ||
    !("body" in payload) ||
    typeof payload.body !== "string"
  )
    return null;
  return {
    title: payload.title.slice(0, 200),
    body: payload.body.slice(0, 600),
  };
}

/** A missing desktop channel must not interrupt renderer events or change persisted Inbox state. */
export function showAssistantNotification(
  payload: unknown,
  show: (notification: { title: string; body: string }) => void,
  unavailable: (error: unknown) => void
) {
  const notification = assistantNotification(payload);
  if (!notification) return;
  try {
    show(notification);
  } catch (error) {
    unavailable(error);
  }
}
