// @ts-nocheck
import { afterEach, expect, test } from "bun:test";

import { Simulate } from "react-dom/test-utils";

import {
  activateDom,
  button,
  click,
  dom,
  flush,
  mount,
  waitFor,
} from "./domTestHarness";
activateDom();
// BlockNote/Excalidraw probe canvas at import; same narrow stub as editorCanvasRendered.
dom.window.HTMLCanvasElement.prototype.getContext = () =>
  ({ filter: "" }) as never;
const { AssistantWorkspace } =
  await import("../src/assistant/AssistantWorkspace");
const { sameEditTarget } = await import("../src/assistant/api");
const { I18nProvider } = await import("../src/i18n");
const mounted = [];
afterEach(() => {
  for (const root of mounted.splice(0)) root.unmount();
  dom.document.body.replaceChildren();
});

const goal = (over = {}) => ({
  id: "g1",
  project_path: "/a",
  title: "Login fix",
  acceptance: "Users can sign in",
  priority: 1,
  contract_revision: 2,
  dependencies: [],
  status: "active",
  next_step: "Run the checks",
  blocker: "",
  assignments: [
    {
      id: "a1",
      session_id: "s1",
      submitted: true,
      taken_over: false,
      owned: true,
      contract_revision: 2,
      confirmed_revision: 1,
      stop_requested: false,
      prior_results: [],
      result: null,
    },
  ],
  verdict: null,
  ...over,
});
const question = (over = {}) => ({
  id: "q1",
  goal_id: "g1",
  assignment_id: "a1",
  contract_revision: 2,
  title: "Which locale?",
  context: "Report language",
  options: ["English", "Chinese"],
  blocking: true,
  state: "open",
  answer: null,
  answered_by: null,
  source_input: null,
  ...over,
});
const turn = (over = {}) => ({
  id: "t1",
  created_at: "2026-10-05T08:00:00Z",
  author: "user",
  content: "Where is login?",
  reply_to: null,
  project_paths: [],
  status: "recorded",
  goal_ids: [],
  question_ids: [],
  error: null,
  source_run_id: null,
  ...over,
});
const proposal = (over = {}) => ({
  id: "p1",
  turn_id: "t1",
  project_path: "/a",
  category: "preference",
  content: "Release notes are written in Chinese",
  content_hash: "abcdef0123456789",
  state: "proposed",
  memory_id: null,
  error: null,
  ...over,
});

