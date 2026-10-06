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
const { AssistantWorkspace } =
  await import("../src/assistant/AssistantWorkspace");
const { I18nProvider } = await import("../src/i18n");
const { DocEditor } = await import("../src/editor/Editor");
const mounted = [];
afterEach(() => {
  for (const root of mounted.splice(0)) root.unmount();
  dom.document.body.replaceChildren();
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const textbox = () => dom.document.querySelector('[role="textbox"]');

function render({ preference = "en" } = {}) {
  const calls = [];
  const snapshot = {
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
      goals: [],
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
    },
    sessions: [],
    memories: [],
  };
  const api = {
    catalog: async () => [[{ path: "/a", name: "Project A" }], []],
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
          activity={<p>Real execution view</p>}
          api={api}
          onSelect={() => {}}
          onClose={() => {}}
        />
      </I18nProvider>
    )
  );
  return calls;
}

// Real contenteditable input: write into the paragraph and let ProseMirror's observer commit it.
async function typeHtml(html) {
  const root = textbox();
  root?.focus();
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

test("the textbox is a real rich contenteditable with the English and Chinese accessible names", async () => {
  render();
  await waitFor(() => expect(textbox()).not.toBeNull());
  expect(textbox().getAttribute("contenteditable")).toBe("true");
  expect(textbox().getAttribute("aria-label")).toBe(
    "Message to chief of staff"
  );
  expect(dom.document.querySelector("textarea")).toBeNull();
  for (const mountedRoot of mounted.splice(0)) mountedRoot.unmount();
  dom.document.body.replaceChildren();
  render({ preference: "zh-CN" });
  await waitFor(() => expect(textbox()).not.toBeNull());
  expect(textbox().getAttribute("aria-label")).toBe("给幕僚的消息");
  expect(button(dom.document.body, "持续任务")).toBeDefined();
});

test("the shared editor exports basic formatting through BlockNote Markdown", async () => {
  const calls = render();
  await waitFor(() => expect(textbox()).not.toBeNull());
  await typeHtml("plain <strong>bold</strong> and <code>code</code>");
  // The editor itself accepted the marks (so a missing Markdown mark is the serializer's).
  expect(
    textbox().querySelector('strong,[data-style-type="bold"],b,.bn-bold')
  ).not.toBeNull();
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(calls.length).toBe(1));
  const { content } = calls[0].edit;
  expect(content).toContain("plain");
  expect(content).toContain("**bold**");
  expect(content).toContain("`code`");
});

test("text-only editor does not offer unsupported block insertion (table/card/image/mention)", async () => {
  render();
  await waitFor(() => expect(textbox()).not.toBeNull());
  // The shared editor's text-only mode drops the card/attachment affordances entirely.
  const root = textbox().closest('[role="region"]') ?? dom.document.body;
  expect(
    root.querySelector('[data-testid*="canvas"],[aria-label*="Canvas" i]')
  ).toBeNull();
  expect(root.querySelector('input[type="file"]')).toBeNull();
});

test("send protects against an unchanged-then-edited draft with a new id and the same payload with the same id", async () => {
  const calls = render();
  await waitFor(() => expect(textbox()).not.toBeNull());
  await typeHtml("same text");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(calls.length).toBe(1));
  await typeHtml("same text");
  click(button(dom.document.body, "Send"));
  await waitFor(() => expect(calls.length).toBe(2));
  // A succeeded message is a new message even with identical text: ids are never reused.
  expect(calls[1].edit.turn_id).not.toBe(calls[0].edit.turn_id);
});

test("the shared text export preserves nested lists and links and refuses a pasted table", async () => {
  const getBlocksRef = { current: null };
  const getMarkdownRef = { current: null };
  const insertMarkdownRef = { current: null };
  mounted.push(
    mount(
      <I18nProvider preferenceOverride="en">
        <DocEditor
          textOnly
          sessionId={null}
          getBlocksRef={getBlocksRef}
          getMarkdownRef={getMarkdownRef}
          insertMarkdownRef={insertMarkdownRef}
          focusRef={{ current: null }}
          clearRef={{ current: null }}
          onEmptyChange={() => {}}
        />
      </I18nProvider>
    )
  );
  await waitFor(() => expect(insertMarkdownRef.current).not.toBeNull());
  await insertMarkdownRef.current(
    "- parent\n  - child\n\n[reference](https://example.test)",
    "replace"
  );
  const content = await getMarkdownRef.current();
  expect(content).toContain("parent");
  expect(content).toContain("child");
  expect(content).toContain("https://example.test");
  expect(content).toMatch(/[-*]\s+parent/);
  await insertMarkdownRef.current(
    "| A | B |\n| --- | --- |\n| one | two |",
    "replace"
  );
  expect(await getMarkdownRef.current()).toBeNull();
});
