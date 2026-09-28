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
const { PluginManagerPage, PluginUiSlot, buildPluginManagerCatalog } =
  await import("../src/plugins");
const desktopStyles = readFileSync(
  new URL("../src/styles.css", import.meta.url),
  "utf-8"
);

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

const plugins = [
  {
    id: "memory",
    name: "Memory",
    description: "Recall and capture project context.",
    version: "1.0.0",
    author: "C2",
    source: "builtin",
    category: "foundation",
    supportedScopes: ["user", "project"],
    dependencies: ["store"],
    commands: ["memory.search"],
    services: ["MemoryCapability"],
    state: {
      effectiveEnabled: true,
      override: "inherit",
      status: "active",
      config: {
        endpoint: "local",
        interval: 15,
        telemetry: false,
        mode: "balanced",
      },
    },
    configSchema: {
      type: "object",
      required: ["endpoint"],
      properties: {
        endpoint: { type: "string", title: "Endpoint" },
        interval: { type: "integer", title: "Interval" },
        telemetry: { type: "boolean", title: "Telemetry" },
        mode: { type: "string", title: "Mode", enum: ["balanced", "strict"] },
      },
    },
  },
  {
    id: "terminal",
    name: "Terminal",
    description: "Desktop terminal host.",
    source: "host",
    supportedScopes: ["user", "project"],
    state: {
      effectiveEnabled: false,
      override: "disabled",
      status: "disabled",
    },
  },
  {
    id: "review-tools",
    name: "Review Tools",
    description: "Installed review workflow bundle.",
    source: "bundle",
    sourceLabel: "Bundle · Review Tools",
    supportedScopes: ["user"],
    state: {
      effectiveEnabled: true,
      status: "failed",
      missingDependencies: ["git"],
      error: "Git service is unavailable.",
    },
    bundle: {
      id: "review-tools",
      repository: "https://example.test/review-tools",
      standardVersion: "1.2.0",
      trusted: false,
      enabled: true,
      requiresTrust: true,
      runtimeManaged: false,
      contributions: [
        { id: "runtime", label: "Process runtime", count: 1 },
        { id: "skills", label: "Skills", count: 2 },
      ],
      diagnostics: [{ level: "warning", message: "Trust before running." }],
      scaffolds: [],
    },
  },
];

const components = [
  {
    id: "skill:docs-search",
    pluginId: "memory",
    pluginName: "Memory",
    policyPluginId: "memory",
    name: "docs-search",
    description: "Search the project documentation through MCP.",
    kind: "mcp",
    source: "builtin",
    supportedScopes: ["user", "project"],
    manageable: true,
    state: { effectiveEnabled: true, status: "active" },
    skill: { id: "docs-search", removable: false },
  },
  {
    id: "skill:release-review",
    pluginId: "review-tools",
    pluginName: "Review Tools",
    policyPluginId: "memory",
    name: "Release review",
    description: "Review a release before deployment.",
    kind: "agent_skill",
    source: "bundle",
    sourceLabel: "Bundle · Review Tools",
    supportedScopes: ["user", "project"],
    manageable: true,
    state: { effectiveEnabled: true, status: "active" },
    skill: { id: "release-review", removable: false },
  },
  {
    id: "review-tools:hook:session",
    pluginId: "review-tools",
    pluginName: "Review Tools",
    name: "session",
    description: "hooks/hooks.json",
    kind: "hook",
    slot: "hooks/hooks.json",
    source: "bundle",
    sourceLabel: "Bundle · Review Tools",
    supportedScopes: ["user"],
    manageable: false,
    availability: "unsupported",
    state: { effectiveEnabled: true, status: "unsupported" },
  },
];

const marketplaceItems = [
  {
    id: "release-review",
    name: "Release Review",
    description: "Review a release before deployment.",
    version: "1.2.0",
    author: "C2 Labs",
    kind: "bundle",
    sourceLabel: "Marketplace",
    installed: false,
    installable: true,
    supportedScopes: ["user", "project"],
  },
];

