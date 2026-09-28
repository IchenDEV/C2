import { expect, test } from "bun:test";

import type { Part } from "../src/bridge";
import { turnsFromTranscript } from "../src/session/turns";

test("legacy plan-only history produces no empty turn", () => {
  expect(
    turnsFromTranscript([["agent", { kind: "plan", entries: ["old item"] }]])
  ).toEqual([]);
});

test("legacy plans do not replace or reappear in normal conversation", () => {
  const entries: [string, Part][] = [
    ["user", { kind: "text", text: "Keep working" }],
    [
      "agent",
      {
        kind: "plan",
        entries: [{ content: "Retired plan", status: "pending" }],
      },
    ],
    ["agent", { kind: "text", text: "Current answer" }],
  ];
  const turns = turnsFromTranscript(entries);
  expect(turns).toHaveLength(1);
  expect(turns[0].prompt).toBe("Keep working");
  expect(turns[0].text).toBe("Current answer");
  expect(turns[0]).not.toHaveProperty("plan");
});
