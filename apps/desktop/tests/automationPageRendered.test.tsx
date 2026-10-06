// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

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
const { AutomationsPage, defaultAutomationActions } =
  await import("../src/automation/AutomationsPage");
const { I18nProvider } = await import("../src/i18n");
const { ToastProvider } = await import("../src/ui/toast");
const appSource = readFileSync(
  new URL("../src/App.tsx", import.meta.url),
  "utf-8"
);
const automationSource = readFileSync(
  new URL("../src/automation/AutomationsPage.tsx", import.meta.url),
  "utf-8"
);

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

describe("AutomationsPage layout", () => {
  test("renders the sidebar recovery action supplied by the persistent shell", async () => {
    activateDom();
    let addedProjects = 0;
    const view = mount(
      <I18nProvider>
        <ToastProvider>
          <AutomationsPage
            projects={[]}
            providers={[]}
            defaultProject="."
            defaultProvider="codex"
            onAddProject={() => {
              addedProjects += 1;
            }}
            onOpenSession={() => {}}
            headerLeadingAction={<button aria-label="Expand the sidebar" />}
          />
        </ToastProvider>
      </I18nProvider>
    );

    await flush();

    expect(
      view.container.querySelector('button[aria-label="Expand the sidebar"]')
    ).not.toBeNull();
    expect(
      view.container.querySelector("[data-automation-list-header]")?.className
    ).toContain("window-controls-safe-main");
    expect(
      view.container.querySelector("[data-automation-detail-header]")?.className
    ).toContain("window-controls-safe-compact-main");
    expect(
      view.container
        .querySelector("[data-automation-detail-leading-action] button")
        ?.getAttribute("aria-label")
    ).toBe("Expand the sidebar");
    expect(view.container.textContent).toContain(
      "Add a project before creating an automation."
    );
    expect(
      view.container.querySelector("[data-automation-page] h1")?.textContent
    ).toBe("Automations");
    expect(
      view.container.querySelector("[data-automation-page] h1")?.className
    ).toContain("text-dialog");
    const emptyState = view.container.querySelector(
      "[data-automation-list-pane] [aria-live]"
    );
    expect(
      emptyState?.querySelector('[data-slot="empty-title"]')?.className
    ).toContain("font-semibold");
    expect(
      emptyState?.querySelector('[data-slot="empty-description"]')?.className
    ).toContain("text-body");
    const addProject = button(view.container, "Add project");
    expect(addProject).not.toBeNull();
    expect(addProject?.hasAttribute("disabled")).toBeFalse();
    click(addProject);
    expect(addedProjects).toBe(1);

    view.unmount();
  });

  test("uses the pull-request split list and detail layout", async () => {
    activateDom();
    const view = mount(
      <I18nProvider>
        <ToastProvider>
          <AutomationsPage
            projects={[
              {
                name: "mini-game",
                path: "/tmp/mini-game",
                last_opened_at: Date.now(),
              },
            ]}
            providers={[]}
            defaultProject="/tmp/mini-game"
            defaultProvider="codex"
            onAddProject={() => {}}
            onOpenSession={() => {}}
          />
        </ToastProvider>
      </I18nProvider>
    );

    const page = view.container.querySelector("[data-automation-page]");
    const listPane = view.container.querySelector(
      "[data-automation-list-pane]"
    );
    const detailPane = view.container.querySelector(
      "[data-automation-detail-pane]"
    );
    const listHeader = listPane?.querySelector("[data-automation-list-header]");
    const listControls = listPane?.querySelector(
      "[data-automation-list-controls]"
    );
    const search = view.container.querySelector("[data-automation-search]");
    const filters = view.container.querySelector("[data-automation-filters]");
    const detailTabs = detailPane?.querySelector('[role="tablist"]');

    expect(page?.className).toContain("flex-1");
    expect(page?.className).toContain("automations-page");
    expect(page?.dataset.compactDetail).toBe("false");
    expect(
      [...view.container.querySelectorAll("button")].some(
        (item) => item.textContent?.trim() === "Back"
      )
    ).toBeFalse();
    expect(listPane?.className).toContain("automation-list-pane");
    expect(listPane?.className).toContain("bg-sidebar");
    expect(listHeader?.className).toContain("pl-page-section");
    expect(listHeader?.className).not.toContain("window-controls-safe-main");
    expect(listHeader?.contains(filters)).toBeFalse();
    expect(listControls?.contains(filters)).toBeTrue();
    expect(listControls?.contains(search)).toBeTrue();
    expect(search?.className).toContain("ms-inline");
    expect(detailPane?.className).toContain("automation-detail-pane");
    expect(
      detailPane?.querySelector("[data-automation-detail-header]")?.className
    ).toContain("pl-4");
    expect(
      detailPane?.querySelector("[data-automation-detail-leading-action]")
    ).toBeNull();
    expect(detailPane?.textContent).toContain("Select an automation");
    expect(search?.querySelector('input[type="search"]')).not.toBeNull();
    const switcher = filters?.querySelector('[data-slot="view-switcher"]');
    expect(switcher?.getAttribute("role")).toBe("group");
    expect(switcher?.querySelectorAll("button[aria-pressed]").length).toBe(3);
    expect(
      switcher?.querySelectorAll('button[aria-pressed="true"]').length
    ).toBe(1);
    expect(detailTabs?.querySelectorAll('[role="tab"]').length).toBe(2);
    expect(
      detailTabs?.querySelectorAll('[role="tab"][aria-disabled="true"]').length
    ).toBe(2);
    expect(
      view.container.querySelector("[data-automation-task-center]")
    ).toBeNull();
    expect(
      view.container.querySelector("[data-automation-inline-detail]")
    ).toBeNull();

    await flush();
    view.unmount();
  });

  test("is mounted inside the persistent session shell instead of replacing the whole window", () => {
    expect(appSource).not.toContain(") : showAutomations ? (");
    expect(appSource).toMatch(/\{showAutomations\s*&&/u);
    expect(appSource).toMatch(
      /showTaskBoard\s*\|\|\s*showPluginManager\s*\|\|\s*showAutomations/u
    );
    const automationsCall =
      /<AutomationsPage\b[\s\S]*?\n\s*\/>/u.exec(appSource)?.[0] ?? "";
    const taskBoardCall =
      /<TaskBoardPage\b[\s\S]*?\n\s*\/>/u.exec(appSource)?.[0] ?? "";
    expect(automationsCall).toContain("onAddProject=");
  });

  test("maps active and paused states to the standard status tones", () => {
    expect(automationSource).toContain(
      '<StatusBadge tone={automation.enabled ? "success" : "neutral"}>'
    );
  });

  test("defaults its actions to the desktop bridge and keeps the replay control", () => {
    expect(automationSource).toContain("actions = defaultAutomationActions");
    expect(automationSource).toContain("subscribeToAlerts: onAutomationAlert");
    expect(automationSource).toContain("rerun: rerunAutomation");
    expect(automationSource).toContain('t("automations.rerun")');
    expect(automationSource).toContain("<RotateCcw");
  });

  test("renders a failure alert toast with a rerun action", async () => {
    activateDom();
    let deliver: ((alert: Record<string, unknown>) => void) | null = null;
    const view = mount(
      <I18nProvider>
        <ToastProvider>
          <AutomationsPage
            projects={[
              {
                name: "mini-game",
                path: "/tmp/mini-game",
                last_opened_at: Date.now(),
              },
            ]}
            providers={[]}
            defaultProject="/tmp/mini-game"
            defaultProvider="codex"
            onAddProject={() => {}}
            onOpenSession={() => {}}
            actions={{
              ...defaultAutomationActions,
              subscribeToAlerts: async (cb) => {
                deliver = cb as (alert: Record<string, unknown>) => void;
                return () => {};
              },
            }}
          />
        </ToastProvider>
      </I18nProvider>
    );
    await flush();

    expect(deliver).not.toBeNull();
    await reactAct(async () => {
      deliver?.({
        automation_id: "a1",
        run_id: "r1",
        automation_name: "Nightly triage",
        status: "failed",
        error: "provider exited 1",
      });
    });
    await flush();

    const alert = view.container.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("Automation failed: Nightly triage");
    const rerun = [...(alert?.querySelectorAll("button") ?? [])].find(
      (item) => item.textContent?.trim() === "Run again"
    );
    expect(rerun).not.toBeUndefined();

    view.unmount();
  });

  test("keeps create and edit work in the detail pane instead of opening a dialog", async () => {
    activateDom();
    const view = mount(
      <I18nProvider>
        <ToastProvider>
          <AutomationsPage
            projects={[
              {
                name: "mini-game",
                path: "/tmp/mini-game",
                last_opened_at: Date.now(),
              },
            ]}
            providers={[
              { id: "codex", display_name: "Codex", available: true },
            ]}
            defaultProject="/tmp/mini-game"
            defaultProvider="codex"
            onAddProject={() => {}}
            onOpenSession={() => {}}
          />
        </ToastProvider>
      </I18nProvider>
    );

    click(button(view.container, "New automation"));
    await flush();

    expect(
      view.container.querySelector("[data-automation-editor]")
    ).not.toBeNull();
    expect(view.container.querySelector('[role="dialog"]')).toBeNull();
    expect(
      view.container.querySelector("[data-automation-search]")
    ).not.toBeNull();
    expect(
      view.container.querySelector(
        "[data-automation-detail-pane] [data-automation-editor]"
      )
    ).not.toBeNull();
    expect(
      view.container.querySelector("[data-automation-page]")?.dataset
        .compactDetail
    ).toBe("true");

    click(button(view.container, "Back to automations"));
    await flush();

    expect(view.container.querySelector("[data-automation-editor]")).toBeNull();
    expect(
      view.container.querySelector("[data-automation-page]")?.dataset
        .compactDetail
    ).toBe("false");

    await flush();
    view.unmount();
  });
});
