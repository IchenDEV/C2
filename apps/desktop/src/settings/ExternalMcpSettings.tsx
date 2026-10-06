import { useCallback, useEffect, useMemo, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

import type {
  ExternalMcpClient,
  ExternalMcpCreateResult,
  ExternalMcpScope,
  ExternalMcpStatus,
  ExternalMcpTtlKind,
} from "../bridge";
import {
  createExternalMcpClient,
  externalMcpClientsList,
  externalMcpSetEnabled,
  externalMcpStatus,
  revokeExternalMcpClient,
} from "../bridge";
import { useT } from "../i18n";
import { GroupHeading, Page, Row } from "./SettingsPrimitives";

const DEFAULT_SCOPES: ExternalMcpScope[] = ["read", "operate", "approve"];

interface PanelProps {
  statusLoader?: typeof externalMcpStatus;
  enabledSaver?: typeof externalMcpSetEnabled;
  clientsLoader?: typeof externalMcpClientsList;
  clientCreator?: typeof createExternalMcpClient;
  clientRevoker?: typeof revokeExternalMcpClient;
  initialClientName?: string;
}

function formatTime(iso: string | null | undefined, locale: string): string {
  if (iso == null || iso === "") return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(locale === "zh-CN" ? "zh-CN" : undefined);
}

export function ExternalMcpSettingsPage({
  statusLoader = externalMcpStatus,
  enabledSaver = externalMcpSetEnabled,
  clientsLoader = externalMcpClientsList,
  clientCreator = createExternalMcpClient,
  clientRevoker = revokeExternalMcpClient,
  initialClientName = "",
}: PanelProps) {
  const t = useT();
  const [status, setStatus] = useState<ExternalMcpStatus | null>(null);
  const [clients, setClients] = useState<ExternalMcpClient[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savingEnabled, setSavingEnabled] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [name, setName] = useState(initialClientName);
  const [scopes, setScopes] = useState<ExternalMcpScope[]>([...DEFAULT_SCOPES]);
  const [ttlKind, setTtlKind] = useState<ExternalMcpTtlKind>("default");
  const [created, setCreated] = useState<ExternalMcpCreateResult | null>(null);
  const [copied, setCopied] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<ExternalMcpClient | null>(
    null
  );
  const [revokeBusy, setRevokeBusy] = useState(false);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [nextStatus, nextClients] = await Promise.all([
        statusLoader(),
        clientsLoader(),
      ]);
      setStatus(nextStatus);
      setClients(nextClients);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [clientsLoader, statusLoader]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function toggleEnabled(enabled: boolean) {
    setSavingEnabled(true);
    setError(null);
    try {
      setStatus(await enabledSaver(enabled));
      if (enabled) await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setSavingEnabled(false);
    }
  }

  function resetCreateForm() {
    setName(initialClientName);
    setScopes([...DEFAULT_SCOPES]);
    setTtlKind("default");
    setCreateError(null);
    setCreated(null);
    setCopied(false);
  }

  async function submitCreate() {
    if (name.trim() === "") {
      setCreateError(t("externalMcp.clientName"));
      return;
    }
    setCreateBusy(true);
    setCreateError(null);
    try {
      const result = await clientCreator({
        name: name.trim(),
        scopes,
        projects: "all",
        ttl:
          ttlKind === "default"
            ? { kind: "default" }
            : ttlKind === "permanent"
              ? { kind: "permanent" }
              : {
                  kind: "days",
                  days:
                    ttlKind === "days90"
                      ? 90
                      : ttlKind === "days365"
                        ? 365
                        : 30,
                },
      });
      setCreated(result);
      await refresh();
    } catch (err) {
      setCreateError(String(err));
    } finally {
      setCreateBusy(false);
    }
  }

  async function confirmRevoke() {
    if (!revokeTarget) return;
    setRevokeBusy(true);
    setError(null);
    try {
      await clientRevoker(revokeTarget.id);
      setRevokeTarget(null);
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setRevokeBusy(false);
    }
  }

  const scopeLabels = useMemo(
    () => ({
      read: t("externalMcp.scope.read"),
      operate: t("externalMcp.scope.operate"),
      approve: t("externalMcp.scope.approve"),
      admin: t("externalMcp.scope.admin"),
    }),
    [t]
  );

  const showAdminWarning = scopes.includes("admin");
  const showPermanentWarning = ttlKind === "permanent";

  return (
    <Page
      title={t("externalMcp.title")}
      description={t("externalMcp.description")}
    >
      {loading ? (
        <div className="flex items-center gap-2 py-4" data-external-mcp-loading>
          <Spinner className="size-4" />
          <span className="text-metadata text-muted-foreground">
            {t("externalMcp.loading")}
          </span>
        </div>
      ) : null}

      {error != null && error !== "" ? (
        <p
          className="text-metadata text-destructive pb-2"
          data-external-mcp-error
        >
          {error}
        </p>
      ) : null}

      {!loading && status ? (
        <>
          <Row
            label={t("externalMcp.enable")}
            hint={t("externalMcp.enableHint")}
          >
            <Switch
              data-external-mcp-enabled
              aria-label={t("externalMcp.enable")}
              checked={status.enabled}
              disabled={savingEnabled}
              onCheckedChange={(enabled) => void toggleEnabled(enabled)}
            />
          </Row>
          <Row
            label={t("externalMcp.endpoint")}
            hint={t("externalMcp.endpointHint")}
          >
            <code className="text-metadata break-all">
              {status.endpoint ?? t("externalMcp.endpointUnknown")}
            </code>
          </Row>
          <Row
            label={t("externalMcp.clients")}
            hint={t("externalMcp.clientsHint")}
          >
            <Button
              size="sm"
              data-external-mcp-create
              disabled={!status.enabled}
              onClick={() => {
                resetCreateForm();
                setCreateOpen(true);
              }}
            >
              {t("externalMcp.createClient")}
            </Button>
          </Row>

          <GroupHeading>{t("externalMcp.clientTable")}</GroupHeading>
          {clients.length === 0 ? (
            <p
              className="text-metadata text-muted-foreground"
              data-external-mcp-empty
            >
              {t("externalMcp.empty")}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <table
                className="text-metadata w-full min-w-[640px] border-collapse"
                data-external-mcp-table
              >
                <thead>
                  <tr className="border-b text-left">
                    <th className="py-2 pr-2">{t("externalMcp.col.name")}</th>
                    <th className="py-2 pr-2">{t("externalMcp.col.scopes")}</th>
                    <th className="py-2 pr-2">
                      {t("externalMcp.col.projects")}
                    </th>
                    <th className="py-2 pr-2">
                      {t("externalMcp.col.created")}
                    </th>
                    <th className="py-2 pr-2">
                      {t("externalMcp.col.expires")}
                    </th>
                    <th className="py-2 pr-2">
                      {t("externalMcp.col.lastUsed")}
                    </th>
                    <th className="py-2 pr-2">{t("externalMcp.col.status")}</th>
                    <th className="py-2" />
                  </tr>
                </thead>
                <tbody>
                  {clients.map((client) => (
                    <tr
                      key={client.id}
                      className="border-b"
                      data-external-mcp-row={client.id}
                    >
                      <td className="py-2 pr-2">{client.name}</td>
                      <td className="py-2 pr-2">
                        {client.scopes
                          .map(
                            (scope) =>
                              scopeLabels[scope as ExternalMcpScope] ?? scope
                          )
                          .join(", ")}
                      </td>
                      <td className="py-2 pr-2">{client.projects_label}</td>
                      <td className="py-2 pr-2">
                        {formatTime(client.created_at, "en")}
                      </td>
                      <td className="py-2 pr-2">
                        {client.expires_never
                          ? t("externalMcp.never")
                          : formatTime(client.expires_at, "en")}
                      </td>
                      <td className="py-2 pr-2">
                        {formatTime(client.last_used_at, "en")}
                      </td>
                      <td className="py-2 pr-2">{client.status}</td>
                      <td className="py-2">
                        <Button
                          variant="outline"
                          size="sm"
                          data-external-mcp-revoke={client.id}
                          disabled={
                            client.status === "revoked" || !status.enabled
                          }
                          onClick={() => setRevokeTarget(client)}
                        >
                          {t("externalMcp.revoke")}
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      ) : null}

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="max-w-lg" data-external-mcp-create-dialog>
          <DialogHeader>
            <DialogTitle>{t("externalMcp.createClient")}</DialogTitle>
          </DialogHeader>
          {created ? (
            <div className="space-y-3">
              <p className="text-metadata">{t("externalMcp.tokenOnce")}</p>
              <Input
                readOnly
                data-external-mcp-token
                aria-label={t("externalMcp.tokenLabel")}
                value={created.token}
              />
              <Button
                size="sm"
                data-external-mcp-copy-token
                onClick={() => {
                  void navigator.clipboard.writeText(created.token);
                  setCopied(true);
                }}
              >
                {copied ? t("externalMcp.copied") : t("externalMcp.copyToken")}
              </Button>
              <pre className="text-metadata bg-muted rounded-control overflow-x-auto p-2">
                {JSON.stringify(created.config_snippet, null, 2)}
              </pre>
              <DialogFooter>
                <Button
                  data-external-mcp-done
                  onClick={() => {
                    setCreateOpen(false);
                    setCreated(null);
                  }}
                >
                  {t("externalMcp.done")}
                </Button>
              </DialogFooter>
            </div>
          ) : (
            <div className="space-y-4">
              <label className="block space-y-1">
                <span className="text-metadata font-medium">
                  {t("externalMcp.clientName")}
                </span>
                <Input
                  data-external-mcp-name
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              </label>
              <fieldset className="space-y-2">
                <legend className="text-metadata font-medium">
                  {t("externalMcp.scopes")}
                </legend>
                {(["read", "operate", "approve", "admin"] as const).map(
                  (scope) => (
                    <label key={scope} className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        data-external-mcp-scope={scope}
                        checked={scopes.includes(scope)}
                        onChange={(event) => {
                          const checked = event.target.checked;
                          setScopes((current) =>
                            checked
                              ? [...current, scope]
                              : current.filter((item) => item !== scope)
                          );
                        }}
                      />
                      <span>{scopeLabels[scope]}</span>
                    </label>
                  )
                )}
                <p
                  className="text-metadata text-muted-foreground"
                  data-external-mcp-admin-hint
                >
                  {t("externalMcp.adminHint")}
                </p>
                {showAdminWarning ? (
                  <p
                    className="text-metadata text-destructive"
                    data-external-mcp-admin-warning
                  >
                    {t("externalMcp.adminWarning")}
                  </p>
                ) : null}
              </fieldset>
              <fieldset className="space-y-2">
                <legend className="text-metadata font-medium">
                  {t("externalMcp.ttl")}
                </legend>
                {(
                  [
                    ["default", t("externalMcp.ttl30")],
                    ["days90", t("externalMcp.ttl90")],
                    ["days365", t("externalMcp.ttl365")],
                    ["permanent", t("externalMcp.ttlPermanent")],
                  ] as const
                ).map(([value, label]) => (
                  <label key={value} className="flex items-center gap-2">
                    <input
                      type="radio"
                      name="external-mcp-ttl"
                      data-external-mcp-ttl={value}
                      checked={ttlKind === value}
                      onChange={() => setTtlKind(value)}
                    />
                    <span>{label}</span>
                  </label>
                ))}
                {showPermanentWarning ? (
                  <p
                    className="text-metadata text-destructive"
                    data-external-mcp-permanent-warning
                  >
                    {t("externalMcp.permanentWarning")}
                  </p>
                ) : null}
              </fieldset>
              {createError != null && createError !== "" ? (
                <p className="text-metadata text-destructive">{createError}</p>
              ) : null}
              <DialogFooter>
                <Button variant="outline" onClick={() => setCreateOpen(false)}>
                  {t("externalMcp.cancel")}
                </Button>
                <Button
                  type="button"
                  data-external-mcp-submit
                  disabled={createBusy}
                  onClick={() => void submitCreate()}
                >
                  {createBusy
                    ? t("externalMcp.creating")
                    : t("externalMcp.createClient")}
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>

      <Dialog
        open={revokeTarget != null}
        onOpenChange={(open) => {
          if (!open) setRevokeTarget(null);
        }}
      >
        <DialogContent data-external-mcp-revoke-dialog>
          <DialogHeader>
            <DialogTitle>{t("externalMcp.revokeTitle")}</DialogTitle>
          </DialogHeader>
          <p className="text-metadata">{t("externalMcp.revokeBody")}</p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)}>
              {t("externalMcp.cancel")}
            </Button>
            <Button
              variant="destructive"
              data-external-mcp-revoke-confirm
              disabled={revokeBusy}
              onClick={() => void confirmRevoke()}
            >
              {t("externalMcp.revoke")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}

export function ExternalMcpSettingsNarrowPreview({
  themeClass,
}: {
  themeClass: "light" | "dark";
}) {
  return (
    <div
      className={cn(themeClass, "bg-background w-[320px] border p-2")}
      data-external-mcp-narrow
    >
      <ExternalMcpSettingsPage
        statusLoader={async () => ({
          enabled: true,
          endpoint: "http://127.0.0.1:4599/external-mcp",
          data_dir_configured: true,
          client_count: 0,
          active_client_count: 0,
        })}
        clientsLoader={async () => []}
      />
    </div>
  );
}
