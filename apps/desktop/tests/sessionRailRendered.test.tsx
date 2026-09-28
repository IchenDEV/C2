// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

import {
  activateDom,
  click,
  dom,
  mount,
  restoreDom,
  waitFor,
} from "./domTestHarness";

activateDom();
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
globalThis.IntersectionObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
const { I18nProvider } = await import("../src/i18n");
const { SessionRail } = await import("../src/sidebar/SessionRail");
const { SIDEBAR_SECTIONS_STORAGE_KEY } =
  await import("../src/sidebar/sidebarSections");
const { SIDEBAR_PROJECTS_STORAGE_KEY } =
  await import("../src/sidebar/sidebarProjects");
const { ToastProvider } = await import("../src/ui/toast");

let restoreCanvasContext: (() => void) | null = null;

function disableCanvasDrawing(): void {
  // Other canvas suites install purpose-specific partial contexts. The activity-orb contract tests
  // only need the canvas element and state metadata, so keep this suite on the supported no-context path.
  const { getContext } = dom.HTMLCanvasElement.prototype;
  dom.HTMLCanvasElement.prototype.getContext = () => null;
  restoreCanvasContext = () => {
    dom.HTMLCanvasElement.prototype.getContext = getContext;
  };
}

afterEach(() => {
  restoreCanvasContext?.();
  restoreCanvasContext = null;
  dom.document.body.replaceChildren();
  dom.window.localStorage.clear();
  delete dom.window.navigator.clipboard;
  restoreDom();
});

function session(id: string, title: string) {
  return {
    id,
    title,
    title_origin: "manual",
    pinned: false,
    provider: "codex",
    model: null,
    cwd: "/tmp/repo",
    worktree_path: null,
    project_path: "/tmp/repo",
    permission_mode: "ask",
    sandbox_policy: "workspace_write",
    acp_session_id: null,
    memory_read: "inherit",
    memory_write: "inherit",
    created_at: Date.now(),
    activity: { revision: 1, state: { kind: "idle" } },
  };
}

function renderRail(overrides = {}) {
  return mount(
    <I18nProvider>
      <ToastProvider>
        <SessionRail
          projects={[
            { name: "repo", path: "/tmp/repo", last_opened_at: Date.now() },
          ]}
          sessions={[
            session("punctuation", "Punctuation"),
            session("meaningful", "Meaningful"),
          ]}
          archivedSessions={[]}
          previews={{ punctuation: " · ", meaningful: "A useful preview" }}
          activeSession="punctuation"
          runningSessions={new Set()}
          onSelect={() => {}}
          onNew={() => {}}
          quickChatOpen={false}
          onToggleQuickChat={() => {}}
          onRename={() => {}}
          onPin={() => {}}
          onArchive={() => {}}
          onAddProject={() => {}}
          displayProvider={() => "Codex"}
          onOpenMarket={() => {}}
          onOpenAutomations={() => {}}
          deviceConnectionsAvailable={false}
          deviceConnectionsOpen={false}
          onOpenDeviceConnections={() => {}}
          newHint="⌘N"
          onOpenTasks={() => {}}
          onOpenSettings={() => {}}
          collapsed={false}
          overlay={false}
          width={320}
          onWidth={() => {}}
          taskBoardOpen={false}
          onOpenTaskBoard={() => {}}
          pullRequestsOpen={false}
          onOpenPullRequests={() => {}}
          automationsOpen={false}
          pluginManagerOpen={false}
          quickQuota={{
            provider: "codex",
            remainingPercent: 42,
            windowMinutes: 10_080,
            resetsAt: null,
          }}
          quickQuotaLoading={false}
          quickQuotaProviderName="Codex"
          onOpenUsage={() => {}}
          {...overrides}
        />
      </ToastProvider>
    </I18nProvider>
  );
}

