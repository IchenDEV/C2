// @ts-nocheck
import { afterEach, expect, test } from "bun:test";

import { Simulate } from "react-dom/test-utils";

import {
  activateDom,
  dom,
  mount,
  click,
  button,
  waitFor,
  flush,
} from "./domTestHarness";
activateDom();
const { AssistantWorkspace } =
  await import("../src/assistant/AssistantWorkspace");
const { I18nProvider } = await import("../src/i18n");
const mounted = [];
afterEach(() => {
  for (const root of mounted.splice(0)) root.unmount();
  dom.document.body.replaceChildren();
});
function render({
  rejectEdits = false,
  coordination = false,
  freshChange = null,
  initialChange = null,
} = {}) {
  const calls = [];
  let snapshot = {
    state: {
      revision: 3,
      settings: {
        enabled: false,
        projects: ["/a", "/b"],
        provider: "codex",
        model: null,
        reasoning_effort: null,
        concurrency: 1,
        turn_limit: 10,
        dispatch_limit: 5,
      },
      goals: [
        {
          id: "g1",
          project_path: "/a",
          title: "Release report",
          acceptance: "Versioned report and checks",
          priority: 1,
          contract_revision: 1,
          dependencies: [],
          status: "active",
          next_step: "Check the report",
          blocker: "",
          assignments: [
            {
              id: "a1",
              session_id: "s1",
              submitted: true,
              taken_over: false,
              owned: true,
              contract_revision: 1,
              confirmed_revision: 1,
              stop_requested: false,
              result: coordination
                ? {
                    id: "result1",
                    contract_revision: 1,
                    evidence: "Versioned report checked",
                    artifacts: [{ path: "report.txt", sha256: "a".repeat(64) }],
                    state: "submitted",
                  }
                : null,
            },
          ],
          verdict: null,
        },
      ],
      messages: [],
      questions: coordination
        ? [
            {
              id: "q1",
              goal_id: "g1",
              assignment_id: "a1",
              contract_revision: 1,
              title: "Which locale?",
              context: "Report language",
              options: ["English", "Chinese"],
              blocking: true,
              state: "open",
              answer: null,
              answered_by: null,
              source_input: null,
            },
          ]
        : [],
      changes: coordination
        ? [
            {
              id: "c1",
              goal_id: "g1",
              from_revision: 1,
              to_revision: 2,
              before_title: "Release report",
              before_acceptance: "Versioned report and checks",
              title: "Chinese report",
              acceptance: "Chinese release summary",
              reason: "Users need Chinese",
              state: "proposed",
              proposed: true,
              remember: false,
              memory_id: null,
            },
          ]
        : [],
      notifications: coordination
        ? [
            {
              id: "n1",
              goal_id: "g1",
              kind: "deliverable",
              title: "Report submitted",
              body: "Waiting for review",
              session_id: "s1",
              reference_id: "result1",
              read: false,
              desktop: "unknown",
            },
          ]
        : [],
      requests: [],
      run: null,
      summary: "Project A is awaiting review",
      attention: null,
      turns: 1,
      dispatches: 1,
    },
    sessions: [],
    memories: [
      {
        id: "m1",
        project_path: "codetwo://chief-of-staff",
        layer: "L1",
        category: "preference",
        content: "Keep reports short",
        origin: "manual",
        active: true,
        editable: true,
        session_id: null,
        updated_at: 1,
      },
    ],
  };
  let snapshotCalls = 0;
  if (initialChange) initialChange(snapshot);
  const api = {
    catalog: async () => [
      [
        { path: "/a", name: "Project A" },
        { path: "/b", name: "Project B" },
      ],
      [],
    ],
    snapshot: async () => {
      if (++snapshotCalls === 2 && freshChange) freshChange(snapshot.state);
      return structuredClone(snapshot);
    },
    edit: async (revision, edit) => {
      calls.push({ revision, edit });
      if (rejectEdits)
        throw new Error("Revision conflict; retry after refreshing");
      if (edit.kind === "takeover") snapshot.state.goals[0].status = "paused";
      if (edit.kind === "answer")
        snapshot.state.questions[0].state = "answered";
      if (edit.kind === "acknowledge")
        snapshot.state.notifications[0].read = true;
      return snapshot.state;
    },
    memory: async (name, args) => {
      calls.push({ name, args });
      if (name === "set_active") snapshot.memories = [];
    },
  };
  mounted.push(
    mount(
      <I18nProvider>
        <AssistantWorkspace
          activity={<p>Real execution view</p>}
          api={api}
          onSelect={(id) => calls.push({ open: id })}
          onClose={() => calls.push({ close: true })}
        />
      </I18nProvider>
    )
  );
  return calls;
}
const panel = () => dom.document.querySelector("section > div.overflow-y-auto");
// Normal goals live in the Continuous tasks view, never in the default conversation.
async function expandGoals() {
  await waitFor(() =>
    expect(button(dom.document.body, "Continuous tasks")).toBeDefined()
  );
  click(button(dom.document.body, "Continuous tasks"));
  await flush();
  await waitFor(() =>
    expect(
      dom.document.querySelector('article[aria-label="Release report"]')
    ).not.toBeNull()
  );
}
async function openMore(name) {
  click(button(dom.document.body, "More tools"));
  await waitFor(() =>
    expect(
      [...dom.document.querySelectorAll('[role="menuitem"],button')].some(
        (e) => e.textContent.trim() === name
      )
    ).toBe(true)
  );
  const item = [
    ...dom.document.querySelectorAll('[role="menuitem"],button'),
  ].find((e) => e.textContent.trim() === name);
  item.dispatchEvent(
    new dom.window.PointerEvent("pointerdown", {
      bubbles: true,
      pointerType: "mouse",
    })
  );
  click(item);
  await flush();
}
async function openSelected(section = "Progress") {
  await expandGoals();
  click(
    button(
      dom.document.querySelector('article[aria-label="Release report"]'),
      "Details"
    )
  );
  await flush();
  if (section !== "Progress") {
    click(button(panel(), section));
    await flush();
  }
}
async function choose(label, value) {
  click(dom.document.querySelector(`[role="combobox"][aria-label="${label}"]`));
  await waitFor(() =>
    expect(
      [...dom.document.querySelectorAll('[role="option"]')].some(
        (e) => e.textContent === value
      )
    ).toBe(true)
  );
  const option = [...dom.document.querySelectorAll('[role="option"]')].find(
    (e) => e.textContent === value
  );
  option.dispatchEvent(
    new dom.window.PointerEvent("pointerdown", {
      bubbles: true,
      pointerType: "mouse",
    })
  );
  click(option);
  await flush();
}
test("project status stays distinct from execution and takeover is explicit", async () => {
  const calls = render();
  await expandGoals();
  await openSelected("Manage");
  expect(dom.document.body.textContent).toContain(
    "Versioned report and checks"
  );
  click(button(panel(), "Take over"));
  await waitFor(() =>
    expect(calls.some((c) => c.edit?.kind === "takeover")).toBe(true)
  );
  expect(calls.find((c) => c.edit).revision).toBe(3);
  click(button(panel(), "Open execution"));
  expect(calls).toContainEqual({ open: "s1" });
  expect(calls).toContainEqual({ close: true });
});
test("global memory is inspectable and forgetting refreshes the notebook", async () => {
  const calls = render();
  await waitFor(() =>
    expect(button(dom.document.body, "Memory")).toBeDefined()
  );
  click(button(dom.document.body, "Memory"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Keep reports short")
  );
  click(button(dom.document.body, "Forget"));
  await waitFor(() =>
    expect(dom.document.body.textContent).not.toContain("Keep reports short")
  );
  expect(calls).toContainEqual({
    name: "set_active",
    args: { id: "m1", value: false },
  });
  // Execution diagnostics are secondary: only reachable through More tools.
  expect(() => button(dom.document.body, "Execution diagnostics")).toThrow();
  await openMore("Execution diagnostics");
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Real execution view")
  );
});

