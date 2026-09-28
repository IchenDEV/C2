// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

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
const { I18nProvider } = await import("../src/i18n");
const {
  Dock,
  dockMaxWidth,
  shouldOverlayRailForDock,
  shouldOverlayRailForWorkspace,
} = await import("../src/dock/Dock");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});
function renderDock(
  availableSurfaces,
  tab = "home",
  open = true,
  onTab = () => {}
) {
  return mount(
    <I18nProvider>
      <Dock
        open={open}
        titlebarHost={null}
        tab={tab}
        availableSurfaces={availableSurfaces}
        onTab={onTab}
        onClose={() => {}}
        content={{
          trajectory: (
            <div aria-label="Execution trajectory">
              No events match this view.
            </div>
          ),
          "side-chat": (
            <div aria-label="Side chat conversation">Ask anything</div>
          ),
        }}
        width={440}
        onWidth={() => {}}
      />
    </I18nProvider>
  );
}

describe("Dock plugin component gate", () => {
  test("portals tab controls into the shell while retaining selection and close behavior", async () => {
    activateDom();
    const host = dom.document.createElement("div");
    dom.document.body.append(host);
    const selected = [];
    let closed = 0;
    const render = (open, tab) => (
      <I18nProvider>
        <Dock
          open={open}
          tab={tab}
          titlebarHost={host}
          width={440}
          onWidth={() => {}}
          availableSurfaces={["trajectory", "files"]}
          onTab={(value) => selected.push(value)}
          onClose={() => {
            closed += 1;
          }}
          content={{
            trajectory: <div>Timeline content</div>,
            files: <div>File content</div>,
          }}
        />
      </I18nProvider>
    );
    const view = mount(render(true, "trajectory"));
    await flush();
    expect(view.container.querySelector("[data-dock-titlebar]")).toBeNull();
    expect(host.querySelectorAll('[role="tab"]')).toHaveLength(2);
    const files = [...host.querySelectorAll('[role="tab"]')].find(
      (tab) => tab.textContent === "Files"
    );
    click(files);
    await flush();
    expect(selected).toEqual(["files"]);
    view.rerender(render(true, "files"));
    await flush();
    expect(files.getAttribute("aria-selected")).toBe("true");
    expect(view.container.textContent).toContain("File content");
    expect(host.textContent).not.toContain("File content");
    click(button(host, "Close panel"));
    expect(closed).toBe(1);
    view.rerender(render(false, null));
    await flush();
    expect(host.querySelector("button")).toBeNull();
    view.rerender(render(true, "home"));
    await flush();
    expect(host.querySelectorAll("button")).toHaveLength(1);
    expect(view.container.textContent).toContain("Open a panel");
    view.unmount();
    host.remove();
  });

  test("preserves the document measure after accounting for an inline rail", () => {
    expect(dockMaxWidth(1280, 288)).toBe(372);
    expect(dockMaxWidth(800)).toBe(300);
    expect(shouldOverlayRailForWorkspace(907, 288)).toBe(true);
    expect(shouldOverlayRailForWorkspace(908, 288)).toBe(false);
    expect(shouldOverlayRailForWorkspace(1039, 420)).toBe(true);
    expect(shouldOverlayRailForWorkspace(1040, 420)).toBe(false);
    expect(shouldOverlayRailForDock(1207, 288)).toBe(true);
    expect(shouldOverlayRailForDock(1208, 288)).toBe(false);
  });

  test("removes its resize separator from the tab order while closed", async () => {
    activateDom();
    const view = renderDock(["terminal"], "home", false);
    await flush();
    const separator = view.container.querySelector('[role="separator"]');

    expect(separator?.getAttribute("tabindex")).toBe("-1");
    expect(separator?.getAttribute("aria-disabled")).toBe("true");

    view.unmount();
  });

  test("opens side chat from the right-panel surface picker", async () => {
    activateDom();
    const opened = [];
    const view = renderDock(
      [
        "trajectory",
        "browser",
        "terminal",
        "side-chat",
        "files",
        "git",
        "pull-request",
      ],
      "home",
      true,
      (surface) => opened.push(surface)
    );
    await flush();

    const cards = [
      ...view.container.querySelectorAll(
        '.dock-surface-list [data-slot="navigation-row"]'
      ),
    ];
    expect(cards[2]?.textContent).toContain("Terminal");
    expect(cards[3]?.textContent).toBe("Side chat");
    expect(cards[6]?.textContent).toBe("PR");
    expect(cards.every((card) => card.dataset.slot === "navigation-row")).toBe(
      true
    );
    expect(cards.every((card) => !card.classList.contains("bg-card"))).toBe(
      true
    );
    expect(
      cards.every((card) => card.classList.contains("min-h-navigation-row"))
    ).toBe(true);
    expect(
      cards.every((card) => !card.className.includes("ring-foreground"))
    ).toBe(true);
    expect(
      cards.every((card) =>
        card.querySelector("svg")?.classList.contains("size-4")
      )
    ).toBe(true);
    click(button(view.container, "Side chat"));
    expect(opened).toEqual(["side-chat"]);

    view.unmount();
  });

  test("renders side chat inside the right Dock rather than as a floating dialog", async () => {
    activateDom();
    const view = renderDock(["side-chat"], "side-chat");
    await flush();

    expect(
      view.container.querySelector('[data-dock-placement="right"]')
    ).not.toBeNull();
    expect(
      view.container.querySelector('[aria-label="Side chat conversation"]')
    ).not.toBeNull();
    expect(view.container.querySelector('[role="dialog"]')).toBeNull();

    view.unmount();
  });

  test("advertises the terminal in the horizontally resizable right panel", async () => {
    activateDom();
    const view = renderDock(["terminal"], "home");
    await flush();

    const panel = view.container.querySelector('[data-dock-placement="right"]');
    expect(panel).not.toBeNull();
    expect(panel?.classList.contains("dock-panel-side")).toBe(true);
    expect(panel?.classList.contains("border-l")).toBe(false);
    const surface = panel?.querySelector('[data-slot="card"]');
    expect(surface?.getAttribute("data-variant")).toBe("raised");
    expect(surface?.classList.contains("m-2")).toBe(true);
    expect(panel?.getAttribute("style")).toMatch(/^width: \d+px;$/u);
    expect(panel?.getAttribute("style")).not.toContain("height");
    expect(
      panel?.querySelector('[data-dock-resize="horizontal"]')
    ).not.toBeNull();
    expect(view.container.textContent).toContain("Terminal");

    view.unmount();
  });

  test("advertises only enabled surfaces and refuses to mount a disabled requested tab", async () => {
    activateDom();
    const view = renderDock(["files"], "terminal");
    await flush();

    expect(view.container.textContent).toContain("Files");
    expect(view.container.textContent).not.toContain("Browser");
    expect(view.container.textContent).not.toContain("Terminal");
    expect(view.container.textContent).not.toContain("Source control");
    expect(
      view.container.querySelector(
        '[data-slot="tabs-content"][data-value="terminal"]'
      )
    ).toBeNull();

    view.unmount();
  });

  test("renders caller-supplied trajectory content as a right-panel module", async () => {
    activateDom();
    const home = renderDock(["trajectory"], "home");
    await flush();

    expect(home.container.textContent).toContain("Execution trajectory");
    expect(home.container.textContent).toContain("Open a panel");
    expect(home.container.textContent).not.toContain(
      "Inspect the session timeline"
    );
    home.unmount();

    const module = renderDock(["trajectory"], "trajectory");
    await flush();

    expect(module.container.querySelector('[role="tabpanel"]')).not.toBeNull();
    expect(
      module.container.querySelector('[aria-label="Execution trajectory"]')
    ).not.toBeNull();
    expect(module.container.textContent).toContain(
      "No events match this view."
    );
    module.unmount();
  });
});