function render({
  state = {},
  rejectEdits = 0,
  onPoll = null,
  legacy = false,
  beforeEdit = null,
} = {}) {
  const calls = [];
  const snapshot = {
    state: {
      revision: 5,
      settings: {
        enabled: true,
        projects: ["/a", "/b"],
        provider: "codex",
        model: null,
        reasoning_effort: null,
        concurrency: 1,
        turn_limit: 10,
        dispatch_limit: 5,
      },
      goals: [goal()],
      messages: [],
      questions: [],
      changes: [],
      notifications: [],
      requests: [],
      run: null,
      summary: "",
      attention: null,
      turns: 0,
      dispatches: 0,
      conversation: [],
      memory_proposals: [],
      ...state,
    },
    sessions: [],
    memories: [],
  };
  if (legacy) {
    delete snapshot.state.conversation;
    delete snapshot.state.memory_proposals;
  }
  let failures = rejectEdits;
  let polls = 0;
  const api = {
    catalog: async () => [
      [
        { path: "/a", name: "Project A" },
        { path: "/b", name: "Project B" },
      ],
      [],
    ],
    snapshot: async () => {
      polls += 1;
      if (polls > 1 && onPoll) onPoll(snapshot.state, polls);
      return structuredClone(snapshot);
    },
    edit: async (revision, edit) => {
      calls.push({ revision, edit });
      if (beforeEdit) await beforeEdit(edit);
      if (failures > 0) {
        failures -= 1;
        throw new Error("Network lost");
      }
      if (edit.kind === "say")
        snapshot.state.conversation = [
          ...(snapshot.state.conversation ?? []),
          turn({ id: edit.turn_id, content: edit.content }),
        ];
      return snapshot.state;
    },
    memory: async () => {},
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
  return { calls, snapshot };
}

const composer = () =>
  dom.document.querySelector(
    '[role="textbox"][aria-label="Message to chief of staff"]'
  );
const panel = () => dom.document.querySelector("section > div.overflow-y-auto");
const text = () => dom.document.body.textContent;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const draft = () => (composer()?.textContent ?? "").trim();
const labelled = (re) =>
  [...dom.document.querySelectorAll("[aria-label],[title]")].filter((e) =>
    re.test(
      `${e.getAttribute("aria-label") ?? ""} ${e.getAttribute("title") ?? ""}`
    )
  );
const maybe = (name) => {
  try {
    return button(dom.document.body, name);
  } catch {
    return null;
  }
};
// Drive the real contenteditable: replace the first paragraph's inline content and let
// ProseMirror's own DOM observer turn it into a document change.
async function type(value) {
  const root = composer();
  root.focus();
  const inline = root.querySelector(".bn-inline-content");
  const range = dom.document.createRange();
  range.selectNodeContents(inline);
  const selection = dom.window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  inline.textContent = value;
  inline.dispatchEvent(
    new dom.window.InputEvent("input", {
      bubbles: true,
      inputType: "insertText",
      data: value,
    })
  );
  await flush();
  await sleep(40);
  await flush();
}
async function ready() {
  await waitFor(() => expect(composer()).not.toBeNull());
  await waitFor(() =>
    expect(button(dom.document.body, "Project scope (optional)")).toBeDefined()
  );
}
async function openTasks() {
  await ready();
  click(button(dom.document.body, "Continuous tasks"));
  await flush();
  await waitFor(() => expect(text()).toContain("Login fix"));
}
const sayCalls = (calls) => calls.filter((c) => c.edit?.kind === "say");

test("the default view is only the conversation and shared rich composer; tasks are one icon away", async () => {
  render({
    state: { goals: [goal(), goal({ id: "g2", title: "Release prep" })] },
  });
  await ready();
  // Same rich textbox as the main composer; never a bespoke textarea.
  expect(composer().getAttribute("contenteditable")).toBe("true");
  expect(dom.document.querySelector("textarea")).toBeNull();
  // Quiet transcript: no goal cards, controls, execution/session/run details or text navigation.
  for (const hidden of [
    "Login fix",
    "Release prep",
    "Request stop",
    "Take over",
    "Real execution view",
    "Execution",
    "Conversation",
  ])
    expect(text()).not.toContain(hidden);
  expect(maybe("Request stop")).toBeNull();
  expect(maybe("Open handling session")).toBeNull();
  expect(maybe("2 in progress")).toBeNull();
  // Icon-only header: Memory, Continuous tasks, Settings, Close.
  for (const name of ["Memory", "Continuous tasks", "Settings", "Close"])
    expect(maybe(name)).not.toBeNull();
  expect(maybe("Back to conversation")).toBeNull();
  // Diagnostics only behind a secondary menu.
  expect(maybe("Execution diagnostics")).toBeNull();
  expect(maybe("More tools")).not.toBeNull();
  // The composer sits outside the scrolling transcript, so history cannot push it away.
  const transcript = dom.document.querySelector(".overscroll-contain");
  expect(transcript).not.toBeNull();
  expect(transcript.contains(composer())).toBe(false);
  click(button(dom.document.body, "Continuous tasks"));
  await flush();
  await waitFor(() => expect(text()).toContain("Login fix"));
  expect(text()).toContain("Release prep");
  expect(maybe("Request stop")).not.toBeNull();
  expect(maybe("Back to conversation")).not.toBeNull();
  click(button(dom.document.body, "Back to conversation"));
  await flush();
  expect(text()).not.toContain("Login fix");
});

test("the composer is the same shared card and send button as the main Composer", async () => {
  render();
  await ready();
  const card = button(dom.document.body, "Send").closest(".composer-card");
  expect(card).not.toBeNull();
  expect(card.contains(composer())).toBe(true);
  // One implementation: both surfaces consume the same exported pieces.
  const main = await Bun.file(
    new URL("../src/session/Composer.tsx", import.meta.url)
  ).text();
  const chief = await Bun.file(
    new URL("../src/assistant/ChiefThread.tsx", import.meta.url)
  ).text();
  for (const piece of ["ComposerCard", "ComposerSendButton"]) {
    expect(main).toContain(`<${piece}`);
    expect(chief).toContain(`<${piece}`);
    expect(chief).toMatch(
      new RegExp(`import[^;]*${piece}[^;]*"../session/Composer"`, "u")
    );
  }
  expect(chief).not.toContain("<textarea");
});

test("normal goals are inspector only, even when the conversation references them", async () => {
  render({
    state: {
      goals: [goal(), goal({ id: "g2", title: "Release prep" })],
      conversation: [turn({ goal_ids: ["g2"] })],
    },
  });
  await waitFor(() => expect(text()).toContain("Where is login?"));
  expect(text()).not.toContain("Release prep");
  expect(text()).not.toContain("Login fix");
  expect(maybe("Request stop")).toBeNull();
});

test("a plain question sends only a say and never creates a goal or request", async () => {
  const { calls } = render();
  await ready();
  await type("  How is the login fix going?  ");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(sayCalls(calls).length).toBe(1));
  const [{ edit, revision }] = sayCalls(calls);
  expect(edit.content).toBe("How is the login fix going?");
  expect(edit.turn_id).toMatch(/^[0-9a-f-]{36}$/u);
  expect("project_paths" in edit).toBe(false);
  expect(revision).toBe(5);
  expect(calls.some((c) => ["goal", "request"].includes(c.edit?.kind))).toBe(
    false
  );
  await waitFor(() => expect(draft()).toBe(""));
  // The recorded reply state is an icon tooltip, never a running/complete execution footer.
  await waitFor(() =>
    expect(
      labelled(/Recorded; waiting for the chief of staff/u).length
    ).toBeGreaterThan(0)
  );
  expect(text()).not.toMatch(/\b(Running|Completed?|Done)\b/u);
});

