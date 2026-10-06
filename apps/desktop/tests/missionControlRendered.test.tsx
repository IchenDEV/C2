// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

import {
  activateDom,
  button,
  click,
  dom,
  mount,
  restoreDom,
  waitFor,
} from "./domTestHarness";

activateDom();
const { MissionControlPage } = await import("../src/sidebar/MissionControl");
const { I18nProvider } = await import("../src/i18n");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

let counter = 0;

function session(id, activityKind = "idle") {
  const state =
    activityKind === "awaiting_input"
      ? { kind: "awaiting_input", turn_id: "t1", pending: [] }
      : activityKind === "failed"
        ? { kind: "failed", reason: "provider_error", message: "boom" }
        : activityKind === "running"
          ? { kind: "running", turn_id: "t1" }
          : { kind: "idle" };
  return {
    id,
    title: `Session ${id}`,
    title_origin: "default",
    pinned: false,
    provider: "claude_code",
    model: null,
    cwd: "/tmp/repo",
    worktree_path: null,
    project_path: "/tmp/repo",
    permission_mode: "ask",
    sandbox_policy: "workspace_write",
    acp_session_id: null,
    memory_read: "inherit",
    memory_write: "inherit",
    created_at: (counter += 1),
    activity: { revision: 1, state },
  };
}

function renderDialog(overrides = {}) {
  const calls = { selected: [], reviewed: [], closed: 0 };
  const props = {
    sessions: [session("alpha", "awaiting_input"), session("beta", "idle")],
    runningSessions: new Set(),
    contextWindows: { alpha: { usedTokens: 50_000, contextWindow: 200_000 } },
    sceneBySession: new Map([["alpha", "builtin:develop"]]),
    onSelect: (id) => calls.selected.push(id),
    onReview: (id) => calls.reviewed.push(id),
    onClose: () => {
      calls.closed += 1;
    },
    fetchStat: async () => ({ files: 3, additions: 12, deletions: 4 }),
    ...overrides,
  };
  const rendered = mount(
    <I18nProvider>
      <MissionControlPage {...props} />
    </I18nProvider>
  );
  click(button(dom.document.body, "Execution"));
  return { rendered, calls };
}

describe("MissionControlPage", () => {
  test("the close button dismisses without selecting or reviewing a task", async () => {
    activateDom();
    const { calls } = renderDialog();
    await waitFor(() =>
      expect(button(dom.document.body, "Close")).not.toBeNull()
    );
    click(button(dom.document.body, "Close"));
    expect(calls.closed).toBe(1);
    expect(calls.selected).toEqual([]);
    expect(calls.reviewed).toEqual([]);
  });

  test("renders one row per session with title, scene pill, and context", async () => {
    activateDom();
    renderDialog();
    const { body } = dom.document;
    await waitFor(() => {
      expect(body.textContent).toContain("Session alpha");
      expect(body.textContent).toContain("Session beta");
    });
    // Scene pill uses the reference's short name; context is a rounded percentage.
    expect(body.textContent).toContain("develop");
    expect(body.textContent).toContain("25%");
    // The self-fetching diff stat cell resolves to "+a −d · n files".
    await waitFor(() => {
      expect(body.textContent).toContain("+12");
      expect(body.textContent).toContain("−4");
      expect(body.textContent).toContain("3 files");
    });
  });

  test("review click calls onSelect and onReview for that session, then closes", async () => {
    activateDom();
    const { calls } = renderDialog();
    const { body } = dom.document;
    await waitFor(() => {
      expect(body.textContent).toContain("Session alpha");
    });
    // Rows are urgency-ordered: awaiting-input alpha renders before idle beta.
    const review = button(body, "Review");
    click(review);
    expect(calls.selected).toEqual(["alpha"]);
    expect(calls.reviewed).toEqual(["alpha"]);
    expect(calls.closed).toBe(1);
  });

  test("the full row primary action is a semantic button", async () => {
    activateDom();
    const { calls } = renderDialog();
    const { body } = dom.document;
    await waitFor(() => expect(button(body, "Session alpha")).not.toBeNull());
    click(button(body, "Session alpha"));
    expect(calls.selected).toEqual(["alpha"]);
    expect(calls.reviewed).toEqual([]);
    expect(calls.closed).toBe(1);
  });

  test("shows the empty line when there are no sessions", async () => {
    activateDom();
    renderDialog({ sessions: [] });
    const { body } = dom.document;
    await waitFor(() => {
      expect(body.textContent).toContain("No sessions yet.");
    });
  });

  test("a session without a checkout diff renders an em dash", async () => {
    activateDom();
    renderDialog({
      sessions: [session("gone", "idle")],
      sceneBySession: new Map(),
      contextWindows: {},
      fetchStat: async () => null,
    });
    const { body } = dom.document;
    await waitFor(() => {
      expect(body.textContent).toContain("Session gone");
      expect(body.textContent).toContain("—");
    });
  });
});
