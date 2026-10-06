// @ts-nocheck
import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import { act as reactAct } from "react";
import { Simulate } from "react-dom/test-utils";

import {
  activateDom,
  button,
  dom,
  flush,
  mount,
  restoreDom,
  waitFor,
} from "./domTestHarness";

activateDom();
const { I18nProvider } = await import("../src/i18n");
const { environmentRegistry } = await import("../src/coreTransport");
const { RemoteEnvironmentsSettingsPage } =
  await import("../src/settings/RemoteEnvironmentsSettings");
const { EnvironmentPopover } =
  await import("../src/environment/EnvironmentPopover");

const realFetch = globalThis.fetch;

beforeEach(() => {
  activateDom();
});

const mounted: { unmount: () => void }[] = [];

/** Mount and remember the tree so a later registry change cannot re-render a dead DOM. */
function show(element) {
  const view = mount(element);
  mounted.push(view);
  return view;
}

afterEach(() => {
  for (const view of mounted.splice(0)) view.unmount();
  globalThis.fetch = realFetch;
  for (const environment of environmentRegistry.list()) {
    environmentRegistry.remove(environment.id);
  }
  dom.document.body.replaceChildren();
  restoreDom();
});

function stubPairing(status = 200) {
  const requests: { url: string; body: unknown }[] = [];
  globalThis.fetch = async (url, init) => {
    requests.push({ url: String(url), body: JSON.parse(String(init.body)) });
    return status === 200
      ? Response.json({ device_id: "device", bearer: "secret-bearer" })
      : new Response("invalid or expired pairing token", { status });
  };
  return requests;
}

function type(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(
    dom.window.HTMLInputElement.prototype,
    "value"
  ).set.call(input, value);
  Simulate.change(input);
}

async function submitPairing(container: HTMLElement, link: string) {
  const input = container.querySelector('input[aria-label="Pairing link"]');
  await reactAct(async () => type(input, link));
  await reactAct(async () => {
    Simulate.submit(container.querySelector("form"));
  });
}

describe("Remote environments settings", () => {
  test("pairs a server from its link and lists it with its address", async () => {
    const requests = stubPairing();
    const view = show(
      <I18nProvider>
        <RemoteEnvironmentsSettingsPage />
      </I18nProvider>
    );
    expect(view.container.textContent).toContain("No remote environments yet.");
    expect(view.container.textContent).toContain("codetwo-server serve");

    await submitPairing(view.container, "http://gpu-box:4599/pair#token=abc");
    await waitFor(() =>
      expect(view.container.textContent).toContain("http://gpu-box:4599")
    );

    expect(requests).toEqual([
      {
        url: "http://gpu-box:4599/api/pair",
        body: { token: "abc", device_name: "C2 Desktop" },
      },
    ]);
    expect(view.container.textContent).toContain("gpu-box:4599");
    expect(view.container.textContent).not.toContain("secret-bearer");
    // The consumed link is cleared so it is not left sitting in the field.
    expect(
      view.container.querySelector('input[aria-label="Pairing link"]').value
    ).toBe("");
  });

  test("shows why a link was refused and adds nothing", async () => {
    stubPairing(401);
    const view = show(
      <I18nProvider>
        <RemoteEnvironmentsSettingsPage />
      </I18nProvider>
    );

    await submitPairing(view.container, "http://gpu-box:4599/pair#token=old");
    await waitFor(() =>
      expect(view.container.querySelector('[role="alert"]')).not.toBeNull()
    );

    expect(
      view.container.querySelector('[role="alert"]').textContent
    ).toContain("codetwo-server pair");
    expect(environmentRegistry.list()).toEqual([]);
  });

  test("removes an environment", async () => {
    stubPairing();
    await environmentRegistry.add("http://gpu-box:4599/pair#token=abc", {
      name: "GPU box",
    });
    const view = show(
      <I18nProvider>
        <RemoteEnvironmentsSettingsPage />
      </I18nProvider>
    );
    expect(view.container.textContent).toContain("GPU box");

    await reactAct(async () => button(view.container, "Remove").click());
    await flush();

    expect(environmentRegistry.list()).toEqual([]);
    expect(view.container.textContent).toContain("No remote environments yet.");
  });
});