test("an empty rich editor never sends", async () => {
  const { calls } = render();
  await ready();
  click(button(dom.document.body, "Send"));
  await flush();
  await sleep(30);
  expect(calls.some((c) => c.edit)).toBe(false);
  await type("   ");
  click(button(dom.document.body, "Send"));
  await flush();
  await sleep(30);
  expect(calls.some((c) => c.edit)).toBe(false);
});

test("typing the next message while a send finishes preserves the new draft", async () => {
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const { calls } = render({ beforeEdit: () => pending });
  await ready();
  await type("First message");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(sayCalls(calls).length).toBe(1));
  await type("Next message");
  release();
  await waitFor(() => expect(text()).toContain("First message"));
  await sleep(40);
  expect(draft()).toBe("Next message");
});

test("one message may cover several projects without a required project choice", async () => {
  const { calls } = render();
  await ready();
  await type("Watch A's login fix and B's release prep");
  click(button(dom.document.body, "Project scope (optional)"));
  await flush();
  click(button(dom.document.body, "Project B"));
  click(button(dom.document.body, "Project A"));
  await flush();
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(sayCalls(calls).length).toBe(1));
  expect(sayCalls(calls)[0].edit.project_paths).toEqual(["/a", "/b"]);
});

test("a failed send keeps the draft and retries with the same id until the text changes", async () => {
  const { calls } = render({ rejectEdits: 2 });
  await ready();
  await type("Plan the release");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(text()).toContain("Not sent"));
  expect(draft()).toBe("Plan the release");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(sayCalls(calls).length).toBe(2));
  await waitFor(() => expect(text()).toContain("Not sent"));
  const [first, second] = sayCalls(calls).map((c) => c.edit);
  expect(second).toEqual(first);
  // A failed send is never replayed by polling or any other path.
  await sleep(80);
  expect(sayCalls(calls).length).toBe(2);
  await type("Plan the release carefully");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(sayCalls(calls).length).toBe(3));
  expect(sayCalls(calls)[2].edit.turn_id).not.toBe(first.turn_id);
  await waitFor(() => expect(draft()).toBe(""));
});

