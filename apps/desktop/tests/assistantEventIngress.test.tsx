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
dom.window.HTMLCanvasElement.prototype.getContext = () =>
  ({ filter: "" }) as never;
const { AssistantWorkspace } =
  await import("../src/assistant/AssistantWorkspace");
const { I18nProvider } = await import("../src/i18n");
const { principalKey } = await import("../src/assistant/api");
const mounted = [];
afterEach(() => {
  for (const root of mounted.splice(0)) root.unmount();
  dom.document.body.replaceChildren();
});

const goal = (id, title, project_path, status = "active") => ({
  id,
  project_path,
  title,
  acceptance: "done",
  priority: 1,
  status,
  contract_revision: 1,
  dependencies: [],
  next_step: "",
  blocker: "",
  assignments: [],
  verdict: null,
});
const source = (patch = {}) => ({
  source_id: "src_1",
  principal: {
    plugin: "bundle:mail",
    realm: "user",
    connector_id: "mail-inbox",
  },
  provider: "imap",
  account_scope: "me@example.com",
  resource_filter: ["r1"],
  actor_filter: [],
  project_paths: ["/a"],
  goal_ids: ["g1"],
  streams: ["inbox"],
  version: 1,
  enabled: true,
  ...patch,
});
const health = (patch = {}) => ({
  key: "src_1",
  kind: "source",
  state: "ok",
  accepted: 0,
  filtered: 0,
  duplicate: 0,
  conflicts: 0,
  rejected: 0,
  unbound: 0,
  gap_count: 0,
  gap_first: null,
  gap_last: null,
  detail: null,
  ...patch,
});
const connector = {
  plugin: "bundle:mail",
  realm: "user",
  connector_id: "mail-inbox",
  provider: "imap",
  label: "Mail",
};

function render({
  sources = [],
  obs = { health: [], receipts: [], connectors: [connector] },
  withObservations = true,
} = {}) {
  const calls = [];
  const observed = [];
  const store = {
    state: {
      revision: 1,
      settings: {
        enabled: true,
        projects: ["/a"],
        provider: "codex",
        model: null,
        reasoning_effort: null,
        concurrency: 1,
        turn_limit: 10,
        dispatch_limit: 5,
      },
      goals: [
        goal("g1", "Ship report", "/a"),
        goal("g2", "Old finished thing", "/a", "completed"),
        goal("g3", "Unmanaged goal", "/b"),
      ],
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
      sources,
    },
    obs,
  };
  const api = {
    catalog: async () => [
      [
        { path: "/a", name: "Project A" },
        { path: "/b", name: "Project B" },
      ],
      [],
    ],
    snapshot: async () =>
      structuredClone({ state: store.state, sessions: [], memories: [] }),
    edit: async (revision, edit) => {
      calls.push({ revision, edit });
      return store.state;
    },
    memory: async () => {},
    ...(withObservations && {
      observations: async (limit) => {
        observed.push(limit);
        return structuredClone(store.obs);
      },
    }),
  };
  mounted.push(
    mount(
      <I18nProvider preferenceOverride="en">
        <AssistantWorkspace
          activity={<p>Real execution view</p>}
          api={api}
          onSelect={() => {}}
          onClose={() => {}}
        />
      </I18nProvider>
    )
  );
  return { calls, observed, store };
}

async function openSources() {
  await waitFor(() => button(dom.document.body, "More tools"));
  click(button(dom.document.body, "More tools"));
  const find = () =>
    [...dom.document.querySelectorAll('[role="menuitem"]')].find(
      (e) => e.textContent.trim() === "Event sources"
    );
  await waitFor(() => expect(find()).toBeDefined());
  find().dispatchEvent(
    new dom.window.PointerEvent("pointerdown", {
      bubbles: true,
      pointerType: "mouse",
    })
  );
  click(find());
  await flush();
}
const change = (element, value) => {
  Object.getOwnPropertyDescriptor(
    dom.window.HTMLInputElement.prototype,
    "value"
  ).set.call(element, value);
  Simulate.change(element);
  return flush();
};
const label = (text) =>
  [...dom.document.querySelectorAll("label")].find((l) =>
    l.textContent.includes(text)
  );
