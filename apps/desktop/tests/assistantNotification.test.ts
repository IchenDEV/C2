import { expect, test } from "bun:test";

import {
  assistantNotification,
  showAssistantNotification,
} from "../src/assistant/notification";

test("only durable assistant alerts reach the native notification adapter", () => {
  expect(
    assistantNotification({ event: "turn_ended", session: "s1" })
  ).toBeNull();
  expect(
    assistantNotification({ event: "assistant_alert", title: "Done" })
  ).toBeNull();
  expect(assistantNotification(null)).toBeNull();
  expect(
    assistantNotification({
      event: "assistant_alert",
      id: "accepted:goal:v2",
      title: "Accepted report",
      body: "Actual checks and files",
    })
  ).toEqual({ title: "Accepted report", body: "Actual checks and files" });
  const alert = assistantNotification({
    event: "assistant_alert",
    id: "question:q1",
    title: "a".repeat(300),
    body: "b".repeat(700),
  });
  expect(alert?.title.length).toBe(200);
  expect(alert?.body.length).toBe(600);
});

test("desktop rejection preserves the durable alert and subsequent renderer delivery", () => {
  const alert = {
    event: "assistant_alert",
    id: "accepted:goal:v2",
    title: "Accepted report",
    body: "Actual checks and files",
    read: false,
    desktop: "unknown",
  };
  const denied = new Error("Notifications denied");
  const failures: unknown[] = [];
  const rendererEvents: unknown[] = [];
  showAssistantNotification(
    alert,
    () => {
      throw denied;
    },
    (error) => failures.push(error)
  );
  rendererEvents.push(alert);
  expect(failures).toEqual([denied]);
  expect(rendererEvents).toEqual([alert]);
  expect(alert.read).toBe(false);
  expect(alert.desktop).toBe("unknown");
  showAssistantNotification(
    { event: "turn_ended" },
    () => {
      throw denied;
    },
    (error) => failures.push(error)
  );
  expect(failures).toHaveLength(1);
});