test("Mod+Enter sends here and never reaches the window-level run", async () => {
  const { calls } = render();
  await ready();
  await type("Send by keyboard");
  let leaked = 0;
  const spy = () => {
    leaked += 1;
  };
  dom.window.addEventListener("keydown", spy);
  try {
    composer().dispatchEvent(
      new dom.window.KeyboardEvent("keydown", {
        key: "Enter",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      })
    );
    await waitFor(() => expect(sayCalls(calls).length).toBe(1));
  } finally {
    dom.window.removeEventListener("keydown", spy);
  }
  expect(sayCalls(calls)[0].edit.content).toBe("Send by keyboard");
  expect(leaked).toBe(0);
});

test("legacy snapshots without conversation fields still render", async () => {
  render({ legacy: true });
  await ready();
  expect(composer()).not.toBeNull();
});

test("intake outcomes are shown per message and an unknown one cannot be resent", async () => {
  const { calls } = render({
    state: {
      conversation: [
        turn({
          id: "t1",
          status: "unknown",
          source_run_id: null,
          error: "No receipt",
        }),
        turn({
          id: "t2",
          author: "assistant",
          content: "**Still** testing",
          reply_to: "t1",
          status: "handled",
        }),
      ],
      scoped_runs: [
        {
          scope: "turn:t1",
          run: { id: "r1", session_id: "review-1" },
          state: "failed",
          summary: "",
          error: "No receipt",
        },
      ],
    },
  });
  await waitFor(() =>
    expect(text()).toContain("will not be resent automatically")
  );
  expect(text()).toContain("No receipt");
  expect(text().split("No receipt").length - 1).toBe(1);
  expect(dom.document.querySelector("strong").textContent).toBe("Still");
  expect(
    [...dom.document.querySelectorAll("button")].some((b) =>
      /resend|retry/iu.test(b.textContent)
    )
  ).toBe(false);
  // Unknown outcomes keep the recovery entry to the original handling session.
  const recover = [...dom.document.querySelectorAll("button")].find((b) =>
    /handling session/iu.test(b.textContent)
  );
  expect(recover).toBeDefined();
  click(recover);
  expect(calls).toContainEqual({ open: "review-1" });
});

test("handled receipts are hidden, failed is explicit, and a failed review hides its manager session", async () => {
  render({
    state: {
      conversation: [
        turn({ id: "t1", content: "Question one", status: "handled" }),
        turn({
          id: "t2",
          content: "Question two",
          status: "failed",
          error: "Review crashed",
          source_run_id: "r2",
        }),
        turn({ id: "t3", content: "Question three", status: "reviewing" }),
      ],
      scoped_runs: [
        {
          scope: "turn:t2",
          run: { id: "r2", session_id: "review-2" },
          state: "failed",
          summary: "",
          error: "Review crashed",
        },
      ],
    },
  });
  await waitFor(() => expect(text()).toContain("Question three"));
  // handled normal receipt is not a visible chip
  expect(text()).not.toContain("Handled");
  expect(labelled(/^Handled/u).length).toBe(0);
  // failed is an explicit, visible status with its reason, shown once
  expect(text()).toContain("Could not be handled");
  expect(text().split("Review crashed").length - 1).toBe(1);
  // reviewing is an icon tooltip, not text
  expect(text()).not.toContain("Chief of staff is reviewing");
  expect(labelled(/Chief of staff is reviewing/u).length).toBeGreaterThan(0);
  // the normal manager session link stays hidden for a failed review
  expect(
    [...dom.document.querySelectorAll("button")].some((b) =>
      /handling session|open review/iu.test(b.textContent)
    )
  ).toBe(false);
});