function change(element, value) {
  element.value = value;
  Simulate.change(element);
}

test("a rejected goal edit in details keeps the draft available for retry", async () => {
  const calls = render({ rejectEdits: true });
  await openSelected("Manage");
  const form = button(panel(), "Save changes").closest("form");
  change(form.querySelector('input[data-slot="input"]'), "Keep this goal");
  change(form.querySelector("textarea"), "Keep this acceptance");
  await flush();
  form.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true })
  );
  await waitFor(() =>
    expect(dom.document.querySelector('[role="alert"]').textContent).toContain(
      "Revision conflict"
    )
  );
  expect(calls.find((c) => c.edit?.kind === "goal").edit).toMatchObject({
    id: "g1",
    project_path: "/a",
    title: "Keep this goal",
  });
  expect(form.querySelector('input[data-slot="input"]').value).toBe(
    "Keep this goal"
  );
  expect(form.querySelector("textarea").value).toBe("Keep this acceptance");
});

test("messages, stop and rework are separate operations and failed sends retain drafts", async () => {
  const calls = render({ coordination: true, rejectEdits: true });
  await openSelected("Updates");
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Message worker")
  );
  const label = [...dom.document.querySelectorAll("label")].find((e) =>
    e.textContent.includes("Message worker")
  );
  change(label.querySelector("textarea"), "Keep the existing section");
  await flush();
  click(button(dom.document.body, "Send message"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Revision conflict")
  );
  expect(label.querySelector("textarea").value).toBe(
    "Keep the existing section"
  );
  expect(
    calls.some((c) => c.edit?.kind === "message" && c.edit.goal_id === "g1")
  ).toBe(true);
  click(button(panel(), "Request stop"));
  await waitFor(() =>
    expect(calls.some((c) => c.edit?.operation === "stop")).toBe(true)
  );
  await flush();
  click(button(panel(), "Progress"));
  await flush();
  const review = [...dom.document.querySelectorAll("label")].find((e) =>
    e.textContent.includes("Review evidence")
  );
  change(review.querySelector("textarea"), "Add concrete regression evidence");
  await flush();
  click(button(dom.document.body, "Request rework"));
  await waitFor(() =>
    expect(
      calls.some(
        (c) =>
          c.edit?.verdict === "rework" &&
          c.edit.evidence === "Add concrete regression evidence"
      )
    ).toBe(true)
  );
});

