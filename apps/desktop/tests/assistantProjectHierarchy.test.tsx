// @ts-nocheck
import { afterEach, expect, test } from "bun:test";

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

const mounted = [];
afterEach(() => {
  for (const root of mounted.splice(0)) root.unmount();
  dom.document.body.replaceChildren();
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const textbox = () => dom.document.querySelector('[role="textbox"]');

const mockHierarchy = {
  schema: 1,
  revision: 3,
  projects: [
    {
      id: "proj-atlas",
      name: "Project Atlas",
      status: "active",
      version: 1,
      created_at: "2026-10-06T00:00:00Z",
      bindings: [
        {
          id: "bind-atlas",
          path: "/work/atlas",
          version: 1,
          active: true,
          source: "user",
          revoked_at: null,
        },
      ],
    },
    {
      id: "proj-beacon",
      name: "Project Beacon",
      status: "active",
      version: 1,
      created_at: "2026-10-06T00:00:00Z",
      bindings: [
        {
          id: "bind-beacon",
          path: "/work/beacon",
          version: 1,
          active: true,
          source: "user",
          revoked_at: null,
        },
      ],
    },
  ],
  instructions: [
    {
      scope: { kind: "global" },
      revision: 1,
      text: "Global standard: prefer TypeScript and Rust.",
      hash: "1111222233334444",
      reach: "chief",
      proposal_id: "prop-1",
      confirmed_by: "user",
      confirmed_at: "2026-10-06T01:00:00Z",
      receipt: "rcpt-1",
    },
    {
      scope: { kind: "project", project_id: "proj-atlas" },
      revision: 2,
      text: "Atlas rule: ensure all API endpoints return JSON.",
      hash: "aaaabbbbccccdddd",
      reach: "project_manager",
      proposal_id: "prop-2",
      confirmed_by: "user",
      confirmed_at: "2026-10-06T02:00:00Z",
      receipt: "rcpt-2",
    },
  ],
  proposals: [],
  shares: [
    {
      id: "share-1",
      memory_id: "mem-common",
      source_scope: "codetwo://chief-of-staff",
      content_hash: "12345678abcdef01",
      target: { kind: "project", project_id: "proj-atlas" },
      reach: "project_manager",
      state: "approved",
      proposer: "chief",
      created_at: "2026-10-06T02:30:00Z",
      decided_at: "2026-10-06T02:31:00Z",
    },
  ],
  goal_owners: {
    "goal-atlas-1": {
      project_id: "proj-atlas",
      binding_id: "bind-atlas",
      epoch: 1,
    },
  },
};

function render({ preference = "en", extraTurns = [] } = {}) {
  const calls = [];
  const snapshot = {
    state: {
      revision: 10,
      settings: {
        enabled: true,
        projects: ["/work/atlas", "/work/beacon"],
        provider: "codex",
        model: null,
        reasoning_effort: null,
        concurrency: 1,
        turn_limit: 10,
        dispatch_limit: 5,
      },
      goals: [
        {
          id: "goal-atlas-1",
          project_path: "/work/atlas",
          title: "Deliver Atlas API Gateway",
          acceptance: "Gateway forwards requests",
          priority: 1,
          contract_revision: 1,
          dependencies: [],
          status: "active",
          next_step: "Implement router",
          blocker: "",
          assignments: [],
          verdict: null,
        },
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
      conversation: [
        {
          id: "turn-global-1",
          created_at: "2026-10-06T03:00:00Z",
          author: "assistant",
          content: "Welcome. Global Chief of Staff standing by.",
          reply_to: null,
          project_paths: [],
          status: "handled",
          goal_ids: [],
          question_ids: [],
          error: null,
          source_run_id: null,
          actor: "chief",
        },
        {
          id: "turn-atlas-1",
          created_at: "2026-10-06T03:05:00Z",
          author: "assistant",
          content:
            "Project Atlas manager here. Router implementation is ready for review.",
          reply_to: null,
          project_paths: ["/work/atlas"],
          status: "handled",
          goal_ids: ["goal-atlas-1"],
          question_ids: [],
          error: null,
          source_run_id: null,
          actor: "project:proj-atlas",
        },
        ...extraTurns,
      ],
      memory_proposals: [],
      hierarchy: mockHierarchy,
    },
    sessions: [],
    memories: [],
  };
  const api = {
    catalog: async () => [
      [
        { path: "/work/atlas", name: "Atlas Workspace" },
        { path: "/work/beacon", name: "Beacon Workspace" },
      ],
      [],
    ],
    snapshot: async () => structuredClone(snapshot),
    edit: async (revision, edit) => {
      calls.push({ revision, edit });
      return snapshot.state;
    },
    memory: async () => {},
  };
  mounted.push(
    mount(
      <I18nProvider preferenceOverride={preference}>
        <AssistantWorkspace
          activity={<p>Execution Activity</p>}
          api={api}
          onSelect={() => {}}
          onClose={() => {}}
        />
      </I18nProvider>
    )
  );
  return calls;
}

async function typeHtml(html) {
  const root = textbox();
  root.focus();
  const inline = root?.querySelector(".bn-inline-content") ?? root;
  if (!inline) return;
  const range = dom.document.createRange();
  range.selectNodeContents(inline);
  const selection = dom.window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  inline.innerHTML = html;
  inline.dispatchEvent(
    new dom.window.InputEvent("input", {
      bubbles: true,
      inputType: "insertText",
    })
  );
  await flush();
  await sleep(40);
  await flush();
}

test("renders scope switcher defaulting to Global and displays actor badges on turns", async () => {
  render();
  await waitFor(() => expect(textbox()).not.toBeNull());

  // Scope switcher button exists and displays Global
  const switcher = dom.document.querySelector('[data-testid="scope-switcher"]');
  expect(switcher).not.toBeNull();
  expect(switcher.textContent).toContain("Global");

  // Turns with actor render distinct attribution badges
  const globalActor = dom.document.querySelector(
    '[data-testid="turn-actor-turn-global-1"]'
  );
  expect(globalActor).not.toBeNull();
  expect(globalActor.textContent).toContain("Global");

  const atlasActor = dom.document.querySelector(
    '[data-testid="turn-actor-turn-atlas-1"]'
  );
  expect(atlasActor).not.toBeNull();
  expect(atlasActor.textContent).toContain("Project Atlas");
});

test("scope switcher allows choosing Project Atlas and sends project-scoped message", async () => {
  const calls = render();
  await waitFor(() => expect(textbox()).not.toBeNull());

  // Open the scope switcher dropdown
  const switcher = dom.document.querySelector('[data-testid="scope-switcher"]');
  click(switcher);
  await flush();

  // Find and select Project Atlas
  const atlasOption = dom.document.querySelector(
    '[data-testid="scope-option-proj-atlas"]'
  );
  expect(atlasOption).not.toBeNull();
  click(atlasOption);
  await flush();

  // Scope switcher should now display Project Atlas
  expect(switcher.textContent).toContain("Project Atlas");

  // Type and send a message
  await typeHtml("Please inspect Atlas endpoint security");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(calls.length).toBe(1));

  const sayEdit = calls[0].edit;
  expect(sayEdit.kind).toBe("say");
  expect(sayEdit.actor).toBe("project:proj-atlas");
  expect(sayEdit.project_paths).toEqual(["/work/atlas"]);
  expect(sayEdit.content).toContain("Atlas endpoint security");
});

test("switching back to Global sends chief-scoped message", async () => {
  const calls = render();
  await waitFor(() => expect(textbox()).not.toBeNull());

  // Switch to Project Atlas first
  const switcher = dom.document.querySelector('[data-testid="scope-switcher"]');
  click(switcher);
  await flush();
  click(dom.document.querySelector('[data-testid="scope-option-proj-atlas"]'));
  await flush();
  expect(switcher.textContent).toContain("Project Atlas");

  // Switch back to Global
  click(switcher);
  await flush();
  const globalOption = dom.document.querySelector(
    '[data-testid="scope-option-global"]'
  );
  expect(globalOption).not.toBeNull();
  click(globalOption);
  await flush();
  expect(switcher.textContent).toContain("Global");

  // Type and send message
  await typeHtml("Coordinate cross-project roadmap");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(calls.length).toBe(1));

  const sayEdit = calls[0].edit;
  expect(sayEdit.kind).toBe("say");
  expect(sayEdit.actor).toBe("chief");
  expect(sayEdit.content).toContain("cross-project roadmap");
});

test("on-demand observe button toggles scope inspector displaying instructions, memory and tasks", async () => {
  render();
  await waitFor(() => expect(textbox()).not.toBeNull());

  const observeButton = dom.document.querySelector(
    '[data-testid="observe-scope-button"]'
  );
  expect(observeButton).not.toBeNull();

  // Panel is closed by default
  expect(
    dom.document.querySelector('[data-testid="observe-scope-panel"]')
  ).toBeNull();

  // Toggle on Global scope observation
  click(observeButton);
  await flush();

  let panel = dom.document.querySelector('[data-testid="observe-scope-panel"]');
  expect(panel).not.toBeNull();
  expect(panel.textContent).toContain("Scope: Global");
  expect(panel.textContent).toContain(
    "Global standard: prefer TypeScript and Rust."
  );
  expect(panel.textContent).toContain("codetwo://chief-of-staff");

  // Switch to Project Atlas while observe panel is open
  const switcher = dom.document.querySelector('[data-testid="scope-switcher"]');
  click(switcher);
  await flush();
  click(dom.document.querySelector('[data-testid="scope-option-proj-atlas"]'));
  await flush();

  panel = dom.document.querySelector('[data-testid="observe-scope-panel"]');
  expect(panel).not.toBeNull();
  expect(panel.textContent).toContain("Scope: Project Atlas");
  expect(panel.textContent).toContain(
    "Atlas rule: ensure all API endpoints return JSON."
  );
  expect(panel.textContent).toContain("codetwo://managed-project/proj-atlas");
  expect(panel.textContent).toContain("Deliver Atlas API Gateway");

  // Toggle off observation
  click(observeButton);
  await flush();
  expect(
    dom.document.querySelector('[data-testid="observe-scope-panel"]')
  ).toBeNull();
});

test("Chinese localization applies to scope switcher and observe panel", async () => {
  render({ preference: "zh-CN" });
  await waitFor(() => expect(textbox()).not.toBeNull());

  const switcher = dom.document.querySelector('[data-testid="scope-switcher"]');
  expect(switcher.textContent).toContain("全局");

  const observeButton = dom.document.querySelector(
    '[data-testid="observe-scope-button"]'
  );
  click(observeButton);
  await flush();

  const panel = dom.document.querySelector(
    '[data-testid="observe-scope-panel"]'
  );
  expect(panel).not.toBeNull();
  expect(panel.textContent).toContain("当前范围：全局");
  expect(panel.textContent).toContain("作用域指令：");
  expect(panel.textContent).toContain("关联持续任务：");
});