const field = (text) => label(text).querySelector("input");
const note = () => dom.document.querySelector('[role="note"]');
const submit = (form) =>
  form.dispatchEvent(
    new dom.window.Event("submit", { bubbles: true, cancelable: true })
  );
async function choose(name, value) {
  click(dom.document.querySelector(`[role="combobox"][aria-label="${name}"]`));
  const find = () =>
    [...dom.document.querySelectorAll('[role="option"]')].find(
      (e) => e.textContent === value
    );
  await waitFor(() => expect(find()).toBeDefined());
  find().dispatchEvent(
    new dom.window.PointerEvent("pointerdown", {
      bubbles: true,
      pointerType: "mouse",
    })
  );
  click(find());
  await flush();
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("the default conversation is unchanged: no inspector, no fetch, no native details", async () => {
  const { observed } = render({ sources: [source()] });
  await waitFor(() =>
    expect(button(dom.document.body, "More tools")).toBeDefined()
  );
  expect(dom.document.body.textContent).not.toContain("Event sources");
  expect(dom.document.body.textContent).not.toContain("me@example.com");
  expect(observed).toEqual([]);
  await openSources();
  await waitFor(() => expect(observed.length).toBeGreaterThan(0));
  expect(observed[0]).toBe(50);
  expect(dom.document.body.textContent).toContain("me@example.com");
  expect(dom.document.querySelector("details,summary")).toBeNull();
  // The create form only exists after the explicit button.
  expect(
    dom.document.querySelector('form[aria-label="Add event source"]')
  ).toBeNull();
});

test("an API without observations shows a load error and invents no connector", async () => {
  render({ sources: [source()], withObservations: false });
  await openSources();
  await waitFor(() =>
    expect(dom.document.querySelector('[role="alert"]').textContent).toContain(
      "not available"
    )
  );
  // Persisted bindings still show; nothing can be created without a real catalog.
  expect(dom.document.body.textContent).toContain("mail-inbox");
  expect(button(dom.document.body, "Add source").disabled).toBe(true);
});

test("an empty catalog offers no source and says why", async () => {
  render({ obs: { health: [], receipts: [], connectors: [] } });
  await openSources();
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain(
      "No installed, trusted connector declares observations."
    )
  );
  expect(button(dom.document.body, "Add source").disabled).toBe(true);
});

test("creating a source sends the exact scoped binding and states the authority", async () => {
  const { calls } = render();
  await openSources();
  await waitFor(() =>
    expect(button(dom.document.body, "Add source").disabled).toBe(false)
  );
  click(button(dom.document.body, "Add source"));
  await flush();
  const form = dom.document.querySelector(
    'form[aria-label="Add event source"]'
  );
  expect(form).not.toBeNull();
  // Required fields missing: cannot save.
  expect(button(form, "Add source").disabled).toBe(true);
  await choose("Connector", "Mail · mail-inbox (bundle:mail)");
  await change(field("Account"), "me@example.com");
  await change(field("Streams"), "inbox, inbox, alerts");
  await change(field("resource IDs"), "r1, r2");
  await change(field("sender IDs"), "a1");
  // Only the managed project is offered; unmanaged and finished goals are not.
  expect(form.textContent).toContain("Project A");
  expect(form.textContent).not.toContain("Project B");
  click(label("Project A").querySelector("input"));
  await flush();
  expect(form.textContent).toContain("Ship report");
  expect(form.textContent).not.toContain("Old finished thing");
  expect(form.textContent).not.toContain("Unmanaged goal");
  // Empty goals mean attention, never "all goals".
  expect(form.textContent).toContain("not routed to every goal");
  expect(note().textContent).toContain("Observe only");
  expect(note().textContent).toContain('["bundle:mail","user","mail-inbox"]');
  expect(note().textContent).toContain("me@example.com");
  expect(note().textContent).toContain("cannot command, dispatch work");
  click(label("Ship report").querySelector("input"));
  await flush();
  expect(form.textContent).not.toContain("not routed to every goal");
  expect(button(form, "Add source").disabled).toBe(false);
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0]).toEqual({
    revision: 1,
    edit: {
      kind: "source",
      binding: {
        source_id: null,
        expected_version: 0,
        principal: {
          plugin: "bundle:mail",
          realm: "user",
          connector_id: "mail-inbox",
        },
        provider: "imap",
        account_scope: "me@example.com",
        resource_filter: ["r1", "r2"],
        actor_filter: ["a1"],
        project_paths: ["/a"],
        goal_ids: ["g1"],
        streams: ["inbox", "alerts"],
        enabled: true,
      },
    },
  });
});

