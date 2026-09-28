// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

import { act as reactAct } from "react";

import {
  activateDom,
  button,
  click,
  dom,
  flush,
  mount,
  restoreDom,
} from "./domTestHarness";

activateDom();
const { CheckoutBar } = await import("../src/session/Composer");
const { I18nProvider } = await import("../src/i18n");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

function config(overrides = {}) {
  return {
    providers: [],
    providersStatus: "ready",
    provider: "codex",
    onProvider: () => {},
    onProviderModel: () => {},
    onReloadProviders: () => {},
    mode: "ask",
    sandbox: "workspace_write",
    modeChangeDisabled: false,
    onSessionMode: () => {},
    worktreeBase: null,
    activeWorktreeBaseline: null,
    activeWorktreeUnknown: false,
    worktreeOptions: [
      {
        kind: "current",
        resolved: {
          kind: "current",
          ref: "refs/heads/main",
          sha: "3befbb7dbec99119065c78a818c2b04f6c90ea0d",
          display: "main @ 3befbb7d",
        },
        unavailable_reason: null,
      },
      {
        kind: "origin_default",
        resolved: {
          kind: "origin_default",
          ref: "refs/remotes/origin/main",
          sha: "3befbb7dbec99119065c78a818c2b04f6c90ea0d",
          display: "origin/main @ 3befbb7d",
        },
        unavailable_reason: null,
      },
    ],
    worktreeOptionsLoading: false,
    onWorktreeBase: () => {},
    planMode: false,
    onPlan: () => {},
    memoryEnabled: true,
    memoryRead: "inherit",
    memoryWrite: "inherit",
    onMemoryPolicy: () => {},
    hasSession: false,
    scenesEnabled: false,
    scenes: [],
    activeScene: null,
    autoScene: false,
    onAutoScene: () => {},
    onScene: () => {},
    onManageScenes: () => {},
    sceneCustomized: false,
    scenePendingFields: [],
    onRestartInScene: () => {},
    ...overrides,
  };
}

function checkout(overrides = {}) {
  return {
    project: "codeTwo",
    branch: "main",
    dirty: 3,
    onOpen: () => {},
    ...overrides,
  };
}

async function openPicker(container) {
  const trigger = container.querySelector(
    '[aria-label="Checkout: Project checkout"]'
  );
  await reactAct(async () => {
    trigger?.dispatchEvent(
      new dom.window.PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      })
    );
    trigger?.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
    );
  });
  await flush();
  return dom.document.body.querySelector('[data-slot="popover-content"]');
}

describe("CheckoutBar", () => {
  test("separates checkout choice from source control and shows exact local baselines", async () => {
    let selected = null;
    let sourceControlOpens = 0;
    const rendered = mount(
      <I18nProvider>
        <CheckoutBar
          config={config({
            onWorktreeBase: (value) => {
              selected = value;
            },
          })}
          checkout={checkout({
            onOpen: () => {
              sourceControlOpens += 1;
            },
          })}
        />
      </I18nProvider>
    );

    try {
      const checkoutBar = rendered.container.querySelector(
        "[data-checkout-bar]"
      );
      expect(checkoutBar?.className).toContain("mx-2");
      expect(checkoutBar?.className).toContain("mb-2");
      expect(checkoutBar?.className).not.toContain("-mt-");

      const sourceControl = button(
        rendered.container,
        "Open source control for main"
      );
      click(sourceControl);
      expect(sourceControlOpens).toBe(1);

      const popup = await openPicker(rendered.container);
      expect(popup?.dataset.side).toBe("bottom");
      expect(popup?.textContent).toContain("codeTwo");
      expect(popup?.textContent).toContain("main");
      expect(popup?.textContent).toContain("origin/main");
      expect(popup?.textContent).toContain("current");
      expect(popup?.textContent).toContain("worktree");
      expect(
        button(popup, "codeTwo, Project checkout").getAttribute("aria-pressed")
      ).toBe("true");
      const currentRef = button(popup, "main, From the current ref");
      expect(currentRef.getAttribute("title")).toBeNull();
      expect(currentRef.textContent).toContain("main @ 3befbb7d");

      click(button(popup, "main, From the current ref"));
      expect(selected).toBe("current");
    } finally {
      rendered.unmount();
      await flush();
    }
  });

  test("keeps the project checkout usable when isolated baselines are unavailable", async () => {
    const rendered = mount(
      <I18nProvider>
        <CheckoutBar
          config={config({
            worktreeOptions: [
              {
                kind: "current",
                resolved: null,
                unavailable_reason: "HEAD is unavailable",
              },
              {
                kind: "origin_default",
                resolved: null,
                unavailable_reason: "origin/HEAD is missing",
              },
            ],
          })}
          checkout={checkout({ project: ".", branch: null, dirty: 0 })}
        />
      </I18nProvider>
    );

    try {
      const popup = await openPicker(rendered.container);
      expect(
        button(popup, "No project selected, Project checkout").disabled
      ).toBe(false);
      expect(button(popup, "From the current ref").disabled).toBe(true);
      expect(button(popup, "From the origin default ref").disabled).toBe(true);
      expect(rendered.container.textContent).not.toContain("main");
      expect(popup?.textContent).toContain("No project");
    } finally {
      rendered.unmount();
      await flush();
    }
  });

  test("shows an active session checkout as fixed without dimming its selected state", async () => {
    const rendered = mount(
      <I18nProvider>
        <CheckoutBar
          config={config({
            hasSession: true,
            activeWorktreeBaseline: {
              kind: "current",
              ref: "refs/heads/main",
              sha: "3befbb7dbec99119065c78a818c2b04f6c90ea0d",
              display: "main @ 3befbb7d",
            },
          })}
          checkout={checkout({ branch: "codetwo/session-123", dirty: 0 })}
        />
      </I18nProvider>
    );

    try {
      const trigger = rendered.container.querySelector(
        '[aria-label="Checkout: Session worktree"]'
      );
      await reactAct(async () => {
        trigger?.dispatchEvent(
          new dom.window.PointerEvent("pointerdown", {
            bubbles: true,
            cancelable: true,
            button: 0,
            pointerId: 1,
          })
        );
        trigger?.dispatchEvent(
          new dom.window.MouseEvent("click", {
            bubbles: true,
            cancelable: true,
          })
        );
      });
      await flush();

      const popup = dom.document.body.querySelector(
        '[data-slot="popover-content"]'
      );
      const selected = button(popup, "main, Session worktree");
      expect(selected.getAttribute("aria-pressed")).toBe("true");
      expect(selected.className).toContain(
        "disabled:data-[selected=true]:opacity-100"
      );
      expect(popup?.textContent).toContain("main");
      expect(selected.getAttribute("title")).toBeNull();
      expect(selected.textContent).toContain("main @ 3befbb7d");
      expect(popup?.textContent).toContain("exact local ref and commit");
    } finally {
      rendered.unmount();
      await flush();
    }
  });
});
