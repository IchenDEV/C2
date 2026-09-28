import { afterEach, describe, expect, test } from "bun:test";

// @ts-nocheck
import { act as reactAct } from "react";

import {
  activateDom,
  button,
  dom,
  flush,
  mount,
  restoreDom,
} from "./domTestHarness";

activateDom();
const { I18nProvider } = await import("../src/i18n");
const { SessionHeaderActions } =
  await import("../src/session/SessionHeaderActions");
const {
  TOOLTIP_FIRST_OPEN_DELAY,
  TOOLTIP_INSTANT_PHASE_TIMEOUT,
  TooltipProvider,
} = await import("../src/components/ui/tooltip");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

function renderActions(overrides = {}) {
  const calls: string[] = [];
  const callback = (name: string) => () => calls.push(name);
  const props = {
    gitAction: {
      primary: { id: "review_changes", destination: "source_control" },
      alternatives: [
        { id: "push", destination: "push" },
        { id: "view_pull_request", destination: "pull_request" },
      ],
      reason: { id: "local_changes", count: 2 },
      changeRequestLabel: "PR",
    },
    onAddAction: callback("add"),
    onOpenCursor: callback("cursor"),
    onOpenAntigravity: callback("antigravity"),
    onOpenFinder: callback("finder"),
    editorLaunchersAvailable: true,
    fileManagerLabel: "Finder",
    finderHint: "⌘O",
    onOpenSourceControl: callback("source-control"),
    onOpenPullRequest: callback("pull-request"),
    onCleanupWorktree: callback("cleanup"),
    onCheckpoint: callback("checkpoint"),
    onPush: callback("push"),
    onMoveTask: callback("move"),
    ...overrides,
  };
  const view = mount(
    <I18nProvider>
      <TooltipProvider>
        <SessionHeaderActions {...props} />
      </TooltipProvider>
    </I18nProvider>
  );
  return { calls, view };
}

async function press(element: Element) {
  await reactAct(async () => {
    element.dispatchEvent(
      new dom.window.PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      })
    );
    element.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
    );
  });
  await flush();
}

