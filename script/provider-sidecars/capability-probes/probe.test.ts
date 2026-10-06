import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// Contract test over the committed report; regenerate with `bun probe.ts` (needs network, no keys).
const report = JSON.parse(readFileSync(join(import.meta.dir, "report.json"), "utf8"));

test("every claim still matched its pinned official evidence", () => {
  const bad = report.checks.filter((c: any) => !c.evidenceOk).map((c: any) => c.id);
  expect(bad).toEqual([]);
});

test("all five provider identities and five topics are covered; Cursor approval is not over-claimed", () => {
  const providers = new Set(report.checks.map((c: any) => c.provider));
  expect([...providers].sort()).toEqual(["claude", "codex", "cursor", "opencode-v1", "opencode-v2"]);
  const topics = new Set(report.checks.map((c: any) => c.topic));
  for (const t of ["approval pause/reply/deny", "tool restriction", "steering", "abort terminal"]) expect(topics.has(t)).toBe(true);
  const cursorApproval = report.checks.filter((c: any) => c.provider === "cursor" && c.topic === "approval pause/reply/deny");
  expect(cursorApproval.every((c: any) => c.verdict === "unsupported")).toBe(true);
  expect(report.honesty).toContain("NOT runtime end-to-end");
});
