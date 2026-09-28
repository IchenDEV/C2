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
const { SceneChip, ScenePicker, SourceBadge } =
  await import("../src/session/SceneChip");
const { SessionControls } = await import("../src/session/Composer");
const { I18nProvider } = await import("../src/i18n");

afterEach(() => {
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
    execution: { session_mode: "auto_edit" },
    ...overrides,
  };
}

function config(overrides = {}) {
  return {
    providers: [],
    providersStatus: "ready",
    provider: "claude_code",
    onProvider: () => {},
    onProviderModel: () => {},
    providerChangeDisabled: false,
    onReloadProviders: () => {},
    mode: "ask",
    sandbox: "workspace_write",
    modeChangeDisabled: false,
    onSessionMode: () => true,
    worktreeBase: null,
    activeWorktreeBaseline: null,
    activeWorktreeUnknown: false,
    worktreeOptions: [],
    worktreeOptionsLoading: false,
    onWorktreeBase: () => {},

    memoryEnabled: true,
    memoryRead: "inherit",
    memoryWrite: "inherit",
    onMemoryPolicy: () => {},
    hasSession: false,
    scenesEnabled: true,
    scenes: [sceneInfo()],
    activeScene: sceneInfo(),
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

function renderChip(cfg) {
  return mount(
    <I18nProvider>
      <SceneChip config={cfg} />
    </I18nProvider>
  );
}

describe("Provider/model picker", () => {
  test("locks provider replacement while the session runtime is busy", () => {
    activateDom();
    const providerModelChanges = [];
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({
            hasSession: true,
            provider: "codex",
            providerChangeDisabled: true,
            providers: [
              {
                id: "codex",
                display_name: "Codex",
                available: true,
                enabled: true,
                models: [{ id: "gpt-5.6-sol", name: "GPT-5.6-Sol" }],
              },
            ],
            onProviderModel: (provider, model) =>
              providerModelChanges.push([provider, model]),
          })}
          models={[{ id: "gpt-5.6-sol", name: "GPT-5.6-Sol" }]}
          currentModel="gpt-5.6-sol"
          defaultModel="gpt-5.6-sol"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );
    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    expect(trigger?.disabled).toBe(true);
    trigger?.click();
    expect(providerModelChanges).toEqual([]);
    rendered.unmount();
  });

  test("offers retry without fabricated providers when desktop detection fails", async () => {
    activateDom();
    let retries = 0;
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({
            providers: [],
            providersStatus: "error",
            provider: "grok",
            onReloadProviders: () => {
              retries += 1;
            },
          })}
          models={[]}
          currentModel={null}
          defaultModel={null}
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );
    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
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

    try {
      const popup = dom.document.body.querySelector(
        '[data-slot="popover-content"]'
      );
      expect(trigger?.textContent).toContain("Default model");
      expect(popup?.querySelectorAll('[role="option"]').length).toBe(0);
      button(popup, "Retry").click();
      expect(retries).toBe(1);
    } finally {
      rendered.unmount();
      await flush();
    }
  });
});