describe("SessionHeaderActions", () => {
  test("uses a deliberate first tooltip delay with an adjacent instant phase", () => {
    expect(TOOLTIP_FIRST_OPEN_DELAY).toBe(600);
    expect(TOOLTIP_INSTANT_PHASE_TIMEOUT).toBe(400);
  });

  test("keeps accessible names when responsive styling hides action labels", () => {
    activateDom();
    const { view } = renderActions();

    const group = view.container.querySelector(".session-header-actions");
    expect(group).not.toBeNull();
    for (const label of ["Add action", "Open", "Review changes"]) {
      const action = button(view.container, label);
      expect(action.classList.contains("session-header-action-main")).toBe(
        true
      );
      expect(
        action.querySelector(".session-header-action-label")?.textContent
      ).toBe(label);
    }

    view.unmount();
  });

  test("renders quiet icon-and-label actions with shared interaction states", () => {
    activateDom();
    const { view } = renderActions();
    const group = view.container.querySelector(".session-header-actions");
    expect(group?.classList.contains("gap-inline")).toBe(true);
    expect(group?.classList.contains("gap-2")).toBe(false);
    expect(group?.classList.contains("rounded-control")).toBe(false);
    expect(group?.classList.contains("p-0.5")).toBe(false);

    const addAction = button(view.container, "Add action");
    expect(addAction.dataset.variant).toBe("ghost");

    for (const label of ["Add action", "Open", "Review changes"]) {
      const action = button(view.container, label);
      expect(action.dataset.variant).toBe("ghost");
      expect(action.classList.contains("bg-fill-rest")).toBe(false);
      expect(action.classList.contains("hover:bg-fill-hover")).toBe(true);
      expect(
        action.querySelector(".session-header-action-icon")
      ).not.toBeNull();
      expect(
        action.querySelector(".session-header-action-label")?.textContent
      ).toBe(label);
      expect(action.classList.contains("button-toolbar-outline")).toBe(false);
    }

    expect(view.container.querySelectorAll("button")).toHaveLength(4);
    expect(
      view.container.querySelector('[data-slot="button-group"]')
    ).toBeNull();
    expect(
      view.container.querySelector(".session-header-git-action")
    ).not.toBeNull();
    expect(
      view.container.querySelector(".session-header-compact-action")
    ).toBeNull();

    for (const action of [...view.container.querySelectorAll("button")]) {
      expect(
        action.classList.contains("h-control") ||
          action.classList.contains("h-control-mini") ||
          action.classList.contains("size-control") ||
          action.classList.contains("size-control-mini") ||
          action.classList.contains("size-7")
      ).toBe(true);
    }

    view.unmount();
  });

  test("uses each complete toolbar item to expose its action menu", async () => {
    activateDom();
    const { calls, view } = renderActions();

    await press(button(view.container, "Add action"));
    await press(button(view.container, "Open"));
    expect(dom.document.body.textContent).toContain("Cursor");
    expect(dom.document.body.textContent).toContain("Antigravity");
    expect(dom.document.body.textContent).toContain("Finder");
    expect(dom.document.body.textContent).toContain("⌘O");

    const finderItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.includes("Finder"));
    if (!finderItem) throw new Error("Finder menu item not found");
    await press(finderItem);
    expect(calls).toEqual(["add", "finder"]);

    await press(button(view.container, "Open"));
    const moveItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.includes("Move task to device"));
    if (!moveItem) throw new Error("Move task menu item not found");
    await press(moveItem);
    expect(calls).toEqual(["add", "finder", "move"]);

    await press(button(view.container, "Review changes"));
    expect(calls).toEqual(["add", "finder", "move", "source-control"]);

    await press(button(view.container, "More Git actions"));
    expect(dom.document.body.textContent).toContain("Checkpoint now");
    expect(dom.document.body.textContent).toContain("Push");
    expect(dom.document.body.textContent).toContain("View PR");
    const pushItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.includes("Push"));
    if (!pushItem) throw new Error("Push menu item not found");
    await press(pushItem);
    expect(calls).toEqual(["add", "finder", "move", "source-control", "push"]);

    view.unmount();
  });

  test("disables the complete Git action while state is unavailable", () => {
    activateDom();
    const { view } = renderActions({
      gitAction: {
        primary: { id: "unavailable", destination: "none", disabled: true },
        alternatives: [],
        reason: { id: "not_repository" },
        changeRequestLabel: "change request",
      },
    });

    expect(button(view.container, "Source control unavailable").disabled).toBe(
      true
    );
    expect(
      button(view.container, "Source control unavailable").classList.contains(
        "disabled:opacity-50"
      )
    ).toBe(true);
    expect(
      view.container.querySelector('[aria-label="More Git actions"]')
    ).toBeNull();

    view.unmount();
  });

  test("labels a merge-ready route as a guarded review step", async () => {
    activateDom();
    const { calls, view } = renderActions({
      gitAction: {
        primary: { id: "merge_pull_request", destination: "pull_request" },
        alternatives: [{ id: "source_control", destination: "source_control" }],
        reason: { id: "merge_ready" },
        changeRequestLabel: "PR",
      },
    });

    await press(button(view.container, "Review & merge PR"));
    expect(calls).toEqual(["pull-request"]);

    view.unmount();
  });

  test("shows only the file manager destination off macOS", async () => {
    activateDom();
    const { calls, view } = renderActions({
      editorLaunchersAvailable: false,
      fileManagerLabel: "File manager",
      finderHint: "Ctrl+O",
    });

    await press(button(view.container, "Open"));
    expect(dom.document.body.textContent).not.toContain("Cursor");
    expect(dom.document.body.textContent).not.toContain("Antigravity");
    expect(dom.document.body.textContent).toContain("File manager");
    expect(dom.document.body.textContent).toContain("Ctrl+O");

    const fileManagerItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.includes("File manager"));
    if (!fileManagerItem) throw new Error("File manager menu item not found");
    await press(fileManagerItem);
    expect(calls).toEqual(["finder"]);

    view.unmount();
  });

  test("runs saved project actions from the header", async () => {
    activateDom();
    const calls: string[] = [];
    const { view } = renderActions({
      actions: [
        {
          id: "test",
          name: "Test",
          kind: "command",
          command: "bun test",
          prompt: "",
          keybinding: "Mod+Shift+T",
          preview_url: "",
          run_on_worktree_create: false,
          open_preview: false,
        },
      ],
      onRunAction: (action) => calls.push(action.id),
    });

    await press(button(view.container, "Test"));
    expect(calls).toEqual(["test"]);

    view.unmount();
  });
});
