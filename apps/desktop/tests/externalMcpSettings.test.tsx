// @ts-nocheck
import { describe, expect, test } from "bun:test";

import { fireEvent, waitFor } from "@testing-library/react";

import { activateDom, flush, mount, restoreDom } from "./domTestHarness";

activateDom();
const { ExternalMcpSettingsPage, ExternalMcpSettingsNarrowPreview } =
  await import("../src/settings/ExternalMcpSettings");
const { I18nProvider } = await import("../src/i18n");

const TOKEN = "ctmcp_test_token_once_only";

function baseStatus(enabled = true) {
  return {
    enabled,
    endpoint: "http://127.0.0.1:4599/external-mcp",
    data_dir_configured: true,
    client_count: 0,
    active_client_count: 0,
  };
}

describe("External MCP settings panel", () => {
  test("defaults, create flow shows token once, revoke confirmation, warnings, error, themes", async () => {
    activateDom();
    let clients: any[] = [];
    let createdTokenShown = false;
    let createCalls = 0;
    const bridge = {
      statusLoader: async () => baseStatus(true),
      clientsLoader: async () => clients,
      enabledSaver: async (enabled: boolean) => baseStatus(enabled),
      clientCreator: async () => {
        createCalls += 1;
        const result = {
          record: {
            id: "client-1",
            name: "ci-bot",
            scopes: ["read", "operate", "approve"],
            created_at: "2026-01-01T00:00:00Z",
            expires_at: "2026-02-01T00:00:00Z",
            status: "active",
            expires_never: false,
            projects_label: "all",
          },
          token: TOKEN,
          config_snippet: {
            mcpServers: {
              codetwo: {
                url: "http://127.0.0.1:4599/external-mcp",
                headers: { Authorization: "Bearer <paste-your-token-here>" },
              },
            },
          },
          expires_never: false,
        };
        createdTokenShown = true;
        clients = [result.record];
        return result;
      },
      clientRevoker: async (id: string) => {
        clients = clients.map((client) =>
          client.id === id ? { ...client, status: "revoked" } : client
        );
      },
    };

    const rendered = mount(
      <I18nProvider>
        <ExternalMcpSettingsPage {...bridge} initialClientName="ci-bot" />
      </I18nProvider>
    );

    await waitFor(() => {
      expect(
        rendered.container.querySelector("[data-external-mcp-enabled]")
      ).toBeTruthy();
    });

    fireEvent.click(
      rendered.container.querySelector("[data-external-mcp-create]")!
    );
    await waitFor(() => {
      expect(
        document.querySelector('[data-external-mcp-scope="read"]')
      ).toBeTruthy();
    });
    expect(
      document.querySelector('[data-external-mcp-scope="approve"]')
    ).toBeTruthy();

    expect(
      document.querySelector("[data-external-mcp-admin-hint]")
    ).toBeTruthy();
    fireEvent.click(
      document.querySelector('[data-external-mcp-scope="admin"]')!
    );
    expect(
      document.querySelector("[data-external-mcp-admin-warning]")
    ).toBeTruthy();

    fireEvent.click(
      document.querySelector('[data-external-mcp-ttl="permanent"]')!
    );
    expect(
      document.querySelector("[data-external-mcp-permanent-warning]")
    ).toBeTruthy();
    fireEvent.click(
      document.querySelector('[data-external-mcp-ttl="default"]')!
    );

    fireEvent.click(document.querySelector("[data-external-mcp-submit]")!);
    await flush();

    await waitFor(
      async () => {
        await flush();
        expect(createCalls).toBeGreaterThan(0);
        expect(
          document.querySelector("[data-external-mcp-token]")
        ).toBeTruthy();
      },
      { timeout: 3000 }
    );
    expect(
      document.querySelector("[data-external-mcp-token]")?.getAttribute("value")
    ).toBe(TOKEN);
    const snippet = document.body.textContent ?? "";
    expect(snippet).not.toContain(TOKEN);
    expect(snippet).toContain("<paste-your-token-here>");

    fireEvent.click(document.querySelector("[data-external-mcp-done]")!);
    await waitFor(() => {
      expect(document.querySelector("[data-external-mcp-token]")).toBeNull();
    });
    expect(createdTokenShown).toBe(true);

    fireEvent.click(
      rendered.container.querySelector('[data-external-mcp-revoke="client-1"]')!
    );
    expect(
      document.querySelector("[data-external-mcp-revoke-dialog]")
    ).toBeTruthy();
    fireEvent.click(
      document.querySelector("[data-external-mcp-revoke-confirm]")!
    );

    await waitFor(() => {
      expect(rendered.container.textContent).toContain("revoked");
    });

    restoreDom();
  });

  test("error state renders message", async () => {
    activateDom();
    const rendered = mount(
      <I18nProvider>
        <ExternalMcpSettingsPage
          statusLoader={async () => {
            throw new Error("boom");
          }}
          clientsLoader={async () => []}
        />
      </I18nProvider>
    );
    await waitFor(() => {
      expect(
        rendered.container.querySelector("[data-external-mcp-error]")
      ).toBeTruthy();
    });
    restoreDom();
  });

  test("light, dark, and narrow containers render", async () => {
    activateDom();
    const light = mount(
      <I18nProvider>
        <div className="light">
          <ExternalMcpSettingsPage
            statusLoader={async () => baseStatus()}
            clientsLoader={async () => []}
          />
        </div>
      </I18nProvider>
    );
    await waitFor(() => {
      expect(
        light.container.querySelector("[data-external-mcp-enabled]")
      ).toBeTruthy();
    });
    light.unmount();

    const dark = mount(
      <I18nProvider>
        <div className="dark">
          <ExternalMcpSettingsPage
            statusLoader={async () => baseStatus()}
            clientsLoader={async () => []}
          />
        </div>
      </I18nProvider>
    );
    await waitFor(() => {
      expect(
        dark.container.querySelector("[data-external-mcp-enabled]")
      ).toBeTruthy();
    });
    dark.unmount();

    const narrow = mount(
      <I18nProvider>
        <ExternalMcpSettingsNarrowPreview themeClass="dark" />
      </I18nProvider>
    );
    expect(
      narrow.container.querySelector("[data-external-mcp-narrow]")
    ).toBeTruthy();
    narrow.unmount();
    restoreDom();
  });
});
