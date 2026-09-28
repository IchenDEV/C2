// @ts-nocheck
import { afterEach, expect, test } from "bun:test";

import { useEffect, useState } from "react";

import {
  activateDom,
  button,
  click,
  dom,
  flush,
  mount,
} from "./domTestHarness";

activateDom();
const { Composer, SessionControls } = await import("../src/session/Composer");
const { I18nProvider } = await import("../src/i18n");
afterEach(() => dom.document.body.replaceChildren());

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

const noop = () => {};
const pickerProps = {
  models: [{ id: "model", name: "Test model" }],
  currentModel: "model",
  defaultModel: "model",
  onModel: noop,
  configOptions: [],
  onConfigOption: noop,
};

function frame(children, overrides = {}) {
  return (
    <I18nProvider>
      <Composer
        config={config()}
        {...pickerProps}
        checkout={{
          project: "codeTwo",
          branch: "main",
          dirty: 0,
          onOpen: noop,
        }}
        docMode={false}
        onDocMode={noop}
        boundsRef={{ current: null }}
        contextWindow={null}
        running={false}
        loading={false}
        docEmpty={false}
        onRun={noop}
        onQueue={noop}
        onMultitask={noop}
        onSteer={noop}
        onStop={noop}
        steeringSupported={true}
        onAttachFile={noop}
        onAttachImages={noop}
        onInsertSkill={noop}
        onInsertIssue={noop}
        onOpenMarket={noop}
        onNewSkill={noop}
        canvasEnabled={false}
        onInsertCanvas={noop}
        voiceEnabled={false}
        onVoiceText={noop}
        runHint="⌘Enter"
        skillHint=""
        filesHint=""
        {...overrides}
      >
        {children}
      </Composer>
    </I18nProvider>
  );
}

test("keeps the same draft editor through expanded mode and places checkout after the card", async () => {
  activateDom();
  let mounts = 0;
  function Draft() {
    const [text, setText] = useState("Draft remains intact");
    useEffect(() => {
      mounts += 1;
    }, []);
    return <button onClick={() => setText("Edited draft")}>{text}</button>;
  }
  const rendered = mount(frame(<Draft />));
  try {
    const card = rendered.container.querySelector(".composer-card");
    expect(card?.nextElementSibling?.hasAttribute("data-checkout-bar")).toBe(
      true
    );
    click(button(rendered.container, "Draft remains intact"));
    await flush();
    const editor = button(rendered.container, "Edited draft");
    rendered.rerender(frame(<Draft />, { docMode: true }));
    expect(button(rendered.container, "Edited draft")).toBe(editor);
    expect(rendered.container.querySelector("[data-checkout-bar]")).toBeNull();
    rendered.rerender(frame(<Draft />));
    expect(button(rendered.container, "Edited draft")).toBe(editor);
    expect(mounts).toBe(1);
  } finally {
    rendered.unmount();
  }
});

test("permission is directly reachable, remains disabled while changing, and is not duplicated in secondary settings", async () => {
  activateDom();
  const rendered = mount(
    <I18nProvider>
      <SessionControls
        {...pickerProps}
        config={config({ modeChangeDisabled: true })}
        showWorktreePicker={false}
      />
    </I18nProvider>
  );
  try {
    const modes = () =>
      rendered.container.querySelectorAll(
        'button[aria-label="Mode: Ask first"]'
      );
    expect(modes()).toHaveLength(1);
    expect(modes()[0].disabled).toBe(true);
    click(button(rendered.container, "Show session settings"));
    await flush();
    expect(modes()).toHaveLength(1);
    expect(
      rendered.container.querySelector("[data-session-options]")?.textContent
    ).toContain("Codex default");
  } finally {
    rendered.unmount();
  }
});

test("right-side actions preserve send, loading and running behavior", () => {
  activateDom();
  let sent = 0;
  let stopped = 0;
  const rendered = mount(frame(<p>Prompt</p>, { onRun: () => sent++ }));
  try {
    const actions = rendered.container.querySelector("[data-composer-actions]");
    expect(button(actions, "Add to the document")).toBeTruthy();
    click(button(actions, "Run this document"));
    expect(sent).toBe(1);
    rendered.rerender(frame(<p>Prompt</p>, { loading: true }));
    expect(actions?.querySelector("button[disabled]")).toBeTruthy();
    rendered.rerender(
      frame(<p>Prompt</p>, { running: true, onStop: () => stopped++ })
    );
    const stop = actions?.querySelector('button[aria-label="Stop this turn"]');
    expect(stop).toBeTruthy();
    click(stop);
    expect(stopped).toBe(1);
  } finally {
    rendered.unmount();
  }
});