test("disabling sends the current version and keeps identity and scope untouched", async () => {
  const { calls } = render({
    sources: [source({ version: 3 })],
    obs: { health: [health()], receipts: [], connectors: [connector] },
  });
  await openSources();
  await waitFor(() =>
    expect(
      button(dom.document.body, "Disable mail-inbox me@example.com")
    ).toBeDefined()
  );
  click(button(dom.document.body, "Disable mail-inbox me@example.com"));
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit.binding).toEqual({
    source_id: "src_1",
    expected_version: 3,
    principal: source().principal,
    provider: "imap",
    account_scope: "me@example.com",
    resource_filter: ["r1"],
    actor_filter: [],
    project_paths: ["/a"],
    goal_ids: ["g1"],
    streams: ["inbox"],
    enabled: false,
  });
});

test("a poll that bumps the version keeps the draft but blocks submit until the user rebases", async () => {
  const { calls, store } = render({
    sources: [source()],
    obs: { health: [health()], receipts: [], connectors: [connector] },
  });
  await openSources();
  click(button(dom.document.body, "Edit mail-inbox me@example.com"));
  await flush();
  const form = dom.document.querySelector(
    'form[aria-label="Edit event source"]'
  );
  // Identity is read-only when editing.
  expect(form.textContent).toContain('["bundle:mail","user","mail-inbox"]');
  expect(
    dom.document.querySelector('[role="combobox"][aria-label="Connector"]')
  ).toBeNull();
  await change(field("Streams"), "inbox, extra");
  // Another writer changes the binding; the 5s poll sees it.
  store.state.sources = [source({ version: 2, resource_filter: ["r9"] })];
  await waitFor(
    () =>
      expect(
        dom.document.querySelector('form [role="alert"]').textContent
      ).toContain("v1 → v2"),
    7000
  );
  expect(field("Streams").value).toBe("inbox, extra");
  expect(button(form, "Save source").disabled).toBe(true);
  submit(form);
  await flush();
  expect(calls).toEqual([]);
  click(button(form, "Keep my draft on the latest version"));
  await flush();
  expect(button(form, "Save source").disabled).toBe(false);
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit.binding.expected_version).toBe(2);
  expect(calls[0].edit.binding.source_id).toBe("src_1");
  expect(calls[0].edit.binding.streams).toEqual(["inbox", "extra"]);
}, 20000);

