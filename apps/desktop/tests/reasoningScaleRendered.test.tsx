// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

import {
  activateDom,
  click,
  dom,
  flush,
  mount,
  restoreDom,
} from "./domTestHarness";

activateDom();
const { ModelPicker } = await import("../src/session/Composer");
const { I18nProvider } = await import("../src/i18n");
const { hiddenModelsForProvider, setModelHidden } =
  await import("../src/session/modelPreferences");

afterEach(() => {
  dom.document.body.replaceChildren();
  dom.localStorage.clear();
  restoreDom();
});

describe("ModelPicker", () => {
  test("uses separate model and reasoning menus and forwards the provider-owned choice", async () => {
    activateDom();
    const selected: [string, string][] = [];
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[{ id: "grok-4.6", name: "Grok 4.6", description: null }]}
          current="grok-4.6"
          defaultModel="grok-4.6"
          provider="grok"
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
          onConfigOption={(configId, value) => selected.push([configId, value])}
          hasSession
        />
      </I18nProvider>
    );

    const modelTrigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    const effortTrigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Reasoning"]'
    );
    expect(modelTrigger?.textContent).toContain("Grok 4.6");
    expect(effortTrigger?.textContent).toContain("Extra High Effort");
    expect(rendered.container.querySelector('input[type="range"]')).toBeNull();

    if (!effortTrigger) throw new Error("reasoning trigger did not render");
    click(effortTrigger);
    await flush();
    const low = [
      ...dom.document.body.querySelectorAll<HTMLButtonElement>(
        '[data-slot="popover-content"] button'
      ),
    ].find((button) => button.textContent?.includes("Low Effort"));
    if (low == null) throw new Error("low effort option did not render");
    click(low);
    await flush();
    expect(selected).toEqual([["reasoning_effort", "low"]]);
    rendered.unmount();
  });

  test("shows provider model choices before the first session is created", () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[{ id: "grok-4.6", name: "Grok 4.6", description: null }]}
          current={null}
          defaultModel={null}
          provider="grok"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession={false}
        />
      </I18nProvider>
    );

    const trigger = rendered.container.querySelector('button[title="Model"]');
    expect(trigger?.textContent).toContain("Default model");
    rendered.unmount();
  });

  test("keeps model selection hidden for providers without an advertised model list", () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[]}
          current={null}
          defaultModel={null}
          provider="opencode"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession={false}
        />
      </I18nProvider>
    );

    expect(
      rendered.container.querySelector('button[title="Model"]')
    ).toBeNull();
    rendered.unmount();
  });

  test("locks the model selector while the current turn is running", () => {
    activateDom();
    const selected: string[] = [];
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[
            { id: "fast", name: "Fast", description: null },
            { id: "deep", name: "Deep", description: null },
          ]}
          current="fast"
          defaultModel="fast"
          provider="grok"
          onModel={(model) => selected.push(model)}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession
          disabled
        />
      </I18nProvider>
    );

    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    expect(trigger?.disabled).toBe(true);
    trigger?.click();
    expect(selected).toEqual([]);
    rendered.unmount();
  });

  test("keeps a long model list inside the available popover height", async () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={Array.from({ length: 40 }, (_, index) => ({
            id: `model-${index}`,
            name: `Model ${index}`,
            description: null,
          }))}
          current={null}
          defaultModel={null}
          provider="opencode"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession={false}
        />
      </I18nProvider>
    );

    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!trigger) throw new Error("model trigger did not render");
    click(trigger);
    await flush();

    const popup = dom.document.body.querySelector(
      '[data-slot="popover-content"]'
    );
    const modelList = popup?.querySelector("[data-model-picker-list]");
    expect(popup?.className).toContain("max-h-(--available-height)");
    expect(popup?.className).toContain("overflow-hidden");
    expect(modelList?.className).toContain("min-h-0");
    expect(modelList?.className).toContain("flex-1");
    expect(modelList?.className).toContain("max-h-80");
    expect(modelList?.className).toContain("overflow-y-auto");
    rendered.unmount();
  });

  test("favorites flat model families without selecting and keeps them first per provider", async () => {
    activateDom();
    dom.localStorage.clear();
    const selected: string[] = [];
    const models = [
      { id: "alpha-low", name: "Alpha (Low)", description: null },
      { id: "alpha-high", name: "Alpha (High)", description: null },
      { id: "beta-low", name: "Beta (Low)", description: null },
      { id: "beta-high", name: "Beta (High)", description: null },
    ];
    const renderPicker = (provider: string) =>
      mount(
        <I18nProvider>
          <ModelPicker
            models={models}
            current="alpha-high"
            defaultModel="alpha-high"
            provider={provider}
            onModel={(model) => selected.push(model)}
            configOptions={[]}
            onConfigOption={() => {}}
            hasSession
          />
        </I18nProvider>
      );

    let rendered = renderPicker("opencode");
    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!trigger) throw new Error("model trigger did not render");
    click(trigger);
    await flush();

    const add = dom.document.body.querySelector<HTMLButtonElement>(
      'button[aria-label="Add Beta to favorites"]'
    );
    if (add == null) throw new Error("favorite action did not render");
    click(add);
    await flush();

    expect(selected).toEqual([]);
    expect(
      dom.document.body.querySelector('[data-slot="popover-content"]')
    ).not.toBeNull();
    expect(
      dom.document.body
        .querySelector('button[aria-label="Remove Beta from favorites"]')
        ?.getAttribute("aria-pressed")
    ).toBe("true");
    expect(
      [
        ...dom.document.body.querySelectorAll(
          '[data-model-picker-row] [data-slot="selectable-row-label"]'
        ),
      ].map((label) => label.textContent)
    ).toEqual(["Beta", "Alpha"]);
    expect(
      [
        ...dom.document.body.querySelectorAll(
          '[data-model-picker-row] [data-slot="selectable-row-label"]'
        ),
      ].filter((label) => label.textContent === "Beta")
    ).toHaveLength(1);

    rendered.unmount();
    rendered = renderPicker("opencode");
    const reopened = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!reopened) throw new Error("remounted model trigger did not render");
    click(reopened);
    await flush();
    expect(
      dom.document.body
        .querySelector('button[aria-label="Remove Beta from favorites"]')
        ?.getAttribute("aria-pressed")
    ).toBe("true");
    rendered.unmount();

    rendered = renderPicker("cursor");
    const otherProvider = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!otherProvider)
      throw new Error("other provider model trigger did not render");
    click(otherProvider);
    await flush();
    expect(
      dom.document.body.querySelector(
        'button[aria-label="Add Beta to favorites"]'
      )
    ).not.toBeNull();
    expect(
      dom.document.body.querySelector(
        'button[aria-label="Remove Beta from favorites"]'
      )
    ).toBeNull();
    rendered.unmount();
  });

  test("favorites provider-owned model options and preserves their selection path", async () => {
    activateDom();
    dom.localStorage.clear();
    const selected: [string, string][] = [];
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[]}
          current="alpha"
          defaultModel="alpha"
          provider="claude_code"
          onModel={() => {}}
          configOptions={[
            {
              id: "model",
              name: "Model",
              category: "model",
              current: "alpha",
              choices: [
                { id: "alpha", name: "Alpha", description: null },
                { id: "beta", name: "Beta", description: null },
              ],
            },
          ]}
          onConfigOption={(configId, value) => selected.push([configId, value])}
          hasSession
        />
      </I18nProvider>
    );

    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!trigger) throw new Error("config-option model trigger did not render");
    click(trigger);
    await flush();
    const favorite = dom.document.body.querySelector<HTMLButtonElement>(
      'button[aria-label="Add Beta to favorites"]'
    );
    if (favorite == null)
      throw new Error("config-option favorite action did not render");
    click(favorite);
    await flush();
    expect(selected).toEqual([]);

    const beta = [
      ...dom.document.body.querySelectorAll<HTMLButtonElement>(
        '[data-model-picker-row] [data-slot="selectable-row"]'
      ),
    ].find((button) => button.getAttribute("aria-label") === "Beta");
    if (beta == null) throw new Error("favorite model row did not render");
    click(beta);
    await flush();
    expect(selected).toEqual([["model", "beta"]]);
    rendered.unmount();
  });

  test("localizes favorite actions in Chinese", async () => {
    activateDom();
    dom.localStorage.clear();
    dom.localStorage.setItem("codetwo.language", "zh-CN");
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[{ id: "model-a", name: "模型甲", description: null }]}
          current={null}
          defaultModel={null}
          provider="opencode"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession={false}
        />
      </I18nProvider>
    );
    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="模型"]'
    );
    if (!trigger) throw new Error("localized model trigger did not render");
    click(trigger);
    await flush();
    expect(
      dom.document.body.querySelector('button[aria-label="收藏模型“模型甲”"]')
    ).not.toBeNull();
    rendered.unmount();
  });

  test("renders search, hides provider preferences, and preserves the selected model", async () => {
    activateDom();
    setModelHidden("opencode", "alpha", true);
    setModelHidden("opencode", "beta", true);
    expect(hiddenModelsForProvider("opencode")).toEqual(["alpha", "beta"]);
    const rendered = mount(
      <I18nProvider>
        <ModelPicker
          models={[
            { id: "alpha", name: "Alpha", description: "Current" },
            { id: "beta", name: "Beta", description: "Hidden" },
            { id: "gamma", name: "Gamma", description: "Fast" },
          ]}
          current="alpha"
          defaultModel="alpha"
          provider="opencode"
          onModel={() => {}}
          configOptions={[]}
          onConfigOption={() => {}}
          hasSession
        />
      </I18nProvider>
    );
    const trigger = rendered.container.querySelector<HTMLButtonElement>(
      'button[title="Model"]'
    );
    if (!trigger) throw new Error("model trigger did not render");
    expect(hiddenModelsForProvider("opencode")).toEqual(["alpha", "beta"]);
    click(trigger);
    await flush();

    const labels = () =>
      [
        ...dom.document.body.querySelectorAll(
          '[data-model-picker-row] [data-slot="selectable-row-label"]'
        ),
      ].map((label) => label.textContent);
    expect(labels()).toEqual(["Alpha", "Gamma"]);

    const search = dom.document.body.querySelector<HTMLInputElement>(
      'input[aria-label="Search models"]'
    );
    if (search == null) throw new Error("model search did not render");
    expect(search.getAttribute("placeholder")).toBe("Search models");
    rendered.unmount();
  });
});

test("shows pending switching state without inventing provider choices", async () => {
  activateDom();
  const rendered = mount(
    <I18nProvider>
      <ModelPicker
        models={[]}
        current={null}
        defaultModel={null}
        provider="custom-agent"
        onModel={() => {}}
        configOptions={[]}
        onConfigOption={() => {}}
        hasSession
        disabled
        providerConfig={{
          providers: [],
          providersStatus: "loading",
          provider: "custom-agent",
          providerSwitching: true,
        }}
      />
    </I18nProvider>
  );
  const trigger = rendered.container.querySelector('button[title="Model"]');
  expect(trigger?.disabled).toBe(true);
  expect(rendered.container.querySelector('[role="status"]')?.textContent).toBe(
    "Switching agent…"
  );
  rendered.unmount();
});