function renderManager(overrides = {}) {
  const calls = {
    scopes: [],
    planned: [],
    applied: [],
    configs: [],
    installs: [],
    imports: [],
    bundleEnabled: [],
    bundleTrust: [],
    uninstalls: [],
    scaffolds: [],
    marketplaceOpens: [],
  };
  const view = mount(
    <PluginManagerPage
      plugins={plugins}
      components={components}
      marketplaceItems={marketplaceItems}
      scope={{ kind: "user" }}
      projects={[{ path: "/tmp/project", label: "Project · demo" }]}
      onScopeChange={(scope) => calls.scopes.push(scope)}
      onPlanChange={async (request) => {
        calls.planned.push(request);
        return {
          confirmationId: "plan-1",
          request,
          summary: "The memory worker will stop before the plugin unloads.",
          requiresConfirmation: true,
          affectedPlugins: [
            {
              id: "memory",
              name: "Memory",
              desiredState: request.desiredState,
            },
          ],
          activeResources: [
            {
              id: "worker-1",
              label: "Memory capture worker",
              kind: "background task",
            },
          ],
        };
      }}
      onApplyChange={async (plan) => calls.applied.push(plan)}
      onSaveConfig={async (request) => calls.configs.push(request)}
      onInstallMarketplaceItem={async (request) => calls.installs.push(request)}
      onImportGithub={async (repository) => {
        calls.imports.push(repository);
        return {
          pluginId: "review-tools",
          name: "Installed Tools",
          version: "2.0.0",
        };
      }}
      onSetBundleEnabled={async (pluginId, enabled) =>
        calls.bundleEnabled.push({ pluginId, enabled })
      }
      onSetBundleTrusted={async (pluginId, trusted) =>
        calls.bundleTrust.push({ pluginId, trusted })
      }
      onUninstallBundle={async (pluginId, keepData) =>
        calls.uninstalls.push({ pluginId, keepData })
      }
      onApplyScaffold={async (pluginId, scaffoldId) => {
        calls.scaffolds.push({ pluginId, scaffoldId });
        return { files: 2 };
      }}
      onOpenMarketplace={async () => calls.marketplaceOpens.push(true)}
      {...overrides}
    />
  );
  return { view, calls };
}

async function setInputValue(input, value) {
  await reactAct(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      dom.window.HTMLInputElement.prototype,
      "value"
    )?.set;
    setter.call(input, value);
    input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
    input.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
  });
  await flush();
}

async function openSelect(trigger) {
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

async function selectItem(item) {
  await reactAct(async () => {
    item.dispatchEvent(
      new dom.window.PointerEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      })
    );
    item.dispatchEvent(
      new dom.window.PointerEvent("pointerup", {
        bubbles: true,
        cancelable: true,
        button: 0,
        pointerId: 1,
      })
    );
    item.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
    );
  });
  await flush();
}