test("health, unbound traffic and receipts keep unknown, handled and each deferred state distinct", async () => {
  render({
    sources: [source()],
    obs: {
      health: [
        health({
          state: "needs_capacity",
          accepted: 12,
          gap_count: 2,
          gap_first: "t1",
          gap_last: "t2",
          detail:
            "source src_1: inputs 2000/2000. Raise the limit with the current binding version 1 to continue.",
        }),
        health({
          key: "principal:bundle:mail|user|other",
          kind: "principal",
          unbound: 4,
        }),
      ],
      receipts: [
        {
          observation_id: "obs_a",
          goal_id: "g1",
          source_id: "src_1",
          binding_version: 1,
          project_path: "/a",
          state: "unknown",
          run_id: null,
          output_id: null,
          seq: 1,
        },
        {
          observation_id: "obs_b",
          goal_id: "g1",
          source_id: "src_1",
          binding_version: 1,
          project_path: "/a",
          state: "handled",
          run_id: "run_1",
          output_id: "out_1",
          seq: 2,
        },
        {
          observation_id: "obs_c",
          goal_id: "g1",
          source_id: "src_1",
          binding_version: 1,
          project_path: "/a",
          state: "deferred_goal_state",
          run_id: null,
          output_id: null,
          seq: 3,
        },
        {
          observation_id: "obs_d",
          goal_id: "g1",
          source_id: "src_1",
          binding_version: 1,
          project_path: "/a",
          state: "deferred_budget",
          run_id: null,
          output_id: null,
          seq: 4,
        },
        {
          observation_id: "obs_e",
          goal_id: "g1",
          source_id: "src_1",
          binding_version: 1,
          project_path: "/a",
          state: "mystery_state",
          run_id: null,
          output_id: null,
          seq: 5,
        },
      ],
      connectors: [connector],
    },
  });
  await openSources();
  await waitFor(() => expect(dom.document.body.textContent).toContain("obs_e"));
  const row = (id) =>
    [...dom.document.querySelectorAll("li")].find((l) =>
      l.textContent.includes(id)
    ).textContent;
  expect(row("obs_a")).toContain("Unknown outcome");
  expect(row("obs_a")).not.toContain("Handled");
  expect(row("obs_b")).toContain("Handled");
  expect(row("obs_b")).toContain("run_1");
  expect(row("obs_c")).toContain("Deferred: goal state");
  expect(row("obs_d")).toContain("Deferred: budget");
  expect(row("obs_e")).toContain("mystery_state");
  expect(row("obs_a")).toContain("mail-inbox · me@example.com");
  const card = dom.document.querySelector("article");
  expect(card.textContent).toContain("Needs capacity");
  expect(card.textContent).toContain("inputs 2000/2000");
  expect(card.textContent).toContain("Gaps 2 (t1 – t2)");
  expect(dom.document.body.textContent).toContain(
    "4 event(s) from bundle:mail|user|other arrived without a binding; no content was kept."
  );
});

test("raising capacity targets the exact binding version and survives a stale poll", async () => {
  const { calls, store } = render({
    sources: [source()],
    obs: {
      health: [health({ state: "needs_capacity", detail: "inputs 2000/2000" })],
      receipts: [],
      connectors: [connector],
    },
  });
  await openSources();
  await waitFor(() => button(dom.document.body, "Raise history limit"));
  click(button(dom.document.body, "Raise history limit"));
  await flush();
  const form = dom.document.querySelector(
    'form[aria-label="Raise history limit"]'
  );
  expect(field("This source").value).toBe("4000");
  await change(field("This source"), "6000");
  await change(field("Global history"), "20000");
  store.state.sources = [source({ version: 2 })];
  await waitFor(
    () =>
      expect(form.querySelector('[role="alert"]').textContent).toContain(
        "v1 → v2"
      ),
    7000
  );
  expect(field("This source").value).toBe("6000");
  submit(form);
  await flush();
  expect(calls).toEqual([]);
  click(button(form, "Keep my numbers on the latest version"));
  await flush();
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit).toEqual({
    kind: "source_capacity",
    source_id: "src_1",
    binding_version: 2,
    source_history_limit: 6000,
    global_history_limit: 20000,
  });
}, 20000);