test("conversationTurns makes one Turn per original record in record order", async () => {
  const { conversationTurns } = await import("../src/assistant/ChiefThread");
  const rows = [
    turn({
      id: "a-early",
      author: "assistant",
      content: "Early reply",
      reply_to: "t2",
    }),
    turn({ id: "t1", content: "First question" }),
    turn({ id: "t2", content: "Second question" }),
    turn({
      id: "a-late",
      author: "assistant",
      content: "Late reply",
      reply_to: "t1",
    }),
    turn({
      id: "a-late2",
      author: "assistant",
      content: "Another late reply",
      reply_to: "t1",
    }),
    turn({
      id: "a-orphan",
      author: "assistant",
      content: "Orphan reply",
      reply_to: "missing",
    }),
  ];
  const frozen = structuredClone(rows);
  const turns = conversationTurns(rows);
  expect(turns.map((t) => t.requestId)).toEqual(rows.map((r) => r.id));
  turns.forEach((t, i) => {
    const row = rows[i];
    if (row.author === "user") expect(t.prompt).toBe(row.content);
    else expect(`${t.prompt}${t.text}`).toContain(row.content);
  });
  // reply_to stays on the AssistantState rows; the projection is read-only.
  expect(rows).toEqual(frozen);
  expect(rows[3].reply_to).toBe("t1");
});

test("late, early and orphan assistant replies render in original record order with no execution footer", async () => {
  render({
    state: {
      conversation: [
        turn({
          id: "a-early",
          author: "assistant",
          content: "ZZ early reply",
          reply_to: "t2",
          status: "handled",
        }),
        turn({ id: "t1", content: "ZZ first question" }),
        turn({ id: "t2", content: "ZZ second question" }),
        turn({
          id: "a-late",
          author: "assistant",
          content: "ZZ late reply",
          reply_to: "t1",
          status: "handled",
        }),
        turn({
          id: "a-orphan",
          author: "assistant",
          content: "ZZ orphan reply",
          reply_to: "gone",
          status: "handled",
        }),
      ],
    },
  });
  await waitFor(() => expect(text()).toContain("ZZ orphan reply"));
  const order = [
    "ZZ early reply",
    "ZZ first question",
    "ZZ second question",
    "ZZ late reply",
    "ZZ orphan reply",
  ].map((needle) => text().indexOf(needle));
  expect(order.every((i) => i >= 0)).toBe(true);
  expect(order).toEqual([...order].toSorted((a, b) => a - b));
  expect(text()).not.toMatch(/\b(Running|Completed?|Worked for)\b/u);
});

test("questions, proposals and deliveries inline use the original records", async () => {
  const delivered = goal({
    assignments: [
      {
        ...goal().assignments[0],
        result: {
          id: "res1",
          contract_revision: 1,
          evidence: "Checks passed",
          artifacts: [{ path: "login.txt", sha256: "b".repeat(64) }],
          state: "submitted",
        },
      },
    ],
  });
  const { calls } = render({
    state: {
      goals: [delivered],
      conversation: [
        turn({ id: "t1", goal_ids: ["g1"], question_ids: ["q1"] }),
      ],
      questions: [question()],
      changes: [
        {
          id: "c1",
          goal_id: "g1",
          from_revision: 1,
          to_revision: 2,
          before_title: "Login fix",
          before_acceptance: "Users can sign in",
          title: "Login and logout",
          acceptance: "Both work",
          reason: "Same flow",
          state: "proposed",
          proposed: true,
          remember: false,
          memory_id: null,
        },
      ],
    },
  });
  await waitFor(() => expect(text()).toContain("Which locale?"));
  // The version-bound delivery says it targets an older requirement version.
  expect(text()).toContain("Submitted; awaiting your review · v1");
  expect(text()).toContain("older version");
  expect(text()).toContain("login.txt");
  click(dom.document.querySelector('input[value="Chinese"]'));
  await flush();
  click(button(dom.document.body, "Send answer"));
  await waitFor(() =>
    expect(
      calls.some(
        (c) => c.edit?.question_id === "q1" && c.edit.answer === "Chinese"
      )
    ).toBe(true)
  );
  click(button(dom.document.body, "Apply change"));
  await waitFor(() =>
    expect(calls.some((c) => c.edit?.change_id === "c1")).toBe(true)
  );
  click(button(dom.document.body, "Review deliverable"));
  await flush();
  expect(button(panel(), "Request rework")).toBeDefined();
});