test("unrelated worker activity does not reject a message composed against the same goal", async () => {
  const calls = render({
    freshChange: (state) => {
      state.revision = 4;
      state.turns += 1;
    },
  });
  await openSelected("Updates");
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Message worker")
  );
  const label = [...dom.document.querySelectorAll("label")].find((e) =>
    e.textContent.includes("Message worker")
  );
  change(label.querySelector("textarea"), "Keep the existing section");
  await flush();
  click(button(dom.document.body, "Send message"));
  await waitFor(() =>
    expect(calls.some((c) => c.edit?.kind === "message")).toBe(true)
  );
  expect(calls.find((c) => c.edit?.kind === "message").revision).toBe(4);
});

test("a new requirement while composing retains the draft for an explicit retry", async () => {
  const calls = render({
    freshChange: (state) => {
      state.revision = 4;
      state.goals[0].contract_revision = 2;
    },
  });
  await openSelected("Updates");
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Message worker")
  );
  const label = [...dom.document.querySelectorAll("label")].find((e) =>
    e.textContent.includes("Message worker")
  );
  change(label.querySelector("textarea"), "Keep the existing section");
  await flush();
  click(button(dom.document.body, "Send message"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("This target changed")
  );
  expect(calls.some((c) => c.edit?.kind === "message")).toBe(false);
  expect(label.querySelector("textarea").value).toBe(
    "Keep the existing section"
  );
});

test("a polled replacement deliverable cannot reuse an earlier review draft", async () => {
  const calls = render({
    coordination: true,
    freshChange: (state) => {
      state.revision = 4;
      state.goals[0].assignments[0].result.id = "result2";
      state.goals[0].assignments[0].result.evidence =
        "New deliverable inspected separately";
    },
  });
  await openSelected("Progress");
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Review evidence")
  );
  const label = [...dom.document.querySelectorAll("label")].find((e) =>
    e.textContent.includes("Review evidence")
  );
  change(label.querySelector("textarea"), "Review of result1");
  await flush();
  await waitFor(
    () =>
      expect(dom.document.body.textContent).toContain(
        "New deliverable inspected separately"
      ),
    6500
  );
  click(button(dom.document.body, "Accept"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("This target changed")
  );
  expect(calls.some((c) => c.edit?.kind === "review")).toBe(false);
  expect(label.querySelector("textarea").value).toBe("Review of result1");
  click(button(dom.document.body, "Reviewed; continue with latest version"));
  await flush();
  click(button(dom.document.body, "Accept"));
  await waitFor(() =>
    expect(calls.some((c) => c.edit?.kind === "review")).toBe(true)
  );
}, 12000);