test("out-of-range capacity values cannot be submitted", async () => {
  const { calls } = render({
    sources: [source()],
    obs: {
      health: [health({ state: "needs_capacity", detail: "full" })],
      receipts: [],
      connectors: [connector],
    },
  });
  await openSources();
  await waitFor(() => button(dom.document.body, "Raise history limit"));
  click(button(dom.document.body, "Raise history limit"));
  await flush();
  const form = dom.document.querySelector(
    'form[aria-label="Raise history limit"]'
  );
  await change(field("This source"), "1999");
  await flush();
  expect(button(form, "Raise limit").disabled).toBe(true);
  await change(field("This source"), "");
  await change(field("Global history"), "");
  await flush();
  expect(button(form, "Raise limit").disabled).toBe(true);
  submit(form);
  await sleep(10);
  expect(calls).toEqual([]);
});

const stream = (checkpoint, patch = {}) => ({
  source_id: "src_1",
  stream_id: "inbox",
  checkpoint,
  ...patch,
});
const resetObs = (checkpoint = "cur-1", extra = {}) => ({
  health: [
    health({ state: "needs_reset", detail: "cursor rejected by provider" }),
  ],
  receipts: [],
  connectors: [connector],
  streams: [stream(checkpoint)],
  resets: [],
  ...extra,
});
const resetButton = () =>
  Array.from(dom.document.querySelectorAll("button")).find(
    (b) =>
      b.getAttribute("aria-label") === "Reset cursor mail-inbox me@example.com"
  );
const resetForm = () =>
  dom.document.querySelector('form[aria-label="Reset stream cursor"]');
async function openReset() {
  await openSources();
  await waitFor(() => expect(resetButton()).toBeDefined());
  click(resetButton());
  await flush();
  expect(resetForm()).not.toBeNull();
  return resetForm();
}

test("principalKey cannot collide across delimiter-bearing identities", () => {
  const a = { plugin: "a|b", realm: "c", connector_id: "d" };
  const b = { plugin: "a", realm: "b|c", connector_id: "d" };
  expect(principalKey(a)).not.toBe(principalKey(b));
  expect(principalKey(a)).toBe('["a|b","c","d"]');
});

test("a reset is only offered at needs_reset", async () => {
  render({
    sources: [source()],
    obs: {
      ...resetObs(),
      health: [health()],
    },
  });
  await openSources();
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("Receiving")
  );
  expect(resetButton()).toBeUndefined();
});

test("approving sends exactly the reviewed SourceReset and nothing earlier", async () => {
  const { calls } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  // Old checkpoint is shown read-only and exact; the only stream is preselected.
  expect(form.textContent).toContain('"cur-1"');
  expect(button(form, "Approve reset").disabled).toBe(true);
  await change(field("New baseline (exact text"), "new-9");
  expect(button(form, "Approve reset").disabled).toBe(true); // reason missing
  await change(field("Gap reason"), "  provider expired the cursor  ");
  expect(button(form, "Approve reset").disabled).toBe(false);
  const review = form.querySelector('[role="note"]').textContent;
  expect(review).toContain("src_1");
  expect(review).toContain('["bundle:mail","user","mail-inbox"]');
  expect(review).toContain("me@example.com");
  expect(review).toContain("Project A");
  expect(review).toContain("Ship report");
  expect(review).toContain('"cur-1" → "new-9"');
  expect(review).toContain("provider expired the cursor");
  expect(review).toContain("fetches nothing, resets nothing and sends nothing");
  // Typing, reviewing and cancelling never reach Core.
  expect(calls).toEqual([]);
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0]).toEqual({
    revision: 1,
    edit: {
      kind: "source_reset",
      source_id: "src_1",
      binding_version: 1,
      stream_id: "inbox",
      checkpoint_before: "cur-1",
      checkpoint_after: "new-9",
      reason: "provider expired the cursor",
    },
  });
});