describe("Paired devices on a remote server", () => {
  /** A server with a mutable device list; records every authenticated call. */
  function stubServer() {
    const devices = [
      {
        id: "me",
        name: "C2 Desktop",
        protocol: "legacy",
        created_at: 1,
        last_seen: 1_790_000_000,
        current: true,
      },
      {
        id: "phone",
        name: "Pixel browser",
        protocol: "legacy",
        created_at: 2,
        last_seen: 0,
        current: false,
      },
    ];
    const calls: string[] = [];
    globalThis.fetch = async (url, init) => {
      const { pathname } = new URL(String(url));
      if (pathname === "/api/pair") {
        return Response.json({ device_id: "me", bearer: "secret-bearer" });
      }
      calls.push(
        `${init?.method} ${pathname} ${init?.headers?.Authorization ?? ""}`
      );
      if (pathname === "/api/devices") return Response.json(devices);
      const revoked = pathname.match(/^\/api\/devices\/(.+)\/revoke$/)?.[1];
      const index = devices.findIndex((device) => device.id === revoked);
      if (index < 0) return new Response("no such device", { status: 404 });
      const [removed] = devices.splice(index, 1);
      return Response.json({ revoked: true, was_current: removed.current });
    };
    return { devices, calls };
  }

  async function open() {
    const server = stubServer();
    await environmentRegistry.add("http://gpu-box:4599/pair#token=abc", {
      name: "GPU box",
    });
    const view = show(
      <I18nProvider>
        <RemoteEnvironmentsSettingsPage />
      </I18nProvider>
    );
    await reactAct(async () =>
      button(view.container, "Manage devices").click()
    );
    await waitFor(() =>
      expect(view.container.textContent).toContain("Pixel browser")
    );
    return { server, view };
  }

  test("lists the server's devices, marks this app, and never shows credentials", async () => {
    const { server, view } = await open();
    const text = view.container.textContent;

    expect(text).toContain("C2 Desktop");
    expect(text).toContain("This app");
    expect(text).toContain("Last seen never");
    expect(text).not.toContain("secret-bearer");
    expect(server.calls).toEqual(["GET /api/devices Bearer secret-bearer"]);
  });

  test("revoking another device needs a confirmation and refreshes the list", async () => {
    const { server, view } = await open();

    await reactAct(async () =>
      button(view.container, "Revoke Pixel browser").click()
    );
    // Nothing is sent until the user confirms.
    expect(server.calls).toHaveLength(1);
    await reactAct(async () => button(view.container, "Cancel").click());
    expect(server.calls).toHaveLength(1);

    await reactAct(async () =>
      button(view.container, "Revoke Pixel browser").click()
    );
    await reactAct(async () => button(view.container, "Revoke now").click());
    await waitFor(() =>
      expect(view.container.textContent).not.toContain("Pixel browser")
    );

    expect(server.calls).toContain(
      "POST /api/devices/phone/revoke Bearer secret-bearer"
    );
    expect(view.container.textContent).toContain("C2 Desktop");
    expect(environmentRegistry.list()).toHaveLength(1);
  });

  test("revoking this app's own credential warns first and forgets the environment", async () => {
    const { view } = await open();

    await reactAct(async () =>
      button(view.container, "Revoke C2 Desktop").click()
    );
    expect(view.container.textContent).toContain(
      "disconnects and removes this environment"
    );
    await reactAct(async () => button(view.container, "Revoke now").click());
    await waitFor(() => expect(environmentRegistry.list()).toEqual([]));
    await flush();

    expect(view.container.textContent).toContain("No remote environments yet.");
  });

  test("a rejected credential explains how to recover", async () => {
    stubServer();
    await environmentRegistry.add("http://gpu-box:4599/pair#token=abc", {
      name: "GPU box",
    });
    globalThis.fetch = async () =>
      new Response("invalid bearer", { status: 401 });
    const view = show(
      <I18nProvider>
        <RemoteEnvironmentsSettingsPage />
      </I18nProvider>
    );
    await reactAct(async () =>
      button(view.container, "Manage devices").click()
    );
    await waitFor(() =>
      expect(view.container.querySelector('[role="alert"]')).not.toBeNull()
    );
    expect(
      view.container.querySelector('[role="alert"]').textContent
    ).toContain("pair again");
  });
});

describe("Environment popover with remote environments", () => {
  async function openPopover(view: ReturnType<typeof mount>) {
    const trigger = view.container.querySelector(
      '[aria-label="Project environment"]'
    );
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
    return trigger;
  }

  const popover = (
    <I18nProvider>
      <EnvironmentPopover
        project="mini-game"
        projectPath="/tmp/mini-game"
        projects={[]}
        git={null}
        diffStat={{ added: 0, deleted: 0 }}
        onRefresh={() => {}}
        onSelectProject={() => {}}
        onAddProject={() => {}}
        onOpenSourceControl={() => {}}
        onOpenSettings={() => {}}
      />
    </I18nProvider>
  );

  test("offers no environment choice until a server is paired", async () => {
    const view = show(popover);
    await openPopover(view);
    expect(
      dom.document.body.querySelector('[data-slot="popover-content"]')
        .textContent
    ).not.toContain("New sessions run on");
  });

  test("chooses where new sessions run and names it on the trigger", async () => {
    stubPairing();
    const environment = await environmentRegistry.add(
      "http://gpu-box:4599/pair#token=abc",
      { name: "GPU box" }
    );
    const view = show(popover);
    const trigger = await openPopover(view);
    const content = dom.document.body.querySelector(
      '[data-slot="popover-content"]'
    );

    expect(content.textContent).toContain("New sessions run on");
    expect(content.textContent).toContain("http://gpu-box:4599");
    expect(environmentRegistry.activeId()).toBeNull();
    expect(trigger.textContent).toContain("Environment");

    // The row reads "<name><address>"; pick it by its name.
    const row = [...content.querySelectorAll("button")].find((candidate) =>
      candidate.textContent.startsWith("GPU box")
    );
    expect(row).toBeTruthy();
    await reactAct(async () => row.click());
    await flush();

    expect(environmentRegistry.activeId()).toBe(environment.id);
    expect(trigger.textContent).toContain("GPU box");
  });
});