describe("SceneChip", () => {
  test("shows the active scene's title on the chip", () => {
    activateDom();
    const rendered = renderChip(config());
    expect(rendered.container.textContent).toContain("Develop");
    expect(rendered.container.textContent).not.toContain("🛠️");
    rendered.unmount();
  });

  test("keeps the scene popover focused on scene selection", async () => {
    activateDom();
    const rendered = renderChip(config());
    const trigger = rendered.container.querySelector(
      '[aria-label="Scene: Develop"]'
    );
    expect(trigger).toBeTruthy();

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

    const content = dom.document.body.textContent ?? "";
    expect(content).toContain("Auto scene");
    expect(content).toContain("Keep current configuration");
    expect(content).toContain("Manage scenes");
    expect(content).not.toContain("Memory on");
    expect(content).not.toContain("No worktree");
    expect(content).not.toContain("🛠️");
    expect(
      dom.document.body.querySelector('[data-slot="popover-content"]')
        ?.className
    ).toContain("w-96");
    expect(
      dom.document.body.querySelector('[data-slot="popover-content"]')
        ?.className
    ).toContain("max-h-(--available-height)");
    const detail = [...dom.document.body.querySelectorAll("span")].find(
      (node) => node.textContent === "Implementation"
    );
    expect(detail?.classList.contains("whitespace-normal")).toBe(true);
    expect(
      dom.document.body.querySelector(
        '[data-slot="selectable-row"][data-selected="true"]'
      )
    ).not.toBeNull();
    rendered.unmount();
  });

  test("keeps primary choices visible and progressively reveals secondary session settings", async () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({ hasSession: true, provider: "grok" })}
          models={[{ id: "grok-4.6", name: "Grok 4.6", description: null }]}
          currentModel="grok-4.6"
          defaultModel="grok-4.6"
          onModel={() => {}}
          configOptions={[
            {
              id: "reasoning_effort",
              name: "Reasoning Effort",
              category: "thought_level",
              current: "xhigh",
              choices: [
                { id: "low", name: "Low Effort", description: null },
                { id: "medium", name: "Medium Effort", description: null },
                { id: "high", name: "High Effort", description: null },
                { id: "xhigh", name: "Extra High Effort", description: null },
              ],
            },
          ]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );
    const row = rendered.container.querySelector("[data-session-controls]");
    expect(row?.textContent).toContain("Develop");
    expect(row?.textContent).toContain("Grok 4.6");
    expect(row?.querySelector('button[title="Model"]')).toBeTruthy();
    expect(
      row?.querySelector('button[title="Reasoning"]')?.textContent
    ).toContain("Extra High Effort");
    expect(row?.querySelector('input[type="range"]')).toBeNull();
    expect(row?.textContent).not.toContain("Ask first");
    expect(row?.textContent).not.toContain("Memory on");
    expect(row?.textContent).not.toContain("No worktree");

    click(button(row, "Show session settings"));
    await flush();

    expect(row?.textContent).toContain("Ask first");
    expect(row?.textContent).toContain("Memory on");
    expect(row?.textContent).toContain("No worktree");
    expect(
      button(row, "Hide session settings").getAttribute("aria-expanded")
    ).toBe("true");
    rendered.unmount();
  });

  test("combines Provider and model selection into one CodeTwo picker", async () => {
    activateDom();
    const providers = [
      {
        id: "codex",
        display_name: "OpenAI Codex",
        available: true,
        enabled: true,
        models: [
          {
            id: "gpt-5.6-sol",
            name: "GPT-5.6-Sol",
            description: "Frontier coding",
          },
          {
            id: "gpt-5.6-terra",
            name: "GPT-5.6-Terra",
            description: "Balanced coding",
          },
        ],
      },
      {
        id: "grok",
        display_name: "Grok",
        available: true,
        enabled: true,
        models: [
          { id: "grok-4.6", name: "Grok 4.6", description: "Fast reasoning" },
        ],
      },
    ];
    const providerChanges = [];
    const modelChanges = [];
    const providerModelChanges = [];
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({
            hasSession: true,
            provider: "codex",
            providers,
            onProvider: (provider) => providerChanges.push(provider),
            onProviderModel: (provider, model) =>
              providerModelChanges.push([provider, model]),
          })}
          models={providers[0].models}
          currentModel="gpt-5.6-sol"
          defaultModel="gpt-5.6-sol"
          onModel={(model) => modelChanges.push(model)}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );

    const controls = rendered.container.querySelector(
      "[data-session-controls]"
    );
    expect(
      controls?.querySelector('button[aria-label^="Provider:"]')
    ).toBeNull();
    const trigger = controls?.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    expect(trigger?.textContent).toContain("GPT-5.6-Sol");
    if (!trigger)
      throw new Error("combined Provider/model trigger did not render");
    click(trigger);
    await flush();

    const picker = dom.document.body.querySelector(
      "[data-provider-model-picker]"
    );
    expect(picker).toBeTruthy();
    expect(picker?.querySelector("[data-provider-rail]")).toBeNull();
    const providerSwitcher = picker?.querySelector("[data-provider-switcher]");
    expect(providerSwitcher).toBeTruthy();
    expect(providerSwitcher?.textContent).toContain("Codex");
    expect(providerSwitcher?.textContent).not.toContain("OpenAI Codex");
    expect(providerSwitcher?.textContent).toContain("Grok");
    expect(
      picker?.querySelector('input[aria-label="Search models"]')
    ).toBeTruthy();
    const grok = picker?.querySelector<HTMLButtonElement>(
      'button[aria-label="Grok"]'
    );
    if (grok == null)
      throw new Error("Grok Provider switcher item did not render");
    click(grok);
    await flush();

    expect(providerChanges).toEqual([]);
    const grokModel = [
      ...(picker?.querySelectorAll<HTMLButtonElement>(
        '[data-model-picker-row] [data-slot="selectable-row"]'
      ) ?? []),
    ].find((row) => row.textContent?.includes("Grok 4.6"));
    if (grokModel == null)
      throw new Error("Grok model did not render after switching Provider");
    click(grokModel);
    await flush();
    expect(modelChanges).toEqual([]);
    expect(providerModelChanges).toEqual([["grok", "grok-4.6"]]);

    rendered.unmount();
  });

  test("selects an installed Provider without requiring a preset model", async () => {
    activateDom();
    const providerChanges = [];
    const providerModelChanges = [];
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({
            provider: "codex",
            providers: [
              {
                id: "codex",
                display_name: "OpenAI Codex",
                available: true,
                enabled: true,
                models: [{ id: "gpt-5.6-sol", name: "GPT-5.6-Sol" }],
              },
              {
                id: "pi",
                display_name: "Pi",
                available: true,
                enabled: true,
                models: [],
              },
            ],
            onProvider: (provider) => providerChanges.push(provider),
            onProviderModel: (provider, model) =>
              providerModelChanges.push([provider, model]),
          })}
          models={[{ id: "gpt-5.6-sol", name: "GPT-5.6-Sol" }]}
          currentModel="gpt-5.6-sol"
          defaultModel="gpt-5.6-sol"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );

    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!trigger)
      throw new Error("combined Provider/model trigger did not render");
    click(trigger);
    await flush();

    const picker = dom.document.body.querySelector(
      "[data-provider-model-picker]"
    );
    const pi = picker?.querySelector<HTMLButtonElement>(
      'button[aria-label="Pi"]'
    );
    if (pi == null) throw new Error("Pi Provider switcher item did not render");
    click(pi);
    await flush();

    expect(providerChanges).toEqual([]);
    expect(providerModelChanges).toEqual([["pi", null]]);
    expect(
      dom.document.body.querySelector("[data-provider-model-picker]")
    ).toBeNull();
    rendered.unmount();
  });

  test("does not duplicate the worktree control when the checkout bar owns it", async () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config()}
          models={[]}
          currentModel={null}
          defaultModel={null}
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          showWorktreePicker={false}
        />
      </I18nProvider>
    );

    click(button(rendered.container, "Show session settings"));
    await flush();

    expect(rendered.container.textContent).toContain("Ask first");
    expect(rendered.container.textContent).toContain("Memory on");
    expect(rendered.container.textContent).not.toContain("No worktree");
    rendered.unmount();
  });

  test("falls back to the no-scene label", () => {
    activateDom();
    const rendered = renderChip(config({ activeScene: null }));
    expect(rendered.container.textContent).toContain(
      "Keep current configuration"
    );
  });

  test("keeps Auto visible with the agent-selected scene and enables it from the menu", async () => {
    activateDom();
    const changes = [];
    const rendered = renderChip(
      config({
        autoScene: true,
        onAutoScene: (enabled) => changes.push(enabled),
      })
    );
    const trigger = rendered.container.querySelector(
      '[aria-label="Scene: Auto · Develop"]'
    );
    expect(trigger).toBeTruthy();

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

    expect(dom.document.body.textContent).toContain(
      "Agent chooses and switches scenes as the task changes"
    );
    const autoOption = [
      ...dom.document.body.querySelectorAll(
        '[data-slot="popover-content"] button'
      ),
    ].find((candidate) => candidate.textContent?.includes("Auto scene"));
    expect(autoOption).toBeTruthy();
    await reactAct(async () => {
      autoOption?.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
      );
    });
    await flush();
    expect(changes).toEqual([true]);
    rendered.unmount();
  });

  test("marks a customized scene", async () => {
    activateDom();
    const rendered = renderChip(config({ sceneCustomized: true }));
    expect(
      rendered.container.querySelector('[aria-label="Customized"]')
    ).toBeTruthy();

    const trigger = rendered.container.querySelector(
      '[aria-label="Scene: Develop"]'
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
    expect(
      dom.document.body.querySelector('[data-slot="status-badge"]')?.dataset
        .tone
    ).toBe("warning");
    rendered.unmount();
  });

  test("marks a partial soft-apply", () => {
    activateDom();
    const rendered = renderChip(
      config({ scenePendingFields: ["model", "worktree"] })
    );
    expect(
      rendered.container.querySelector('[aria-label="Partially applied"]')
    ).toBeTruthy();
  });

  test("opens the complete scene manager from the scene chip", async () => {
    activateDom();
    let opened = false;
    const rendered = renderChip(
      config({
        onManageScenes: () => {
          opened = true;
        },
      })
    );
    const trigger = rendered.container.querySelector(
      '[aria-label="Scene: Develop"]'
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
    button(dom.document.body, "Manage scenes").click();
    expect(opened).toBe(true);
    rendered.unmount();
  });

  test("keeps session settings reachable while removing every scene surface", async () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({ scenesEnabled: false })}
          models={[]}
          currentModel={null}
          defaultModel={null}
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );

    expect(rendered.container.textContent).not.toContain("Develop");
    expect(rendered.container.textContent).not.toContain("Ask first");

    click(button(rendered.container, "Show session settings"));
    await flush();

    const content = rendered.container.textContent ?? "";
    expect(content).toContain("Ask first");
    expect(content).toContain("Memory on");
    expect(content).toContain("No worktree");
    expect(content).not.toContain("Auto scene");
    expect(content).not.toContain("Manage scenes");
    rendered.unmount();
  });

  test("removes the direct memory policy control when memory is disabled", () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <SessionControls
          config={config({ memoryEnabled: false })}
          models={[]}
          currentModel={null}
          defaultModel={null}
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
        />
      </I18nProvider>
    );

    expect(rendered.container.textContent).not.toContain("Memory on");
    rendered.unmount();
  });
});