test("cancelling and an unfinished form never send a reset", async () => {
  const { calls } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  await change(field("New baseline (exact text"), "x");
  submit(form); // reason missing: guarded even if the button is bypassed
  await sleep(10);
  click(button(form, "Cancel"));
  await flush();
  expect(resetForm()).toBeNull();
  expect(calls).toEqual([]);
});

test("null and empty-string checkpoints stay different on screen and on the wire", async () => {
  const { calls, observed, store } = render({
    sources: [source()],
    obs: resetObs(null),
  });
  let form = await openReset();
  expect(form.textContent).toContain("null (no checkpoint)");
  await change(field("Gap reason"), "empty");
  // Unchecked + empty text is the empty string, not null.
  expect(button(form, "Approve reset").disabled).toBe(false);
  expect(form.querySelector('[role="note"]').textContent).toContain('→ ""');
  store.obs = resetObs("");
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit.checkpoint_before).toBeNull();
  expect(calls[0].edit.checkpoint_after).toBe("");
  // Now the checkpoint is the empty string and the new baseline is a real null.
  const seen = observed.length;
  await waitFor(() => expect(observed.length).toBeGreaterThan(seen), 6500);
  await flush();
  await waitFor(() => expect(resetButton()).toBeDefined());
  click(resetButton());
  await flush();
  form = resetForm();
  expect(form.textContent).toContain('Current checkpoint (read-only): ""');
  click(label("is null").querySelector("input"));
  await flush();
  expect(field("New baseline (exact").disabled).toBe(true);
  await change(field("Gap reason"), "back to none");
  expect(form.querySelector('[role="note"]').textContent).toContain(
    "null (no checkpoint)"
  );
  submit(form);
  await waitFor(() => expect(calls.length).toBe(2));
  expect(calls[1].edit.checkpoint_before).toBe("");
  expect(calls[1].edit.checkpoint_after).toBeNull();
}, 20000);

test("over-long baselines and reasons cannot be approved", async () => {
  const { calls } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  await change(field("Gap reason"), "ok");
  await change(field("New baseline (exact text"), "é".repeat(2049)); // 4098 bytes
  expect(button(form, "Approve reset").disabled).toBe(true);
  await change(field("New baseline (exact text"), "é".repeat(2048));
  expect(button(form, "Approve reset").disabled).toBe(false);
  await change(field("Gap reason"), "r".repeat(257));
  expect(button(form, "Approve reset").disabled).toBe(true);
  submit(form);
  await sleep(10);
  expect(calls).toEqual([]);
});

test("a Core that does not report stream checkpoints cannot pin a reset", async () => {
  const { calls } = render({
    sources: [source()],
    obs: resetObs("cur-1", { streams: undefined }),
  });
  const form = await openReset();
  expect(form.textContent).toContain("not reported");
  await change(field("Gap reason"), "why");
  await change(field("New baseline (exact text"), "n");
  expect(button(form, "Approve reset").disabled).toBe(true);
  submit(form);
  await sleep(10);
  expect(calls).toEqual([]);
});

test("a binding change keeps the reset draft, blocks approval, and needs an explicit rebase", async () => {
  const { calls, store } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  await change(field("New baseline (exact text"), "new-9");
  await change(field("Gap reason"), "expired");
  store.state.sources = [source({ version: 2, resource_filter: ["r9"] })];
  await waitFor(
    () =>
      expect(form.querySelector('[role="alert"]').textContent).toContain(
        "v1 → v2"
      ),
    7000
  );
  expect(field("New baseline (exact text").value).toBe("new-9");
  expect(field("Gap reason").value).toBe("expired");
  expect(button(form, "Approve reset").disabled).toBe(true);
  submit(form);
  await flush();
  expect(calls).toEqual([]);
  click(button(form, "Keep my draft on the latest version"));
  await flush();
  expect(button(form, "Approve reset").disabled).toBe(false);
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit.binding_version).toBe(2);
  expect(calls[0].edit.checkpoint_before).toBe("cur-1");
  expect(calls[0].edit.checkpoint_after).toBe("new-9");
}, 20000);

