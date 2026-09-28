import { afterEach, describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

// @ts-nocheck
import { act as reactAct } from "react";

import {
  activateDom,
  dom,
  flush,
  mount,
  restoreDom,
  waitFor,
} from "./domTestHarness";

activateDom();
const { I18nProvider } = await import("../src/i18n");
const { EnvironmentPopover } =
  await import("../src/environment/EnvironmentPopover");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

function renderEnvironment(onRefresh = () => {}, preview = null, props = {}) {
  return mount(
    <I18nProvider>
      <EnvironmentPopover
        project="mini-game"
        projectPath="/tmp/mini-game"
        projects={[]}
        git={null}
        diffStat={{ added: 0, deleted: 0 }}
        onRefresh={onRefresh}
        onSelectProject={() => {}}
        onAddProject={() => {}}
        onOpenSourceControl={() => {}}
        onOpenSettings={() => {}}
        preview={preview}
        {...props}
      />
    </I18nProvider>
  );
}

describe("EnvironmentPopover layout", () => {
  test("remains mounted in the session header", () => {
    const appSource = readFileSync(
      new URL("../src/App.tsx", import.meta.url),
      "utf-8"
    );
    const environmentSource = readFileSync(
      new URL("../src/environment/EnvironmentPopover.tsx", import.meta.url),
      "utf-8"
    );

    expect(appSource).toContain('from "./environment/EnvironmentPopover"');
    expect(appSource).toContain("<EnvironmentPopover");
    expect(environmentSource).toMatch(
      /<PopoverContent\s+align="start"\s+sideOffset=\{16\}/u
    );
    expect(environmentSource).not.toContain("alignOffset={-36}");
  });

  test("stays out of the reading surface until requested and dismisses on outside press", async () => {
    activateDom();
    let refreshes = 0;
    const outside = dom.document.createElement("button");
    outside.textContent = "Reading surface";
    dom.document.body.append(outside);
    const view = renderEnvironment(() => {
      refreshes += 1;
    });

    expect(
      dom.document.body.querySelector('[data-slot="popover-content"]')
    ).toBeNull();

    const trigger = view.container.querySelector(
      '[aria-label="Project environment"]'
    );
    expect(trigger).toBeTruthy();
    expect(trigger?.dataset.variant).toBe("ghost");
    expect(trigger?.classList.contains("text-foreground")).toBe(true);
    expect(
      trigger?.querySelector(".session-header-context-label")?.textContent
    ).toBe("Environment");
    expect(
      trigger?.querySelector(".session-header-context-icon")
    ).not.toBeNull();
    expect(trigger?.classList.contains("hover:text-foreground")).toBe(true);
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

    const content = dom.document.body.querySelector(
      '[data-slot="popover-content"]'
    );
    expect(content).toBeTruthy();
    expect(trigger?.dataset.variant).toBe("ghost");
    expect(trigger?.classList.contains("bg-fill-rest")).toBe(true);
    expect(trigger?.classList.contains("text-primary")).toBe(false);
    expect(content?.className).toContain("max-h-(--available-height)");
    expect(content?.className).toContain("overflow-y-auto");
    expect(content?.textContent).toContain("Changes");
    expect(content?.textContent).toContain("Local");
    expect(content?.textContent).toContain("Commit or push");
    expect(content?.querySelector("[data-task-plan-panel]")).toBeNull();
    expect(content?.textContent).not.toContain("Current tasks");
    expect(content?.textContent).not.toContain("Checkpoint now");
    expect(content?.textContent).not.toContain("Copy path");
    expect(content?.textContent).not.toContain("Tools");
    expect(content?.textContent).not.toContain("Issues");
    expect(content?.textContent).not.toContain("Usage");
    expect(content?.textContent).not.toContain("Plugin Hub");
    expect(content?.querySelector('[aria-label="Settings"]')).toBeTruthy();
    expect(refreshes).toBe(1);

    await reactAct(async () => {
      outside.dispatchEvent(
        new dom.window.PointerEvent("pointerdown", {
          bubbles: true,
          cancelable: true,
          button: 0,
          pointerId: 2,
        })
      );
      outside.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
      );
    });

    await waitFor(() => {
      expect(
        dom.document.body.querySelector('[data-slot="popover-content"]')
      ).toBeNull();
    });
    view.unmount();
  });

  test("hangs the current Browser Use screenshot below the environment rows", async () => {
    activateDom();
    const view = renderEnvironment(() => {}, {
      kind: "browser",
      title: "Open example.com",
      artifact: {
        id: "browser-shot-2",
        mime_type: "image/png",
        bytes: 1024,
        width: 1280,
        height: 720,
        display_name: "example.com.png",
      },
    });

    const trigger = view.container.querySelector(
      '[aria-label="Project environment"]'
    );
    await reactAct(async () => {
      trigger?.dispatchEvent(
        new dom.window.PointerEvent("pointerdown", {
          bubbles: true,
          cancelable: true,
          button: 0,
          pointerId: 3,
        })
      );
      trigger?.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
      );
    });
    await flush();

    const preview = dom.document.body.querySelector(
      '[data-tool-preview="browser"]'
    );
    expect(preview).toBeTruthy();
    expect(preview?.dataset.artifactId).toBe("browser-shot-2");
    expect(preview?.textContent).toContain("Browser Use");
    expect(preview?.textContent).toContain("Open example.com");
    view.unmount();
  });
});