test("a permission request opens its original session and offers no inline approval", async () => {
  const { calls } = render({
    state: {
      conversation: [turn({ question_ids: ["q1"] })],
      questions: [
        question({
          title: "Allow shell?",
          source_input: {
            kind: "permission",
            options: [["allow", "Allow"]],
            context: {},
          },
        }),
      ],
    },
  });
  await waitFor(() => expect(text()).toContain("Allow shell?"));
  expect(dom.document.querySelector('input[value="allow"]')).toBeNull();
  expect(
    [...dom.document.querySelectorAll("button")].some(
      (b) => b.textContent === "Send answer"
    )
  ).toBe(false);
  click(button(dom.document.body, "Open original session"));
  expect(calls).toContainEqual({ open: "s1" });
  expect(calls.some((c) => c.edit)).toBe(false);
});

test("memory suggestions show scope and text and bind the content hash", async () => {
  const { calls } = render({
    state: { conversation: [turn()], memory_proposals: [proposal()] },
  });
  await waitFor(() =>
    expect(text()).toContain("Release notes are written in Chinese")
  );
  expect(text()).toContain("Project A");
  // The content hash is binding data, not reading material: it travels in the confirm edit below.
  click(button(dom.document.body, "Remember"));
  await waitFor(() =>
    expect(calls.find((c) => c.edit?.kind === "confirm_memory")?.edit).toEqual({
      kind: "confirm_memory",
      proposal_id: "p1",
      content_hash: "abcdef0123456789",
    })
  );
});

test("a memory suggestion whose text changed before the click is not applied", async () => {
  const { calls } = render({
    state: { conversation: [turn()], memory_proposals: [proposal()] },
    onPoll: (state) => {
      state.memory_proposals[0].content_hash = "ffffffff00000000";
    },
  });
  await waitFor(() => expect(text()).toContain("Release notes are written"));
  click(button(dom.document.body, "Remember"));
  await waitFor(() => expect(text()).toContain("This target changed"));
  expect(calls.some((c) => c.edit)).toBe(false);
});

test.each([
  [null, "Confirmed; waiting to save"],
  ["memory-1", "Saved to memory"],
])("confirmed memory receipt %s shows %s", async (memory_id, label) => {
  render({
    state: {
      conversation: [turn()],
      memory_proposals: [proposal({ state: "confirmed", memory_id })],
    },
  });
  await waitFor(() => expect(text()).toContain(label));
  expect(text()).toContain("Release notes are written in Chinese");
  if (memory_id == null) expect(text()).not.toContain("Saved to memory");
});