describe("SourceBadge", () => {
  test("names each source", () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <SourceBadge source="project" />
      </I18nProvider>
    );
    expect(rendered.container.textContent).toBe("Project");
  });
});

describe("ScenePicker management", () => {
  test("offers create, edit, and duplicate actions without making builtins editable", async () => {
    activateDom();
    const calls = [];
    const builtin = sceneInfo();
    const user = sceneInfo({
      reference: "user:review",
      name: "review",
      title: "Review",
      source: "user",
    });
    const rendered = mount(
      <I18nProvider>
        <ScenePicker
          scenes={[builtin, user]}
          active={null}
          auto={false}
          onAuto={() => {}}
          onScene={() => {}}
          onCreate={() => calls.push("create")}
          onEdit={(scene) => calls.push(`edit:${scene.reference}`)}
          onDuplicate={(scene) => calls.push(`duplicate:${scene.reference}`)}
          onClose={() => {}}
        />
      </I18nProvider>
    );
    await flush();

    button(dom.document.body, "New scene").click();
    button(dom.document.body, "Duplicate: Develop").click();
    button(dom.document.body, "Edit: Review").click();
    expect(calls).toEqual([
      "create",
      "duplicate:builtin:develop",
      "edit:user:review",
    ]);
    expect(() => button(dom.document.body, "Edit: Develop")).toThrow();
    rendered.unmount();
  });
});