test("forgetting a memory exposes affected assignments and sends a distinct correction", async () => {
  const calls = render();
  await waitFor(() =>
    expect(button(dom.document.body, "Memory")).toBeDefined()
  );
  click(button(dom.document.body, "Memory"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Keep reports short")
  );
  click(button(dom.document.body, "Forget"));
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain(
      "These assignments may have used"
    )
  );
  const form = button(dom.document.body, "Send memory correction").closest(
    "form"
  );
  change(
    form.querySelector("textarea"),
    "The previous length preference is withdrawn; follow the current acceptance criteria."
  );
  await flush();
  form.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true })
  );
  await waitFor(() =>
    expect(
      calls.some((c) => c.edit?.kind === "message" && c.edit.goal_id === "g1")
    ).toBe(true)
  );
  expect(dom.document.body.textContent).toContain("Correction recorded");
});

test("independent review sessions expose scoped progress and failure", async () => {
  const calls = render({
    initialChange(snapshot) {
      snapshot.state.scoped_runs = [
        {
          scope: "g1",
          run: { id: "r1", session_id: "review-a" },
          state: "running",
          summary: "",
          error: null,
        },
        {
          scope: "request-b",
          run: { id: "r2", session_id: null },
          state: "failed",
          summary: "",
          error: "B needs a decision",
        },
      ];
    },
  });
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("B needs a decision")
  );
  await openSelected();
  expect(dom.document.body.textContent).toContain("Reviewing");
  click(button(dom.document.body, "Open review"));
  expect(calls).toContainEqual({ open: "review-a" });
});

test("unknown answer remains visible and cannot be blindly replayed", async () => {
  const calls = render({
    coordination: true,
    initialChange(snapshot) {
      snapshot.state.questions[0].state = "delivery_unknown";
    },
  });
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Answer outcome unknown")
  );
  expect(button(dom.document.body, "Reopen clarification").disabled).toBe(true);
  expect(calls.some((c) => c.edit?.kind === "retry_question")).toBe(false);
});

test("conversation replaces the forms; sections keep draft and stop access", async () => {
  const calls = render({ coordination: true });
  await waitFor(() =>
    expect(dom.document.querySelector('[role="textbox"]')).not.toBeNull()
  );
  expect(dom.document.querySelectorAll("details, summary, select").length).toBe(
    0
  );
  // One shared rich textbox plus the open question's own answer field; no request or goal forms.
  expect(dom.document.querySelectorAll('[role="textbox"]').length).toBe(1);
  expect(dom.document.querySelectorAll("textarea").length).toBe(1);
  expect(dom.document.body.textContent).not.toContain("Give a request");
  expect(dom.document.body.textContent).not.toContain("Define acceptance");
  await openSelected("Updates");
  const input = button(panel(), "Send message")
    .closest("form")
    .querySelector("textarea");
  change(input, "Keep this draft across sections");
  await flush();
  click(button(panel(), "Manage"));
  await flush();
  expect(input.closest("[hidden]")).not.toBeNull();
  click(button(panel(), "Updates"));
  await flush();
  expect(input.value).toBe("Keep this draft across sections");
  expect(button(panel(), "Request stop").closest("[hidden]")).toBeNull();
  expect(calls.some((c) => c.edit)).toBe(false);
});

test("settings and memory composition are on demand and do not mutate on navigation", async () => {
  const calls = render();
  await waitFor(() =>
    expect(button(dom.document.body, "Settings")).toBeDefined()
  );
  click(button(dom.document.body, "Settings"));
  await flush();
  const advanced = dom.document.querySelector("#assistant-advanced-settings");
  expect(advanced.hidden).toBe(true);
  expect(dom.document.querySelector('[role="region"]').hidden).toBe(true);
  click(button(dom.document.body, "Advanced settings"));
  await flush();
  expect(advanced.hidden).toBe(false);
  click(button(dom.document.body, "Memory"));
  await flush();
  const remember = button(dom.document.body, "Remember");
  expect(remember.closest("[hidden]")).not.toBeNull();
  click(button(dom.document.body, "Add memory"));
  await flush();
  expect(remember.closest("[hidden]")).toBeNull();
  expect(calls.some((c) => c.edit || c.name)).toBe(false);
});