test("sameEditTarget checks memory proposals and the question a say answers", () => {
  const before = {
    memory_proposals: [proposal()],
    questions: [question()],
    goals: [],
  };
  const confirm = {
    kind: "confirm_memory",
    proposal_id: "p1",
    content_hash: "abcdef0123456789",
  };
  expect(sameEditTarget(before, structuredClone(before), confirm)).toBe(true);
  const rehashed = structuredClone(before);
  rehashed.memory_proposals[0].content_hash = "other";
  expect(
    sameEditTarget(before, rehashed, { ...confirm, kind: "reject_memory" })
  ).toBe(false);
  const done = structuredClone(before);
  done.memory_proposals[0].state = "confirmed";
  expect(sameEditTarget(before, done, confirm)).toBe(false);
  const say = {
    kind: "say",
    turn_id: "x",
    content: "ok",
    answers_question: "q1",
  };
  expect(sameEditTarget(before, structuredClone(before), say)).toBe(true);
  const newer = structuredClone(before);
  newer.questions[0].contract_revision = 3;
  expect(sameEditTarget(before, newer, say)).toBe(false);
  expect(
    sameEditTarget(before, newer, { kind: "say", turn_id: "y", content: "hi" })
  ).toBe(true);
});

test("stop and take over are reachable from Continuous tasks through the original edits", async () => {
  const { calls } = render();
  await openTasks();
  click(button(dom.document.body, "Request stop"));
  await waitFor(() =>
    expect(
      calls.some(
        (c) =>
          c.edit?.kind === "control" &&
          c.edit.operation === "stop" &&
          c.edit.goal_id === "g1"
      )
    ).toBe(true)
  );
  click(button(dom.document.body, "Take over"));
  await waitFor(() =>
    expect(
      calls.some((c) => c.edit?.kind === "takeover" && c.edit.id === "g1")
    ).toBe(true)
  );
});

test("latest progress stays visible while earlier updates remain available on demand", async () => {
  const note = (id, goal_id, title) => ({
    id,
    goal_id,
    title,
    body: title,
    session_id: null,
    read: false,
    kind: "progress",
  });
  render({
    state: {
      notifications: [
        note("n1", "g1", "Earlier A"),
        note("n2", "g2", "Latest B"),
        note("n3", "g1", "Latest A"),
      ],
    },
  });
  await waitFor(() => expect(text()).toContain("Latest A"));
  expect(text()).toContain("Latest B");
  expect(text()).not.toContain("Earlier A");
  click(button(dom.document.body, "Earlier updates (1)"));
  await waitFor(() => expect(text()).toContain("Earlier A"));
});

test("polling and moving across Memory, tasks, settings and details keep the unsent draft and reading position", async () => {
  render({
    state: {
      conversation: Array.from({ length: 6 }, (_, i) =>
        turn({ id: `t${i}`, content: `History ${i}` })
      ),
    },
    onPoll: (state) => {
      state.revision += 1;
      state.turns += 1;
    },
  });
  await ready();
  await type("Half written plan");
  const scroller = () => dom.document.querySelector(".overscroll-contain");
  const viewport = scroller();
  expect(viewport).not.toBeNull();
  // happy-dom has no layout; give the viewport real scroll metrics so "reading up" is detectable.
  Object.defineProperty(viewport, "scrollHeight", {
    configurable: true,
    value: 2000,
  });
  Object.defineProperty(viewport, "clientHeight", {
    configurable: true,
    value: 300,
  });
  viewport.scrollTop = 120;
  viewport.dispatchEvent(new dom.window.Event("scroll", { bubbles: true }));
  await flush();
  for (const name of ["Memory", "Settings", "Continuous tasks"]) {
    click(button(dom.document.body, name));
    await flush();
    expect(composer().closest("[hidden]")).not.toBeNull();
    viewport.scrollTop = 0; // a hidden pane loses its offset in a real browser
    click(button(dom.document.body, "Back to conversation"));
    await flush();
    expect(composer().closest("[hidden]")).toBeNull();
    expect(draft()).toBe("Half written plan");
  }
  click(button(dom.document.body, "Continuous tasks"));
  await flush();
  click(button(dom.document.body, "Details"));
  await flush();
  expect(panel().hidden).toBe(false);
  await sleep(5300);
  click(button(dom.document.body, "Back to conversation"));
  await flush();
  expect(draft()).toBe("Half written plan");
  expect(scroller().scrollTop).toBe(120);
}, 15000);