test("a disabled source cannot be rebased onto and blocks the reset", async () => {
  const { calls, store } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  await change(field("New baseline (exact text"), "n");
  await change(field("Gap reason"), "r");
  store.state.sources = [source({ version: 2, enabled: false })];
  await waitFor(
    () => expect(form.querySelector('[role="alert"]')).not.toBeNull(),
    7000
  );
  expect(
    Array.from(form.querySelectorAll("button")).find(
      (b) => b.textContent === "Keep my draft on the latest version"
    )
  ).toBeUndefined();
  expect(button(form, "Approve reset").disabled).toBe(true);
  submit(form);
  await sleep(10);
  expect(calls).toEqual([]);
}, 20000);

test("a moved checkpoint keeps the draft but never approves the old cursor until it is adopted", async () => {
  const { calls, store } = render({ sources: [source()], obs: resetObs() });
  const form = await openReset();
  await change(field("New baseline (exact text"), "new-9");
  await change(field("Gap reason"), "expired");
  store.obs = resetObs("cur-2");
  await waitFor(
    () =>
      expect(form.querySelector('[role="alert"]').textContent).toContain(
        '"cur-1", it is now "cur-2"'
      ),
    7000
  );
  expect(field("New baseline (exact text").value).toBe("new-9");
  expect(field("Gap reason").value).toBe("expired");
  expect(button(form, "Approve reset").disabled).toBe(true);
  // The read-only line shows what the user reviewed; the review does not silently switch.
  expect(form.querySelector('[role="note"]').textContent).toContain(
    '"cur-1" → "new-9"'
  );
  submit(form);
  await flush();
  expect(calls).toEqual([]);
  click(button(form, "Adopt the current checkpoint"));
  await flush();
  expect(form.querySelector('[role="note"]').textContent).toContain(
    '"cur-2" → "new-9"'
  );
  expect(button(form, "Approve reset").disabled).toBe(false);
  submit(form);
  await waitFor(() => expect(calls.length).toBe(1));
  expect(calls[0].edit.checkpoint_before).toBe("cur-2");
  expect(calls[0].edit.checkpoint_after).toBe("new-9");
  expect(calls[0].edit.binding_version).toBe(1);
}, 20000);

const approved = (patch = {}) => ({
  approval_ref: "reset_ref_7",
  source_id: "src_1",
  binding_version: 1,
  stream_id: "inbox",
  checkpoint_before: "cur-1",
  checkpoint_after: null,
  reason: "provider expired the cursor",
  approved_at_ms: 1_700_000_000_000,
  ...patch,
});

test("an approved reference shows exact old and new values and does not claim the adapter finished", async () => {
  render({
    sources: [source()],
    obs: resetObs("cur-1", { resets: [approved()] }),
  });
  await openSources();
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("reset_ref_7")
  );
  const box = dom.document.querySelector(
    '[aria-label="Approved reset reset_ref_7"]'
  );
  expect(box.textContent).toContain('"cur-1"');
  expect(box.textContent).toContain("null (no checkpoint)");
  expect(box.textContent).toContain("provider expired the cursor");
  expect(box.textContent).toContain("2023-11-14T22:13:20.000Z");
  expect(box.textContent).toContain("nothing was reset yet");
  expect(box.textContent).not.toMatch(/completed|finished|done/i);
});

test("a moved checkpoint with the approval still listed is not reported as completed", async () => {
  render({
    sources: [source()],
    obs: resetObs("somewhere-else", { resets: [approved()] }),
  });
  await openSources();
  await waitFor(() =>
    expect(dom.document.body.textContent).toContain("reset_ref_7")
  );
  const box = dom.document.querySelector(
    '[aria-label="Approved reset reset_ref_7"]'
  );
  expect(box.textContent).toContain("not confirmed as used");
  expect(box.textContent).not.toContain("nothing was reset yet");
});