describe("SessionRail row layout", () => {
  test("renders external resource Sections in the same flat scroll flow", () => {
    activateDom();
    const view = renderRail({
      resourceSections: <section>Feishu resources</section>,
    });

    const sessionScroll = view.container.querySelector(
      "[data-rail-session-scroll]"
    );
    const resources = [
      ...(sessionScroll?.querySelectorAll("section") ?? []),
    ].find((section) => section.textContent?.includes("Feishu resources"));
    expect(resources).toBeTruthy();
    expect(sessionScroll?.contains(resources ?? null)).toBe(true);
    expect(
      view.container.querySelector("[data-rail-project-switcher]")
    ).toBeNull();
    expect(view.container.textContent).toContain("Punctuation");
    expect(
      view.container.querySelector(
        '[data-rail-utilities] [aria-label="Settings"]'
      )
    ).toBeTruthy();

    view.unmount();
  });

  test("keeps pinned Tasks ahead of global recency without inventing a Highlight Section", () => {
    activateDom();
    const pinned = {
      ...session("pinned", "Pinned chat"),
      pinned: true,
      created_at: 50,
      last_active_at: 50,
    };
    const revived = {
      ...session("revived", "Revived chat"),
      created_at: 100,
      last_active_at: 300,
    };
    const newer = {
      ...session("newer", "Newer chat"),
      created_at: 200,
      last_active_at: 200,
    };
    const view = renderRail({
      sessions: [newer, pinned, revived],
      activeSession: null,
      previews: {},
    });

    const text = view.container.textContent ?? "";
    expect(text.indexOf("Pinned chat")).toBeLessThan(
      text.indexOf("Revived chat")
    );
    expect(text.indexOf("Revived chat")).toBeLessThan(
      text.indexOf("Newer chat")
    );
    expect(
      view.container.querySelector(
        '[data-task-section-toggle="system:highlight"]'
      )
    ).toBeNull();

    view.unmount();
  });

  test("shows every Project as an ordered folder with its own Tasks", () => {
    activateDom();
    dom.window.localStorage.setItem("rail.archivedOpen", "1");
    const local = { ...session("local", "Local task"), last_active_at: 200 };
    const other = {
      ...session("other", "Other project task"),
      cwd: "/tmp/other",
      project_path: "/tmp/other",
      last_active_at: 300,
    };
    const archivedOther = {
      ...session("archived-other", "Archived other task"),
      cwd: "/tmp/other",
      project_path: "/tmp/other",
      created_at: 400,
    };
    const view = renderRail({
      projects: [
        { name: "repo", path: "/tmp/repo", last_opened_at: 10 },
        { name: "other", path: "/tmp/other", last_opened_at: 20 },
      ],
      sessions: [local, other],
      archivedSessions: [archivedOther],
      activeSession: null,
      previews: {},
    });

    const projectPaths = [
      ...view.container.querySelectorAll("[data-project-group]"),
    ].map((group) => group.dataset.projectGroup);
    expect(projectPaths).toEqual(["/tmp/other", "/tmp/repo"]);
    expect(
      view.container.querySelector(
        '[data-project-content="/tmp/other"] [data-session-id="other"]'
      )
    ).toBeTruthy();
    expect(
      view.container.querySelector(
        '[data-project-content="/tmp/repo"] [data-session-id="local"]'
      )
    ).toBeTruthy();
    expect(
      view.container.querySelector(
        '[data-rail-archive-list] [data-session-id="archived-other"]'
      )
    ).toBeTruthy();
    expect(
      view.container.querySelectorAll("[data-project-group]")
    ).toHaveLength(2);

    view.unmount();
  });

  test("wraps root Projects in one expanded default Project group", async () => {
    activateDom();
    const view = renderRail({ activeSession: null });
    const group = view.container.querySelector("[data-default-project-group]");
    const toggle = group?.querySelector("[data-default-project-toggle]");
    const content = group?.querySelector("[data-default-project-content]");

    expect(toggle?.textContent).toContain("All projects");
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(content?.querySelectorAll("[data-project-group]")).toHaveLength(1);
    expect(
      content?.querySelector('[data-project-group="/tmp/repo"]')
    ).toBeTruthy();

    click(toggle);
    await waitFor(() => {
      expect(toggle?.getAttribute("aria-expanded")).toBe("false");
      expect(group?.querySelector("[data-default-project-content]")).toBeNull();
    });

    view.unmount();
  });

  test("keeps explicit Sections separate from the ordinary unsectioned feed", async () => {
    activateDom();
    disableCanvasDrawing();
    dom.window.localStorage.setItem(
      SIDEBAR_SECTIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        sections: [{ id: "work", name: "Work", collapsed: false }],
        assignments: { running: "work" },
      })
    );
    const pinned = { ...session("pinned", "Pinned task"), pinned: true };
    const running = {
      ...session("running", "Running task"),
      last_active_at: 300,
    };
    const idle = { ...session("idle", "Flat task"), last_active_at: 200 };
    const view = renderRail({
      sessions: [idle, pinned, running],
      runningSessions: new Set(["running"]),
      activeSession: null,
      previews: {},
    });

    const work = view.container.querySelector(
      '[data-task-section-content="work"]'
    );
    const flat = view.container.querySelector(
      '[data-project-content="/tmp/repo"]'
    );
    expect(
      view.container.querySelector(
        '[data-task-section-content="system:highlight"]'
      )
    ).toBeNull();
    expect(work?.textContent).toContain("Running task");
    expect(flat?.textContent).toContain("Pinned task");
    expect(flat?.textContent).toContain("Flat task");

    const ids = [...view.container.querySelectorAll("[data-session-id]")].map(
      (row) => row.dataset.sessionId
    );
    expect(ids).toHaveLength(3);
    expect(new Set(ids).size).toBe(3);

    const workToggle = view.container.querySelector(
      '[data-task-section-toggle="work"]'
    );
    expect(workToggle?.children[0]?.textContent).toBe("Work");
    expect(workToggle?.children[1]?.tagName).toBe("svg");
    expect(workToggle?.className).toContain("px-surface-inset");
    expect(workToggle?.parentElement?.className).toContain("pr-2");
    expect(workToggle?.parentElement?.className).not.toContain("px-2");

    const runningRow = view.container.querySelector(
      '[data-session-id="running"]'
    );
    runningRow?.dispatchEvent(
      new dom.window.MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
      })
    );
    await waitFor(() => {
      expect(
        dom.document.body.querySelector(
          '[data-slot="context-menu-sub-trigger"]'
        )?.textContent
      ).toContain("Section");
    });

    view.unmount();
  });

  test("keeps Project rows collapsible without a trailing disclosure icon", async () => {
    activateDom();
    const view = renderRail();
    const toggle = view.container.querySelector(
      '[data-project-toggle="/tmp/repo"]'
    );

    expect(toggle?.querySelectorAll("svg")).toHaveLength(1);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(toggle?.className).toContain("hover:bg-transparent");
    expect(toggle?.className).toContain("dark:hover:bg-transparent");
    expect(toggle?.className).not.toContain("hover:bg-accent");
    const projectContent = view.container.querySelector(
      '[data-project-content="/tmp/repo"]'
    );
    expect(projectContent).toBeTruthy();
    expect(projectContent?.className).toContain("ml-0");
    expect(projectContent?.className).not.toContain("ml-2");
    expect(projectContent?.className).not.toContain("ml-6");
    const groupedRow = projectContent?.querySelector(
      '[data-session-id="meaningful"]'
    );
    expect(groupedRow?.className).not.toContain("ml-6");
    expect(toggle?.getAttribute("data-size")).toBe("row");
    expect(toggle?.className).toContain("min-h-navigation-row");
    expect(toggle?.className).toContain("px-module-inset");
    expect(groupedRow?.className).toContain("px-2");
    // One content column: the project mark, the nav marks and the session text all sit on the
    // rail's single 16px edge, so the session content adds no padding of its own.
    expect(
      groupedRow?.querySelector("[data-session-content]")?.className
    ).not.toContain("pl-");

    click(toggle);
    await waitFor(() => {
      expect(toggle?.getAttribute("aria-expanded")).toBe("false");
      expect(
        view.container.querySelector('[data-project-content="/tmp/repo"]')
      ).toBeNull();
    });

    view.unmount();
  });

  test("aligns an empty Project placeholder beneath the Project label", () => {
    activateDom();
    const view = renderRail({
      sessions: [],
      activeSession: null,
      previews: {},
    });
    const projectContent = view.container.querySelector(
      '[data-project-content="/tmp/repo"]'
    );
    const placeholder = projectContent?.querySelector("p");

    expect(placeholder?.textContent).toBe("No chats");
    expect(placeholder?.className).toContain("pl-8");
    expect(placeholder?.className).toContain("pr-2");
    expect(placeholder?.className).not.toContain("px-2");

    view.unmount();
  });

  test("exposes dnd-kit keyboard guidance without native HTML5 drag attributes", async () => {
    activateDom();
    const first = { ...session("first", "First task"), last_active_at: 300 };
    const second = { ...session("second", "Second task"), last_active_at: 200 };
    const view = renderRail({
      projects: [
        { name: "repo", path: "/tmp/repo", last_opened_at: 1 },
        { name: "other", path: "/tmp/other", last_opened_at: 2 },
      ],
      sessions: [first, second],
      activeSession: null,
      previews: {},
    });

    const secondHandle = view.container.querySelector<HTMLElement>(
      '[data-session-id="second"] [data-session-drag-handle]'
    );
    if (!secondHandle) throw new Error("missing Task drag fixture");

    const repoHandle = view.container.querySelector<HTMLElement>(
      '[data-project-drag-handle="/tmp/repo"]'
    );
    if (!repoHandle) throw new Error("missing Project drag fixture");

    await waitFor(() => {
      expect(repoHandle.getAttribute("aria-roledescription")).toBe("draggable");
      expect(secondHandle.getAttribute("aria-roledescription")).toBe(
        "draggable"
      );
    });
    expect(repoHandle.getAttribute("draggable")).toBeNull();
    expect(secondHandle.getAttribute("draggable")).toBeNull();
    expect(view.container.querySelector("[draggable=true]")).toBeNull();
    const description = dom.document.getElementById(
      repoHandle.getAttribute("aria-describedby") ?? ""
    );
    expect(description?.textContent).toContain("use the arrow keys");
    expect(
      view.container
        .querySelector(
          '[data-session-id="second"] [data-session-drag-handle] svg'
        )
        ?.getAttribute("class")
    ).toContain("pointer-events-none");

    view.unmount();
  });

  test("project menu removes registered projects and hides removal for synthesized ones", async () => {
    activateDom();
    const removed: string[] = [];
    const view = renderRail({
      sessions: [
        session("punctuation", "Punctuation"),
        {
          ...session("ghosted", "Ghosted"),
          id: "ghosted",
          cwd: "/tmp/ghost",
          project_path: "/tmp/ghost",
        },
      ],
      previews: { punctuation: " · ", ghosted: "" },
      onRemoveProject: (path: string) => removed.push(path),
    });

    click(view.container.querySelector('[data-project-actions="/tmp/repo"]'));
    await waitFor(() => {
      expect(dom.document.body.textContent).toContain("Remove from list");
    });
    const removeItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.includes("Remove from list"));
    expect(removeItem).toBeTruthy();
    click(removeItem);
    expect(removed).toEqual(["/tmp/repo"]);

    click(view.container.querySelector('[data-project-actions="/tmp/ghost"]'));
    await waitFor(() => {
      const menu = [
        ...dom.document.body.querySelectorAll('[role="menu"]'),
      ].pop();
      expect(menu?.textContent).toContain("New section");
      expect(menu?.textContent).not.toContain("Remove from list");
    });

    view.unmount();
  });

  test("keeps resize tracking native to the separator and releases it when the pointer is cancelled", () => {
    activateDom();
    const widths: number[] = [];
    const view = renderRail({ onWidth: (width: number) => widths.push(width) });
    const grip = view.container.querySelector<HTMLElement>(".rail-grip");
    let capturedPointer: number | null = null;

    expect(grip).toBeTruthy();
    if (!grip) throw new Error("missing rail resize grip");
    expect(grip.tabIndex).toBe(0);
    expect(grip.getAttribute("role")).toBe("separator");
    expect(grip.getAttribute("aria-orientation")).toBe("vertical");
    expect(grip.getAttribute("aria-valuemin")).toBe("220");
    expect(grip.getAttribute("aria-valuemax")).toBe("420");
    expect(grip.getAttribute("aria-valuenow")).toBe("320");

    grip.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key: "ArrowRight",
      })
    );
    expect(widths.at(-1)).toBe(330);

    grip.setPointerCapture = (pointerId) => {
      capturedPointer = pointerId;
    };
    grip.hasPointerCapture = (pointerId) => capturedPointer === pointerId;
    grip.releasePointerCapture = (pointerId) => {
      if (capturedPointer === pointerId) capturedPointer = null;
    };

    grip.dispatchEvent(
      new dom.window.PointerEvent("pointerdown", {
        bubbles: true,
        button: 0,
        clientX: 320,
        pointerId: 7,
      })
    );
    expect(capturedPointer).toBe(7);
    expect(dom.document.body.classList.contains("resizing-h")).toBe(true);

    grip.dispatchEvent(
      new dom.window.PointerEvent("pointermove", {
        bubbles: true,
        clientX: 360,
        pointerId: 7,
      })
    );
    expect(widths.at(-1)).toBe(360);

    grip.dispatchEvent(
      new dom.window.PointerEvent("pointercancel", {
        bubbles: true,
        pointerId: 7,
      })
    );
    expect(capturedPointer).toBeNull();
    expect(dom.document.body.classList.contains("resizing-h")).toBe(false);

    view.unmount();
  });

  test("starts the task list without a duplicate window header", () => {
    activateDom();
    const view = renderRail();
    expect(view.container.querySelector("[data-rail-header]")).toBeNull();
    expect(view.container.textContent).not.toContain("CodeTwo");
    expect(view.container.querySelector("[data-task-actions]")).not.toBeNull();
    view.unmount();
  });

  test("keeps labeled global navigation and utilities outside the collapsed task list", () => {
    activateDom();
    const opened = [];
    const view = renderRail({
      collapsed: true,
      taskBoardOpen: true,
      onOpenTasks: () => opened.push("chats"),
      onOpenPullRequests: () => opened.push("pull-requests"),
      onOpenTaskBoard: () => opened.push("tasks"),
      onOpenAutomations: () => opened.push("scheduled"),
      onOpenMarket: () => opened.push("plugins"),
      onOpenUsage: () => opened.push("usage"),
      onOpenSettings: () => opened.push("settings"),
    });
    const navigation = view.container.querySelector("[data-app-navigation]");
    const sidebar = view.container.querySelector("aside");
    expect(sidebar?.getAttribute("aria-hidden")).toBe("true");
    expect(sidebar?.hasAttribute("inert")).toBe(true);
    expect(sidebar?.contains(navigation)).toBe(false);
    const buttons = [...navigation.querySelectorAll("button")];
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Chats",
      "Pull requests",
      "Task board",
      "Scheduled tasks",
      "Plugins",
      "Settings",
      "Codex · Weekly limit · 42% left · Open Usage settings",
    ]);
    expect(
      buttons.filter((button) => button.getAttribute("aria-current") === "page")
    ).toEqual([buttons[2]]);
    for (const button of buttons) click(button);
    expect(opened).toEqual([
      "chats",
      "pull-requests",
      "tasks",
      "scheduled",
      "plugins",
      "settings",
      "usage",
    ]);
    expect(
      view.container.querySelector("[data-rail-utilities]")?.dataset.layout
    ).toBe("icon-column");
    expect(
      sidebar?.querySelector('[data-rail-feature="new-task"]')
    ).toBeTruthy();
    expect(
      navigation?.querySelector('[data-rail-feature="new-task"]')
    ).toBeNull();
    view.unmount();
  });

  test("keeps device connections in the lower-left utility group with a phone icon", () => {
    activateDom();
    const opened = [];
    const view = renderRail({
      deviceConnectionsAvailable: true,
      deviceConnectionsOpen: true,
      onOpenDeviceConnections: () => opened.push("device-connections"),
    });

    const features = view.container.querySelector("[data-rail-features]");
    const utilities = view.container.querySelector("[data-rail-utilities]");
    const button = utilities?.querySelector(
      '[data-rail-feature="device-connections"] [data-slot="rail-utility-button"]'
    );

    expect(
      features?.querySelector('[data-rail-feature="device-connections"]')
    ).toBeNull();
    expect(button?.textContent?.trim()).toBe("");
    expect(button?.getAttribute("aria-label")).toBe("Device connections");
    expect(button?.getAttribute("aria-current")).toBe("page");
    expect(button?.dataset.selected).toBe("true");
    expect(button?.className).toContain("rounded-full");
    expect(
      button?.querySelector('[data-device-connections-icon="phone"]')
    ).toBeTruthy();
    expect(
      button?.parentElement?.previousElementSibling?.dataset.railFeature
    ).toBe("usage");
    click(button);
    expect(opened).toEqual(["device-connections"]);

    view.unmount();
  });

  test("removes the device-connections row when its plugin component is unavailable", () => {
    activateDom();
    const view = renderRail({ deviceConnectionsAvailable: false });

    expect(
      view.container.querySelector('[data-rail-feature="device-connections"]')
    ).toBeNull();

    view.unmount();
  });

  test("keeps one Quick Chat action beside the tracked Task action", () => {
    activateDom();
    const created = [];
    const view = renderRail({
      onNew: () => created.push("task"),
      onToggleQuickChat: () => created.push("quick-chat"),
    });

    const control = view.container.querySelector(
      '[data-rail-feature="new-task"]'
    );
    const primary = control?.querySelector("button:first-of-type");
    const quickChat = control?.querySelector("button[data-rail-quick-chat]");

    expect(control?.getAttribute("role")).toBe("group");
    expect(control?.className).toContain("h-control");
    expect(control?.className).toContain("hover:bg-fill-hover");
    expect(primary?.textContent?.trim()).toBe("New task");
    expect(control?.querySelectorAll("button")).toHaveLength(2);
    expect(control?.querySelector("[data-rail-quick-session]")).toBeNull();
    expect(control?.querySelector("[data-rail-side-chat]")).toBeNull();
    expect(quickChat?.getAttribute("aria-label")).toBe("Toggle Quick Chat");
    expect(quickChat?.getAttribute("aria-pressed")).toBe("false");
    expect(quickChat?.className).toContain("mr-2");
    expect(quickChat?.className).not.toContain("mr-1");

    click(primary);
    click(quickChat);
    expect(created).toEqual(["task", "quick-chat"]);
    expect(dom.document.body.querySelector('[role="menu"]')).toBeNull();

    view.unmount();
  });

  test("treats a user-created Highlight like every other editable Section", async () => {
    activateDom();
    dom.window.localStorage.setItem(
      SIDEBAR_SECTIONS_STORAGE_KEY,
      JSON.stringify({
        version: 2,
        sections: [{ id: "highlight", name: "Highlight", collapsed: false }],
        assignments: {},
        taskOrder: {},
      })
    );
    dom.window.localStorage.setItem(
      SIDEBAR_PROJECTS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        assignments: { "/tmp/repo": "highlight" },
        order: { highlight: ["/tmp/repo"] },
        collapsed: {},
      })
    );
    const view = renderRail({
      sessions: [{ ...session("highlight", "Highlighted task"), pinned: true }],
      archivedSessions: [session("archived", "Archived task")],
    });

    const recent = view.container.querySelector(
      '[data-rail-section-label="recent"]'
    );
    const project = view.container.querySelector(
      "[data-rail-project-switcher]"
    );
    const highlight = view.container.querySelector(
      '[data-task-section-toggle="highlight"]'
    );
    const archived = view.container.querySelector("[data-rail-archive-toggle]");

    expect(recent).toBeNull();
    expect(project).toBeNull();
    expect(view.container.querySelector("[data-new-task-section]")).toBeNull();

    for (const label of [highlight, archived]) {
      expect(label?.className).toContain("text-body");
      expect(label?.className).toContain("font-normal");
      expect(label?.className).toContain("text-foreground/55");
      expect(label?.className).toContain("px-surface-inset");
      expect(label?.className).not.toContain("px-4");
      expect(label?.className).not.toContain("font-medium");
      expect(label?.className).not.toContain("uppercase");
      expect(label?.className).not.toContain("tracking-");
    }
    expect(highlight?.children[0]?.textContent).toBe("Highlight");
    expect(highlight?.children[1]?.tagName).toBe("svg");
    expect(
      view.container.querySelector(
        '[data-task-section-content="highlight"] [data-project-group="/tmp/repo"]'
      )
    ).toBeTruthy();
    expect(
      view.container.querySelector('[data-task-section-actions="highlight"]')
    ).toBeTruthy();
    expect(
      view.container.querySelector(
        '[data-task-section-toggle="system:highlight"]'
      )
    ).toBeNull();
    expect(archived?.children[0]?.textContent).toBe("Archived");
    expect(archived?.children[1]?.tagName).toBe("svg");

    click(
      view.container.querySelector('[data-task-section-actions="highlight"]')
    );
    await waitFor(() => {
      const menu = dom.document.body.querySelector('[role="menu"]');
      expect(menu?.textContent).toContain("Edit section");
      expect(menu?.textContent).toContain("Archive all tasks");
      expect(menu?.textContent).toContain("Delete section");
    });

    view.unmount();
  });

  test("shows a recent AI reply and metadata beneath the title when it is useful", () => {
    activateDom();
    const view = renderRail();
    const punctuation = view.container.querySelector(
      '[data-session-id="punctuation"]'
    );
    const meaningful = view.container.querySelector(
      '[data-session-id="meaningful"]'
    );

    expect(punctuation?.querySelectorAll("[data-session-line]")).toHaveLength(
      2
    );
    expect(
      punctuation?.querySelector('[data-session-line="preview"]')
    ).toBeNull();
    expect(punctuation?.querySelector("[data-session-preview]")).toBeNull();
    expect(punctuation?.getAttribute("title")).toBeNull();

    expect(meaningful?.querySelectorAll("[data-session-line]")).toHaveLength(2);
    expect(
      meaningful?.querySelector("[data-session-preview]")?.textContent
    ).toBe("A useful preview");
    expect(
      meaningful?.querySelector("[data-session-preview]")?.className
    ).toContain("truncate");
    expect(
      meaningful
        ?.querySelector("[data-session-select]")
        ?.getAttribute("aria-describedby")
    ).toBe("session-preview-meaningful");
    expect(meaningful?.getAttribute("title")).toBe("A useful preview");
    const meaningfulSummary = meaningful?.querySelector(
      '[data-session-line="summary"]'
    );
    const meaningfulSummaryOrder = [...(meaningfulSummary?.children ?? [])].map(
      (child) => {
        if (Object.hasOwn(child.dataset, "sessionPreview")) return "preview";
        if (Object.hasOwn(child.dataset, "sessionProvider")) return "provider";
        if (Object.hasOwn(child.dataset, "sessionAge")) return "age";
        if (Object.hasOwn(child.dataset, "sessionCheckoutKind"))
          return "checkout";
        return "other";
      }
    );
    expect(meaningfulSummaryOrder).toEqual([
      "preview",
      "provider",
      "age",
      "checkout",
    ]);

    for (const row of [punctuation, meaningful]) {
      const summary = row?.querySelector('[data-session-line="summary"]');
      expect(
        view.container.querySelector('[data-project-toggle="/tmp/repo"]')
          ?.textContent
      ).toContain("repo");
      expect(row?.querySelector('[data-session-line="workspace"]')).toBeNull();
      expect(
        summary?.querySelector('[data-session-checkout-kind="checkout"]')
      ).toBeTruthy();
      expect(
        row?.querySelector('[data-session-provider="codex"] svg')
      ).toBeTruthy();
      expect(row?.querySelector("[data-session-age]")).toBeTruthy();
      expect(row?.querySelector("[data-session-status]")).toBeNull();
      expect(row?.querySelector("[data-session-actions]")?.className).toContain(
        "hidden"
      );
      expect(row?.querySelector("[data-session-actions]")?.className).toContain(
        "group-hover:flex"
      );
      expect(row?.querySelector("[data-session-actions]")?.className).toContain(
        "group-focus-within:flex"
      );
    }

    const meaningfulLines = [
      ...(meaningful?.querySelectorAll("[data-session-line]") ?? []),
    ].map((line) => line.dataset.sessionLine);
    expect(meaningfulLines).toEqual(["title", "summary"]);

    view.unmount();
  });

  test("keeps provider identity accessible and shows its compact mark", () => {
    activateDom();
    const openCodeSession = {
      ...session("opencode", "OpenCode task"),
      provider: "opencode2",
    };
    const view = renderRail({
      sessions: [openCodeSession],
      activeSession: openCodeSession.id,
      previews: {},
      displayProvider: () => "OpenCode 2 (Beta)",
    });
    const row = view.container.querySelector('[data-session-id="opencode"]');

    expect(
      row
        ?.querySelector('[data-session-provider="opencode2"]')
        ?.getAttribute("title")
    ).toBe("OpenCode 2 (Beta)");
    expect(
      row
        ?.querySelector('[data-session-provider="opencode2"] svg')
        ?.getAttribute("viewBox")
    ).toBe("96 96 320 320");
    expect(
      row?.querySelector("[data-session-select]")?.getAttribute("aria-label")
    ).toContain("OpenCode 2 (Beta)");
    expect(row?.querySelectorAll("[data-session-line]")).toHaveLength(2);

    view.unmount();
  });

  test("shows the latest AI reply, Provider mark, and relative age on the second line", () => {
    activateDom();
    const recent = {
      ...session("recent", "Recent task"),
      provider: "opencode2",
      created_at: Date.now() - 5 * 60_000,
      last_active_at: Date.now() - 5 * 60_000,
    };
    const view = renderRail({
      sessions: [recent],
      activeSession: recent.id,
      previews: { recent: "The newest AI reply" },
      displayProvider: () => "OpenCode 2 (Beta)",
    });
    const row = view.container.querySelector('[data-session-id="recent"]');
    const lines = [...(row?.querySelectorAll("[data-session-line]") ?? [])];
    const summary = row?.querySelector('[data-session-line="summary"]');

    expect(lines[0]?.dataset.sessionLine).toBe("title");
    expect(lines[1]?.dataset.sessionLine).toBe("summary");
    expect(summary?.textContent).toContain("The newest AI reply");
    expect(summary?.querySelector("svg")?.getAttribute("viewBox")).toBe(
      "96 96 320 320"
    );
    expect(summary?.querySelector("time")?.textContent).toBe("5m");

    view.unmount();
  });

  test("keeps grouped Task provenance on the summary rail instead of an empty third line", () => {
    activateDom();
    const grouped = {
      ...session("grouped", "Grouped task"),
      last_active_at: Date.now() - 60 * 60_000,
    };
    const view = renderRail({
      sessions: [grouped],
      activeSession: null,
      previews: { grouped: "A long reply that must truncate before metadata" },
    });
    const row = view.container.querySelector('[data-session-id="grouped"]');
    const summary = row?.querySelector('[data-session-line="summary"]');

    expect(summary?.querySelector("[data-session-age]")?.textContent).toBe(
      "1h"
    );
    expect(
      summary?.querySelector('[data-session-checkout-kind="checkout"]')
    ).toBeTruthy();
    expect(Boolean(row?.querySelector('[data-session-line="workspace"]'))).toBe(
      false
    );

    view.unmount();
  });

  test("uses the same two-line style for ordinary ungrouped Tasks", () => {
    activateDom();
    const ungrouped = {
      ...session("ungrouped", "Ungrouped task"),
      project_path: null,
    };
    const view = renderRail({
      projects: [],
      sessions: [ungrouped],
      activeSession: null,
      previews: { ungrouped: "A useful reply" },
    });
    const row = view.container.querySelector('[data-session-id="ungrouped"]');
    const summary = row?.querySelector('[data-session-line="summary"]');

    expect(Boolean(row?.querySelector('[data-session-line="workspace"]'))).toBe(
      false
    );
    expect(
      summary?.querySelector('[data-session-checkout-kind="checkout"]')
    ).toBeTruthy();
    expect(row?.querySelectorAll("[data-session-line]")).toHaveLength(2);
    expect(row?.textContent).not.toContain("repo");
    expect(
      row?.querySelector("[data-session-content]")?.className
    ).not.toContain("pl-");
    expect(row?.querySelector("[data-session-content]")?.className).toContain(
      "z-10"
    );

    view.unmount();
  });

  test("shows regular checkout/worktree provenance and the linked PR outcome", async () => {
    activateDom();
    const regular = session("regular", "Regular checkout");
    const isolated = {
      ...session("isolated", "Isolated worktree"),
      cwd: "/tmp/repo-worktree",
      worktree_path: "/tmp/repo-worktree",
      last_active_at: Date.now() + 1,
    };
    const loadPullRequest = async (path: string) => ({
      number: path.endsWith("worktree") ? 84 : 83,
      title: "Sidebar status",
      url: `https://github.com/example/repo/pull/${path.endsWith("worktree") ? 84 : 83}`,
      state: path.endsWith("worktree") ? "MERGED" : "OPEN",
      is_draft: false,
      head_ref: "feature",
      base_ref: "main",
      additions: 1,
      deletions: 0,
      changed_files: 1,
      body: "",
      review_decision: null,
      mergeable: "MERGEABLE",
      merge_state_status: "CLEAN",
      author: "author",
      comments_count: 0,
      reviews_count: 0,
      checks: path.endsWith("worktree")
        ? []
        : [
            {
              name: "test",
              status: "COMPLETED",
              conclusion: "FAILURE",
              details_url: null,
              workflow_name: null,
            },
          ],
      created_at: "2026-08-31T00:00:00Z",
      updated_at: "2026-08-31T00:00:00Z",
    });
    const view = renderRail({
      sessions: [regular, isolated],
      previews: {},
      activeSession: null,
      loadPullRequest,
    });

    expect(
      view.container.querySelector(
        '[data-session-id="isolated"] [data-session-checkout-kind="worktree"]'
      )?.textContent
    ).toContain("Worktree");
    expect(
      view.container.querySelector(
        '[data-session-id="regular"] [data-session-checkout-kind="checkout"]'
      )?.textContent
    ).toContain("Checkout");

    await waitFor(() => {
      const merged = view.container.querySelector(
        '[data-session-id="isolated"] [data-session-pull-request="merged"]'
      );
      const failed = view.container.querySelector(
        '[data-session-id="regular"] [data-session-pull-request="ci_failed"]'
      );
      // The state travels by colour and the shared tooltip/accessible name, not by visible words.
      expect(merged?.textContent).toContain("#84");
      expect(merged?.textContent).not.toContain("Merged");
      expect(merged?.getAttribute("title")).toContain("Merged");
      expect(merged?.getAttribute("aria-label")).toContain("Merged");
      expect(failed?.textContent).toContain("#83");
      expect(failed?.textContent).not.toContain("CI failed");
      expect(failed?.getAttribute("title")).toContain("CI failed");
    });

    // The provenance marks are inline meta, not pills: no fill and no pill padding, so their text
    // edge matches the row title's edge on the shared content container.
    for (const selector of [
      '[data-session-id="isolated"] [data-session-checkout-kind="worktree"]',
      '[data-session-id="regular"] [data-session-pull-request="ci_failed"]',
    ]) {
      const badge = view.container.querySelector(selector);
      expect(badge?.classList.contains("bg-fill-quiet")).toBe(false);
      expect(badge?.classList.contains("rounded-micro")).toBe(false);
      expect(badge?.classList.contains("px-1")).toBe(false);
    }
    const row = view.container.querySelector('[data-session-id="regular"]');
    const titleLine = row?.querySelector('[data-session-line="title"]');
    const workspaceLine = row?.querySelector('[data-session-line="workspace"]');
    expect(titleLine?.parentElement).toBe(workspaceLine?.parentElement);
    expect(titleLine?.parentElement?.hasAttribute("data-session-content")).toBe(
      true
    );

    view.unmount();
  });

  test("omits punctuation-only previews without dropping Provider and age metadata", () => {
    activateDom();
    const view = renderRail({
      sessions: [session("echo", "Paste support")],
      previews: { echo: " · " },
      activeSession: "echo",
    });
    const row = view.container.querySelector('[data-session-id="echo"]');

    expect(row?.querySelectorAll("[data-session-line]")).toHaveLength(2);
    expect(row?.querySelector('[data-session-line="preview"]')).toBeNull();
    expect(row?.querySelector("#session-preview-echo")).toBeNull();
    expect(
      row?.querySelector('[data-session-provider="codex"] svg')
    ).toBeTruthy();
    expect(row?.querySelector("[data-session-age]")).toBeTruthy();
    expect(
      row
        ?.querySelector("[data-session-select]")
        ?.getAttribute("aria-describedby")
    ).toBeNull();
    expect(row?.getAttribute("title")).toBeNull();

    view.unmount();
  });

  test("uses semantic activity orbs for running sessions and sidebar loading", () => {
    activateDom();
    disableCanvasDrawing();
    const view = renderRail({
      runningSessions: new Set(["meaningful"]),
      quickQuota: null,
      quickQuotaLoading: true,
    });
    const running = view.container.querySelector(
      '[data-session-id="meaningful"]'
    );
    const runningOrb = running?.querySelector("[data-session-status] canvas");
    const quotaOrb = view.container.querySelector(
      '[data-rail-feature="usage"] canvas[data-activity-state="searching"]'
    );

    expect(runningOrb?.dataset.activityState).toBe("working");
    expect(runningOrb?.style.width).toBe("14px");
    expect(
      running
        ?.querySelector("[data-session-status]")
        ?.getAttribute("aria-label")
    ).toBe("Working");
    expect(
      view.container.querySelector(
        '[data-session-id="punctuation"] [data-session-status]'
      )
    ).toBeNull();
    expect(quotaOrb?.style.width).toBe("14px");
    expect(
      view.container
        .querySelector(
          '[data-rail-feature="usage"] [data-slot="rail-utility-button"]'
        )
        ?.getAttribute("aria-busy")
    ).toBe("true");

    view.unmount();
  });

  test("keeps awaiting-input and failed states visible without restoring routine status copy", () => {
    activateDom();
    const awaiting = {
      ...session("awaiting", "Awaiting task"),
      activity: {
        revision: 2,
        state: { kind: "awaiting_input", turn_id: "turn-1", pending: [] },
      },
    };
    const failed = {
      ...session("failed", "Failed task"),
      activity: {
        revision: 2,
        state: {
          kind: "failed",
          turn_id: "turn-2",
          reason: "provider_error",
          message: "Provider stopped",
        },
      },
    };
    const view = renderRail({
      sessions: [awaiting, failed],
      activeSession: null,
      previews: {},
    });
    const awaitingStatus = view.container.querySelector(
      '[data-session-id="awaiting"] [data-session-status]'
    );
    const failedStatus = view.container.querySelector(
      '[data-session-id="failed"] [data-session-status]'
    );

    expect(awaitingStatus?.getAttribute("aria-label")).toBe("Awaiting input");
    expect(awaitingStatus?.className).toContain("text-warning");
    expect(awaitingStatus?.querySelector(".bg-warning")).toBeTruthy();
    expect(failedStatus?.getAttribute("aria-label")).toBe("Failed");
    expect(failedStatus?.getAttribute("title")).toBe("Provider stopped");
    expect(failedStatus?.className).toContain("text-destructive");
    expect(
      view.container.querySelectorAll('[data-session-line="workspace"]')
    ).toHaveLength(0);
    expect(
      view.container.querySelectorAll(
        '[data-session-line="summary"] [data-session-checkout-kind="checkout"]'
      )
    ).toHaveLength(2);

    view.unmount();
  });

  test("uses compact spacing and exposes real session actions on right click", async () => {
    activateDom();
    const view = renderRail();
    const row = view.container.querySelector('[data-session-id="meaningful"]');
    const activeRow = view.container.querySelector(
      '[data-session-id="punctuation"]'
    );

    expect(row?.dataset.sessionDensity).toBe("compact");
    expect(row?.parentElement?.className).toContain("gap-0.5");
    expect(activeRow?.className).toContain("bg-fill-selected");
    expect(activeRow?.className).toContain("rounded-control");
    expect(activeRow?.className.split(/\s+/)).not.toContain("bg-accent");
    expect(row?.className).toContain("hover:bg-fill-hover");
    expect(row?.className).toContain("focus-within:bg-fill-hover");
    expect(
      row?.querySelector('[data-session-line="title"]')?.className
    ).toContain("gap-2");
    expect(
      row
        ?.querySelector("[data-session-actions]")
        ?.querySelector('button[aria-label="Pin"]')
    ).toBeTruthy();

    row?.dispatchEvent(
      new dom.window.MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: 120,
        clientY: 80,
      })
    );

    await waitFor(() => {
      expect(dom.document.body.textContent).toContain("Copy session ID");
      expect(dom.document.body.textContent).toContain(
        "Reveal working directory"
      );
    });

    const renameItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.trim() === "Rename");
    expect(renameItem).toBeTruthy();
    click(renameItem);
    await waitFor(() => {
      expect(
        view.container.querySelector('input[value="Meaningful"]')
      ).toBeTruthy();
    });

    view.unmount();
  });

  test("finishes the archive exit before moving a session", async () => {
    activateDom();
    const archived = [];
    const view = renderRail({
      onArchive: (id, value) => archived.push([id, value]),
    });
    const row = view.container.querySelector('[data-session-id="meaningful"]');
    const archive = row?.querySelector('button[aria-label="Archive"]');

    click(archive);
    await waitFor(() => {
      expect(row?.getAttribute("data-session-archive-motion")).toBe("archive");
      expect(row?.getAttribute("aria-busy")).toBe("true");
    });
    expect(archived).toEqual([]);

    row?.dispatchEvent(new dom.Event("animationend", { bubbles: true }));
    await waitFor(() => expect(archived).toEqual([["meaningful", true]]));
    await waitFor(() => {
      expect(row?.getAttribute("data-session-archive-motion")).toBeNull();
      expect(row?.getAttribute("aria-busy")).toBeNull();
    });

    view.unmount();
  });

  test("supports keyboard selection, row navigation, and the keyboard context-menu gesture", async () => {
    activateDom();
    const selected = [];
    const view = renderRail({ onSelect: (id) => selected.push(id) });
    const first = view.container.querySelector(
      '[data-session-id="punctuation"]'
    );
    const second = view.container.querySelector(
      '[data-session-id="meaningful"]'
    );
    const firstSelect = first?.querySelector("[data-session-select]");
    const secondSelect = second?.querySelector("[data-session-select]");

    expect(firstSelect?.getAttribute("aria-current")).toBe("page");
    expect(firstSelect?.getAttribute("tabindex")).toBe("0");
    expect(first?.getAttribute("role")).toBeNull();

    firstSelect.focus();
    firstSelect.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", { key: "Enter", bubbles: true })
    );
    expect(selected).toEqual(["punctuation"]);

    firstSelect.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
      })
    );
    expect(dom.document.activeElement).toBe(secondSelect);

    secondSelect.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", {
        key: "F10",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      })
    );
    await waitFor(() => {
      expect(dom.document.body.textContent).toContain("Copy session ID");
    });

    view.unmount();
  });

  test("switches session tabs with an immediate row-local selection surface", () => {
    activateDom();
    const view = renderRail();
    const list = view.container.querySelector("[data-session-list]");
    const activeRow = view.container.querySelector(
      '[data-session-id="punctuation"]'
    );
    const inactiveRow = view.container.querySelector(
      '[data-session-id="meaningful"]'
    );

    expect(list?.dataset.sessionSelection).toBe("instant");
    expect(activeRow?.className.split(/\s+/)).toContain("bg-fill-selected");
    expect(activeRow?.className).not.toContain("transition-[background-color");
    expect(inactiveRow?.className.split(/\s+/)).not.toContain(
      "bg-fill-selected"
    );

    view.unmount();
  });

  test("confirms clipboard actions instead of failing silently", async () => {
    activateDom();
    const copied = [];
    Object.defineProperty(dom.window.navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (value) => copied.push(value) },
    });
    const view = renderRail();
    const row = view.container.querySelector('[data-session-id="meaningful"]');

    row?.dispatchEvent(
      new dom.window.MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: 120,
        clientY: 80,
      })
    );
    await waitFor(() => {
      expect(dom.document.body.textContent).toContain("Copy session ID");
    });

    const copyItem = [
      ...dom.document.body.querySelectorAll('[role="menuitem"]'),
    ].find((item) => item.textContent?.trim() === "Copy session ID");
    expect(copyItem).toBeTruthy();
    click(copyItem);

    await waitFor(() => {
      expect(copied).toEqual(["meaningful"]);
      expect(dom.document.body.textContent).toContain("Session ID copied.");
    });

    view.unmount();
  });
});

describe("SessionRail empty projects", () => {
  test("offers adding a Project when none are registered", () => {
    activateDom();
    let added = 0;
    const view = renderRail({
      projects: [],
      sessions: [],
      previews: {},
      onAddProject: () => {
        added += 1;
      },
    });

    expect(view.container.textContent).not.toContain("No projects yet");
    const addButton = [...view.container.querySelectorAll("button")].find(
      (item) => item.textContent?.trim() === "Add a project…"
    );
    expect(addButton).toBeTruthy();
    click(addButton);
    expect(added).toBe(1);

    view.unmount();
  });
});
