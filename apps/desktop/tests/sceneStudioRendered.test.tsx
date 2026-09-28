// @ts-nocheck
import { afterEach, describe, expect, test } from "bun:test";

import { act as reactAct } from "react";

import { activateDom, button, dom, mount, restoreDom } from "./domTestHarness";

activateDom();
const { SceneStudio } = await import("../src/session/SceneStudio.tsx");
const { I18nProvider } = await import("../src/i18n");

afterEach(() => {
  dom.document.body.replaceChildren();
  restoreDom();
});

function scene(overrides = {}) {
  return {
    reference: "builtin:develop",
    name: "develop",
    title: "Develop",
    description: "Implementation",
    icon: null,
    source: "builtin",
    plugin_id: null,
    keywords: [],
    has_brief: true,
    localizations: {},
    execution: { session_mode: "auto_edit" },
    brief: null,
    artifacts: [],
    skills: null,
    exit: null,
    ...overrides,
  };
}

function renderStudio(props = {}) {
  return mount(
    <I18nProvider>
      <SceneStudio
        scenes={[scene()]}
        active={scene()}
        request={null}
        providers={[]}
        skills={[]}
        cwd="/tmp/project"
        onRequest={() => {}}
        onScene={() => {}}
        onSaved={() => {}}
        onDeleted={() => {}}
        onClose={() => {}}
        {...props}
      />
    </I18nProvider>
  );
}

describe("SceneStudio rendered", () => {
  test("is a first-class page with a scene library and no editor dialog", () => {
    activateDom();
    const rendered = renderStudio();

    expect(
      rendered.container.querySelector('[data-page="scene-studio"]')
    ).toBeTruthy();
    expect(rendered.container.textContent).toContain("Scenes");
    expect(rendered.container.textContent).toContain("Provided scenes");
    expect(rendered.container.textContent).toContain("Develop");
    expect(
      dom.document.body.querySelector('[data-slot="dialog-content"]')
    ).toBeNull();
    rendered.unmount();
  });

  test("opens creation inside the page flow", async () => {
    activateDom();
    let request = null;
    const rendered = renderStudio({
      onRequest: (next) => {
        request = next;
      },
    });

    await reactAct(async () => {
      button(rendered.container, "New scene").click();
    });

    expect(request).toEqual({ kind: "create" });
    rendered.unmount();
  });
});