describe("PluginManagerPage", () => {
  test("does not show a missing-dependency warning for zero missing dependencies", () => {
    const { view } = renderManager({
      plugins: [
        {
          ...plugins[0],
          state: { ...plugins[0].state, missingDependencies: [] },
        },
      ],
    });
    expect(dom.document.body.textContent).not.toContain("0 missing");
    view.unmount();
  });

  test("opens a requested plugin and renders only its host-owned settings extension", async () => {
    activateDom();
    const { view } = renderManager({
      initialPluginId: "review-tools",
      pluginDetailsExtension: {
        pluginId: "review-tools",
        content: (
          <section data-host-plugin-settings>
            Feishu connection settings
          </section>
        ),
      },
    });
    await flush();

    expect(
      view.container.querySelector(
        '[aria-label="Plugin list"] [data-selected="true"]'
      )?.textContent
    ).toContain("Review Tools");
    expect(
      view.container.querySelector("[data-plugin-details]")?.textContent
    ).toContain("Review Tools");
    expect(
      view.container.querySelector("[data-host-plugin-settings]")?.textContent
    ).toBe("Feishu connection settings");

    click(
      [...view.container.querySelectorAll("button")].find((candidate) =>
        candidate.textContent?.includes("Memory")
      )
    );
    await flush();
    expect(
      view.container.querySelector("[data-host-plugin-settings]")
    ).toBeNull();

    view.unmount();
  });

  test("keeps the sidebar expansion action available on the full-page plugin surface", async () => {
    activateDom();
    const expanded = [];
    const { view } = renderManager({
      headerLeadingAction: (
        <button type="button" onClick={() => expanded.push(true)}>
          Expand sidebar
        </button>
      ),
    });
    await flush();

    const action = view.container.querySelector(
      "[data-plugin-manager-leading-action]"
    );
    const detailAction = view.container.querySelector(
      "[data-plugin-manager-detail-leading-action]"
    );
    expect(action).not.toBeNull();
    expect(action?.textContent).toContain("Expand sidebar");
    expect(
      view.container.querySelector(".plugin-manager-list-header")?.className
    ).toContain("pl-surface-inset");
    expect(
      view.container.querySelector(".plugin-manager-list-header h1")?.className
    ).not.toContain("ms-surface-inset");
    expect(detailAction).not.toBeNull();
    expect(detailAction?.textContent).toContain("Expand sidebar");
    click(action?.querySelector("button"));
    click(detailAction?.querySelector("button"));
    expect(expanded).toEqual([true, true]);

    view.unmount();
  });

  test("renders a host-owned plugin action slot and invokes its declared action", async () => {
    activateDom();
    const invoked = [];
    const contribution = {
      id: "review",
      pluginId: "review-tools",
      pluginName: "Review Tools",
      slot: "composer.above",
      label: "Review workspace",
      description: "Check the current changes before commit.",
      command: "review.run",
      input: null,
      order: 10,
    };
    const view = mount(
      <PluginUiSlot
        slot="composer.above"
        contributions={[contribution]}
        onInvoke={async (value) => invoked.push(value)}
      />
    );
    await flush();

    expect(
      view.container
        .querySelector('[data-plugin-ui-slot="composer.above"]')
        ?.getAttribute("aria-label")
    ).toBe("Plugin actions");
    expect(view.container.textContent).toContain("Review workspace");
    expect(view.container.textContent).not.toContain(contribution.description);
    click(button(view.container, "Review Tools: Review workspace"));
    await flush();
    expect(invoked).toEqual([contribution]);

    view.unmount();
  });

  test("renders all supported plugin action slots with host-owned controls", async () => {
    activateDom();
    const invoked = [];
    const slots = [
      "rail.features",
      "session.header",
      "transcript.before",
      "composer.above",
      "composer.toolbar",
    ];
    const contributions = slots.map((slot, order) => ({
      id: `action-${order}`,
      pluginId: "review-tools",
      pluginName: "Review Tools",
      slot,
      label: `Action ${order}`,
      description: `Action ${order} description`,
      command: "review.run",
      input: null,
      order,
    }));
    const view = mount(
      <div>
        {slots.map((slot, order) => (
          <PluginUiSlot
            key={slot}
            slot={slot}
            contributions={[contributions[order]]}
            onInvoke={async (value) => invoked.push(value)}
          />
        ))}
      </div>
    );
    await flush();

    for (const [order, slot] of slots.entries()) {
      const region = view.container.querySelector(
        `[data-plugin-ui-slot="${slot}"]`
      );
      expect(region).not.toBeNull();
      const action =
        region.querySelector(
          `button[aria-label="Review Tools: Action ${order}"]`
        ) ?? button(region, `Action ${order}`);
      expect(action).not.toBeNull();
      if (slot === "session.header") {
        expect(action.dataset.variant).toBe("ghost");
        expect(action.classList.contains("text-muted-foreground")).toBe(true);
        expect(action.classList.contains("hover:text-muted-foreground")).toBe(
          true
        );
      }
    }

    const railAction = view.container.querySelector(
      '[data-plugin-ui-slot="rail.features"] button'
    );
    expect(railAction?.className).toContain("h-control");
    expect(railAction?.className).toContain("text-body");
    expect(railAction?.className).toContain("text-foreground/75");

    const toolbar = view.container.querySelector(
      '[data-plugin-ui-slot="composer.toolbar"]'
    );
    click(toolbar.querySelector('button[aria-label="Review Tools: Action 4"]'));
    await flush();
    expect(invoked).toEqual([contributions[4]]);

    view.unmount();
  });

  test("renders one scoped manager for built-in, host, bundle, and marketplace data", async () => {
    activateDom();
    const { view } = renderManager();
    await flush();

    expect(
      view.container.querySelector("[data-plugin-manager-page]")?.tagName
    ).toBe("MAIN");
    expect(
      view.container
        .querySelector(".plugin-manager-list-header")
        ?.querySelectorAll('[role="tab"]')
    ).toHaveLength(0);
    expect(
      view.container
        .querySelector("[data-plugin-manager-page]")
        ?.classList.contains("@container/plugin-manager")
    ).toBe(true);
    expect(
      view.container
        .querySelector("[data-plugin-manager-page]")
        ?.classList.contains("plugin-manager-page")
    ).toBe(true);
    expect(
      view.container.querySelector(".plugin-manager-list-pane")
    ).not.toBeNull();
    expect(
      view.container.querySelector(".plugin-manager-list-header")?.className
    ).toContain("h-layout-titlebar");
    expect(
      view.container.querySelector(".plugin-manager-list-header")?.className
    ).toContain("pl-page-section");
    expect(
      view.container.querySelector(".plugin-manager-list-header h1")?.className
    ).not.toContain("ms-surface-inset");
    expect(
      view.container.querySelector(".plugin-manager-detail-header")?.className
    ).toContain("h-layout-titlebar");
    expect(view.container.querySelector(".plugin-manager-tabs")).not.toBeNull();
    expect(
      view.container
        .querySelector(".plugin-manager-tabs")
        ?.closest('[data-slot="tabs"]')?.className
    ).toContain("ms-surface-inset");
    expect(
      view.container.querySelector(".plugin-manager-tabs")?.className
    ).toContain("overflow-x-auto");
    expect(
      view.container.querySelectorAll(".plugin-manager-tab-count")
    ).toHaveLength(5);
    expect(
      view.container.querySelectorAll("[data-plugin-manager-tab-label]")
    ).toHaveLength(5);
    const listControls = view.container.querySelector(
      "[data-plugin-manager-list-controls]"
    );
    expect(listControls?.className).toContain("px-2");
    expect(listControls?.className).not.toContain("px-4");
    expect(listControls?.querySelectorAll('[role="tab"]')).toHaveLength(5);
    expect(
      listControls?.querySelector("[data-plugin-manager-search]")
    ).not.toBeNull();
    expect(
      listControls?.querySelector("[data-plugin-manager-search-field]")
    ).not.toBeNull();
    expect(
      listControls?.querySelector("[data-plugin-manager-search-field]")
        ?.className
    ).toContain("ms-inline");
    expect(desktopStyles).toContain(
      "container: plugin-manager-list / inline-size"
    );
    expect(desktopStyles).toContain(
      "@container plugin-manager-list (max-width: 24rem)"
    );
    expect(desktopStyles).toMatch(
      /\.plugin-manager-tabs\s*{[^}]*gap:\s*var\(--ds-space-inline\)/s
    );
    expect(desktopStyles).toMatch(
      /\.plugin-manager-tabs \.plugin-manager-tab\s*{[^}]*padding-inline:\s*var\(--ds-space-inline\)/s
    );
    expect(desktopStyles).not.toContain(
      ".plugin-manager-tab:not(:first-of-type)"
    );
    expect(
      view.container.querySelector(".plugin-manager-detail-pane")
    ).not.toBeNull();
    expect(
      view.container.querySelector("[data-plugin-manager-page]")?.dataset
        .compactDetail
    ).toBe("true");
    expect(
      view.container
        .querySelector("[data-plugin-manager-scroll]")
        ?.classList.contains("w-full")
    ).toBe(true);
    expect(
      view.container
        .querySelector("[data-plugin-manager-search]")
        ?.classList.contains("w-full")
    ).toBe(true);
    expect(view.container.querySelector("[data-plugin-details]")?.tagName).toBe(
      "ARTICLE"
    );
    expect(
      view.container.querySelector(
        '[aria-label="Plugin list"] [data-selected="true"]'
      )?.textContent
    ).toContain("Memory");
    expect(
      view.container.querySelector(
        '[aria-label="Plugin list"] [data-selected="true"] > span:first-child'
      )?.className
    ).toContain("text-foreground");
    expect(
      view.container.querySelector(".plugin-manager-list-header h1")
        ?.textContent
    ).toBe("Features & plugins");
    expect(
      view.container.querySelector(
        '[aria-label="Plugin list"] [data-selected="true"] [data-plugin-status="active"] .bg-success'
      )
    ).not.toBeNull();
    expect(view.container.textContent).toContain("Built-in feature");
    expect(view.container.textContent).toContain("Host feature");
    expect(view.container.textContent).toContain("Bundle · Review Tools");
    expect(
      view.container.querySelector('[data-slot="select-trigger"]')?.textContent
    ).toContain("User");
    expect(
      view.container.querySelector("[data-plugin-details]")?.textContent
    ).toContain("MemoryCapability");
    expect(
      view.container.querySelector(
        "[data-plugin-details] [data-slot='status-badge']"
      )?.dataset.tone
    ).toBe("success");
    expect(
      view.container.querySelector("#plugin-config-endpoint")
    ).not.toBeNull();
    expect(
      view.container
        .querySelector("#plugin-config-interval")
        ?.getAttribute("step")
    ).toBe("1");
    expect(
      view.container.querySelector('[data-slot="checkbox"]')
    ).not.toBeNull();
    expect(button(view.container, "Install from GitHub")).not.toBeNull();
    expect(button(view.container, "MCPs 1")).not.toBeNull();
    expect(button(view.container, "Skills 1")).not.toBeNull();
    expect(button(view.container, "Hooks 1")).not.toBeNull();

    click(button(view.container, "MCPs 1"));
    await flush();
    expect(
      view.container.querySelector("[data-resource-details]")?.textContent
    ).toContain("docs-search");
    expect(
      view.container
        .querySelector("[data-plugin-manager-search]")
        ?.getAttribute("placeholder")
    ).toBe("Search MCP servers…");

    click(button(view.container, "Marketplace 1"));
    await flush();
    expect(view.container.textContent).toContain("Release Review");
    expect(button(view.container, "Install")).not.toBeNull();

    view.unmount();
  });

  test("manages MCP and skill policy while routing hook management to its plugin", async () => {
    activateDom();
    const { view, calls } = renderManager();
    await flush();

    click(button(view.container, "MCPs 1"));
    await flush();
    click(
      view.container.ownerDocument.getElementById(
        "component-state-skill:docs-search"
      )
    );
    await flush();
    expect(calls.planned[0]).toMatchObject({
      targetKind: "component",
      targetId: "skill:docs-search",
      desiredState: "disabled",
      scope: { kind: "user" },
    });
    click(button(dom.document.body, "Cancel"));
    await flush();

    click(button(view.container, "Skills 1"));
    await flush();
    expect(
      view.container.querySelector("[data-resource-details]")?.textContent
    ).toContain("Release review");

    click(button(view.container, "Hooks 1"));
    await flush();
    expect(
      view.container.querySelector("[data-resource-details]")?.textContent
    ).toContain("Unsupported");
    click(button(view.container, "Manage plugin"));
    await flush();
    expect(
      view.container.querySelector("[data-plugin-details]")?.textContent
    ).toContain("Review Tools");

    view.unmount();
  });

  test("shows inline GitHub bundle validation on the primary management page", async () => {
    activateDom();
    const { view } = renderManager();
    await flush();

    click(button(view.container, "Install from GitHub"));
    await flush();
    const installer = view.container.querySelector(
      "[data-plugin-github-installer]"
    );
    expect(installer).not.toBeNull();

    click(button(installer, "Install"));
    await flush();
    expect(installer.querySelector('[role="alert"]')?.textContent).toContain(
      "owner/repository"
    );
    expect(
      installer
        .querySelector("#plugin-github-repository")
        ?.getAttribute("aria-invalid")
    ).toBe("true");

    view.unmount();
  });

  test("maps failed plugin and trusted bundle states independently", async () => {
    activateDom();
    const trustedPlugins = plugins.map((plugin) =>
      plugin.id === "review-tools"
        ? { ...plugin, bundle: { ...plugin.bundle, trusted: true } }
        : plugin
    );
    const { view } = renderManager({ plugins: trustedPlugins });
    await flush();

    const bundleButton = [...view.container.querySelectorAll("button")].find(
      (candidate) => candidate.textContent?.includes("Review Tools")
    );
    click(bundleButton);
    await flush();

    const detailsTones = [
      ...view.container.querySelectorAll(
        "[data-plugin-details] [data-slot='status-badge']"
      ),
    ].map((badge) => badge.dataset.tone);
    expect(detailsTones.filter((tone) => tone === "destructive")).toHaveLength(
      2
    );
    expect(
      view.container.querySelector(
        "[data-bundle-administration] [data-slot='status-badge']"
      )?.dataset.tone
    ).toBe("success");
    view.unmount();
  });

  test("manages bundle trust and confirms removal without discarding data by default", async () => {
    activateDom();
    const { view, calls } = renderManager();
    await flush();

    const bundleButton = [...view.container.querySelectorAll("button")].find(
      (candidate) => candidate.textContent?.includes("Review Tools")
    );
    click(bundleButton);
    await flush();

    const administration = view.container.querySelector(
      "[data-bundle-administration]"
    );
    const details = view.container.querySelector("[data-plugin-details]");
    expect(administration?.textContent).toContain("Bundle management");
    expect(administration?.textContent).toContain("1 Process runtime");
    expect(administration?.textContent).toContain("Trust before running.");
    expect(
      administration?.querySelector('[data-slot="status-badge"]')?.dataset.tone
    ).toBe("destructive");
    expect(
      details?.querySelector("[data-plugin-trust-gate]")?.textContent
    ).toBe("Trust required before enabling");
    expect(details?.querySelector("#plugin-state-review-tools")).toBeNull();
    expect(button(administration, "Review source")?.dataset.variant).toBe(
      "secondary"
    );
    expect(button(administration, "Trust plugin")?.dataset.variant).toBe(
      "default"
    );
    expect(button(administration, "Uninstall")?.dataset.variant).toBe("ghost");

    click(button(administration, "Trust plugin"));
    await flush();
    expect(calls.bundleTrust).toEqual([
      { pluginId: "review-tools", trusted: true },
    ]);

    click(button(administration, "Uninstall"));
    await flush();
    const dialog = dom.document.body.querySelector(
      '[data-slot="alert-dialog-content"]'
    );
    expect(dialog?.textContent).toContain("Uninstall Review Tools?");
    expect(
      dialog?.querySelector("#keep-plugin-data-review-tools")?.checked
    ).toBe(true);

    click(button(dialog, "Uninstall"));
    await flush();
    expect(calls.uninstalls).toEqual([
      { pluginId: "review-tools", keepData: true },
    ]);

    view.unmount();
  });

  test("keeps scaffolds and local marketplace actions on the unified page", async () => {
    activateDom();
    const bundleWithScaffold = {
      ...plugins[2],
      bundle: {
        ...plugins[2].bundle,
        scaffolds: [
          {
            id: "review-config",
            name: "Review configuration",
            description: "Adds review defaults.",
            files: 2,
          },
        ],
      },
    };
    const localItem = {
      ...marketplaceItems[0],
      id: "marketplace:local:review-tools",
      marketplace: {
        manifestPath: "/tmp/marketplace.json",
        pluginName: "review-tools",
      },
    };
    const { view, calls } = renderManager({
      plugins: [bundleWithScaffold],
      marketplaceItems: [localItem],
      marketplaceSources: [
        {
          id: "/tmp/marketplace.json",
          name: "Local tools",
          description: "Workspace-local plugin catalog.",
          diagnostics: [],
        },
      ],
    });
    await flush();

    click(button(view.container, "Add to project"));
    await flush();
    expect(calls.scaffolds).toEqual([
      { pluginId: "review-tools", scaffoldId: "review-config" },
    ]);
    expect(view.container.textContent).toContain("2 project files added.");

    click(button(view.container, "Marketplace 1"));
    await flush();
    expect(view.container.textContent).toContain("Local tools");
    click(button(view.container, "Load local marketplace"));
    click(button(view.container, "Install"));
    await flush();
    expect(calls.marketplaceOpens).toEqual([true]);
    expect(calls.installs).toEqual([
      { itemId: "marketplace:local:review-tools", scope: { kind: "user" } },
    ]);

    view.unmount();
  });

  test("uses direct bundle lifecycle for data-only bundles instead of the managed runtime plan", async () => {
    activateDom();
    const dataOnly = {
      ...plugins[2],
      state: { effectiveEnabled: true, status: "active" },
      bundle: {
        ...plugins[2].bundle,
        trusted: true,
        requiresTrust: false,
        runtimeManaged: false,
        contributions: [{ id: "skills", label: "Skills", count: 2 }],
        diagnostics: [],
      },
    };
    const { view, calls } = renderManager({ plugins: [dataOnly] });
    await flush();

    click(view.container.querySelector("#plugin-state-review-tools"));
    await flush();

    expect(calls.bundleEnabled).toEqual([
      { pluginId: "review-tools", enabled: false },
    ]);
    expect(calls.planned).toHaveLength(0);

    view.unmount();
  });

  test("keeps an active project scope visible when it is not in the recent-project list", async () => {
    activateDom();
    const { view } = renderManager({
      scope: { kind: "project", projectPath: "/tmp/unlisted" },
      projects: [],
    });
    await flush();

    expect(
      view.container.querySelector('[data-slot="select-trigger"]')?.textContent
    ).toContain("/tmp/unlisted");

    view.unmount();
  });

  test("plans and confirms a live unload before applying it", async () => {
    activateDom();
    const { view, calls } = renderManager();
    await flush();

    const enabled = view.container.querySelector("#plugin-state-memory");
    click(enabled);
    await flush();

    expect(calls.planned).toHaveLength(1);
    expect(calls.planned[0]).toMatchObject({
      targetKind: "plugin",
      targetId: "memory",
      desiredState: "disabled",
      scope: { kind: "user" },
    });
    expect(
      dom.document.body.querySelector('[data-slot="alert-dialog-content"]')
        ?.textContent
    ).toContain("Memory capture worker");

    click(button(dom.document.body, "Apply change"));
    await flush();
    expect(calls.applied).toHaveLength(1);
    expect(calls.applied[0].confirmationId).toBe("plan-1");

    view.unmount();
  });

  test("edits supported schema fields and saves structured configuration", async () => {
    activateDom();
    const { view, calls } = renderManager();
    await flush();

    await setInputValue(
      view.container.querySelector("#plugin-config-endpoint"),
      "remote"
    );
    await setInputValue(
      view.container.querySelector("#plugin-config-interval"),
      "30"
    );
    click(button(view.container, "Save configuration"));
    await flush();

    expect(calls.configs).toHaveLength(1);
    expect(calls.configs[0]).toEqual({
      pluginId: "memory",
      scope: { kind: "user" },
      config: {
        endpoint: "remote",
        interval: 30,
        telemetry: false,
        mode: "balanced",
      },
    });

    click(button(view.container, "Advanced JSON"));
    await flush();
    expect(
      view.container.querySelector("#plugin-config-json")?.value
    ).toContain('"endpoint": "remote"');

    view.unmount();
  });

  test("uses inherit-enabled-disabled controls for project-scoped entries", async () => {
    activateDom();
    const { view, calls } = renderManager({
      scope: { kind: "project", projectPath: "/tmp/project" },
    });
    await flush();

    const trigger = view.container.querySelector(
      '[aria-label="Memory project state"]'
    );
    expect(trigger?.textContent).toContain("Inherit");
    await openSelect(trigger);
    const disabled = [
      ...dom.document.body.querySelectorAll('[data-slot="select-item"]'),
    ].find((item) => item.textContent?.trim() === "Disabled");
    await selectItem(disabled);

    expect(calls.planned[0]).toMatchObject({
      targetKind: "plugin",
      targetId: "memory",
      desiredState: "disabled",
      scope: { kind: "project", projectPath: "/tmp/project" },
    });

    view.unmount();
  });

  test("renders backend-managed project state for an installed bundle", async () => {
    activateDom();
    const metadata = {
      origin: "third_party",
      role: "extension",
      category: "developer_tools",
      scope_support: ["user", "project"],
      essential: false,
      default_enabled: true,
    };
    const policy = {
      id: "bundle:review",
      description: "Review bundle",
      metadata,
      dependencies: { required: [], optional: [] },
      state: "disabled",
      enabled: false,
      running: false,
      status: "disposed",
      missing: [],
      error: null,
      config: {},
      schema: null,
      available: true,
      components: {},
      commands: [],
      services: [],
    };
    const model = buildPluginManagerCatalog({
      catalog: {
        graph_revision: 3,
        config_revision: 2,
        recovery: { kind: "normal" },
        plugins: [policy],
      },
      userCatalog: {
        graph_revision: 2,
        config_revision: 1,
        recovery: { kind: "normal" },
        plugins: [
          {
            ...policy,
            state: "enabled",
            enabled: true,
            running: true,
            status: "active",
          },
        ],
      },
      bundles: [
        {
          id: "review",
          name: "Review Tools",
          version: "1.0.0",
          description: "Review bundle",
          author: "C2",
          source: "GitHub · c2/review",
          repository: "https://example.test/review",
          standard_version: "1.2.0",
          enabled: true,
          trusted: true,
          scope: "user",
          counts: {},
          scaffolds: [],
          extension_components: [],
          runtime_commands: [],
          ui_contributions: [],
          connector_contributions: [],
          lsp_servers: [],
          diagnostics: [],
        },
      ],
      skills: [],
      market: [],
      scope: { kind: "project", projectPath: "/tmp/project" },
    });
    const { view, calls } = renderManager({
      plugins: model.plugins,
      components: model.components,
      marketplaceItems: model.marketplaceItems,
      scope: { kind: "project", projectPath: "/tmp/project" },
    });
    await flush();

    const trigger = view.container.querySelector(
      '[aria-label="Review Tools project state"]'
    );
    expect(trigger?.textContent).toContain("Disabled");
    await openSelect(trigger);
    const enabled = [
      ...dom.document.body.querySelectorAll('[data-slot="select-item"]'),
    ].find((item) => item.textContent?.trim() === "Enabled");
    await selectItem(enabled);

    expect(calls.planned[0]).toMatchObject({
      targetKind: "plugin",
      targetId: "bundle:review",
      desiredState: "enabled",
      scope: { kind: "project", projectPath: "/tmp/project" },
    });

    view.unmount();
  });

  test("falls back to JSON when a schema contains unsupported nested structures", async () => {
    activateDom();
    const nested = {
      ...plugins[0],
      id: "nested",
      name: "Nested config",
      state: { ...plugins[0].state, config: { rules: ["one"] } },
      configSchema: {
        type: "object",
        properties: { rules: { type: "array", items: { type: "string" } } },
      },
    };
    const { view } = renderManager({ plugins: [nested] });
    await flush();

    expect(view.container.querySelector("#plugin-config-json")).not.toBeNull();
    expect(view.container.querySelectorAll('[role="tab"]')).toHaveLength(5);

    view.unmount();
  });

  test("surfaces recovery state and resets a managed plugin", async () => {
    activateDom();
    const resets = [];
    const { view } = renderManager({
      recovery: { kind: "safe_mode", error: "invalid JSON" },
      onResetPlugin: async (pluginId, scope) =>
        resets.push({ pluginId, scope }),
    });
    await flush();

    expect(
      view.container.querySelector('[data-plugin-recovery="safe_mode"]')
        ?.textContent
    ).toContain("safe mode");
    expect(view.container.textContent).toContain("invalid JSON");
    click(button(view.container, "Reset to defaults"));
    await flush();
    expect(resets).toEqual([{ pluginId: "memory", scope: { kind: "user" } }]);

    view.unmount();
  });
});
