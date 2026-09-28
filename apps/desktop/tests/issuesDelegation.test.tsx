// @ts-nocheck
import { afterEach, describe, expect, mock, test } from "bun:test";

import { act as reactAct } from "react";

import {
  activateDom,
  button,
  dom,
  flush,
  maybeButton,
  mount,
  restoreDom,
} from "./domTestHarness";

activateDom();

const ISSUE = {
  id: "42",
  title: "Fix login",
  state: "open",
  url: "https://github.com/o/r/issues/42",
  body: "Steps to reproduce…",
  source: "github",
};

const realBridge = await import("../src/bridge");
void mock.module("../src/bridge", () => ({
  ...realBridge,
  listGithubIssues: async () => [ISSUE],
}));

const { IssuesModal } = await import("../src/issues/Issues");
const { I18nProvider } = await import("../src/i18n");

// Unmount before wiping the body: the modal renders through Radix portals directly into
// `document.body`, and tearing those nodes out from under a live React root corrupts later
// suites' cleanup on the shared happy-dom window.
const mountedRoots = [];

afterEach(() => {
  for (const mounted of mountedRoots.splice(0)) mounted.unmount();
  dom.document.body.replaceChildren();
  restoreDom();
});

function sceneInfo(overrides = {}) {
  return {
    reference: "builtin:develop",
    name: "develop",
    title: "Develop",
    description: "Implementation",
    icon: "🛠️",
    source: "builtin",
    keywords: [],
    has_brief: true,
    localizations: {},
    artifacts: [],
    ...overrides,
  };
}

const SCENES = [
  sceneInfo(),
  sceneInfo({
    reference: "builtin:review",
    name: "review",
    title: "Review",
    icon: "🔍",
  }),
];

async function renderModal(props = {}) {
  const mounted = mount(
    <I18nProvider>
      <IssuesModal
        repoChecker={async () => ({ is_repo: true })}
        cwd="."
        scenes={SCENES}
        onInsert={() => {}}
        onDelegate={() => {}}
        onClose={() => {}}
        {...props}
      />
    </I18nProvider>
  );
  mountedRoots.push(mounted);
  // The issue list resolves asynchronously after mount.
  await flush();
  await flush();
  return mounted;
}

/**
 * Bun module mocks leak across test files: the Canvas suite replaces `useT` with an identity
 * translator for the rest of the run, so labels may render as raw `issueDeleg.*` keys here.
 * Look controls up under both spellings instead of depending on suite ordering.
 */
function labeledButton(translated, key) {
  return (
    maybeButton(dom.document.body, translated) ?? button(dom.document.body, key)
  );
}

/** Radix menus open on pointerdown (mouse) — the harness click alone is not enough. */
async function openDelegateMenu() {
  const trigger = labeledButton("Delegate…", "issueDeleg.delegate");
  await reactAct(async () => {
    trigger.dispatchEvent(
      new dom.window.PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      })
    );
    trigger.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
    );
  });
  await flush();
}

function menuItems() {
  return [
    ...dom.document.body.querySelectorAll('[data-slot="dropdown-menu-item"]'),
  ];
}

describe("IssuesModal delegation", () => {
  test("shows a per-row Delegate menu listing every scene", async () => {
    await renderModal();
    // The existing add-to-prompt flow stays intact next to the new control.
    expect(button(dom.document.body, "Add to prompt")).toBeTruthy();
    await openDelegateMenu();
    const labels = menuItems().map((item) => item.textContent?.trim());
    expect(labels).toContain("Develop");
    expect(labels).toContain("Review");
    expect(labels.join(" ")).not.toMatch(/[🛠🔍]/u);
  });

  test("picking a scene reports the issue and that scene's reference", async () => {
    const delegated = [];
    await renderModal({
      onDelegate: (issue, sceneReference) =>
        delegated.push([issue, sceneReference]),
    });
    await openDelegateMenu();
    const review = menuItems().find((item) =>
      item.textContent?.includes("Review")
    );
    expect(review).toBeTruthy();
    await reactAct(async () => {
      review.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
      );
    });
    expect(delegated).toEqual([[ISSUE, "builtin:review"]]);
  });

  test("an empty scene library renders one disabled row instead of a dead menu", async () => {
    await renderModal({ scenes: [] });
    await openDelegateMenu();
    const items = menuItems();
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toMatch(
      /No scenes available|issueDeleg\.noScenes/
    );
    expect(items[0].dataset.disabled).not.toBeNull();
  });
});
