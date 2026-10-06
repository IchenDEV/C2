import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

import { environmentRegistry } from "../coreTransport";
import { useEnvironments } from "../environment/useEnvironments";
import { useT } from "../i18n";
import type {
  RemoteEnvironment,
  RemoteServerDevice,
} from "../remoteEnvironments";
import { GroupHeading, Page, Row } from "./SettingsPrimitives";

function lastSeenText(seconds: number, never: string): string {
  return seconds > 0 ? new Date(seconds * 1000).toLocaleString() : never;
}

function DeviceList({ environment }: { environment: RemoteEnvironment }) {
  const t = useT();
  const [devices, setDevices] = useState<RemoteServerDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setDevices(null);
    setError(null);
    environmentRegistry.devices(environment.id).then(
      (list) => {
        if (active) setDevices(list);
      },
      (failure: unknown) => {
        if (!active) return;
        setError(failure instanceof Error ? failure.message : String(failure));
        setDevices([]);
      }
    );
    return () => {
      active = false;
    };
  }, [environment.id]);

  const revoke = async (device: RemoteServerDevice) => {
    setRevoking(device.id);
    setError(null);
    try {
      const { wasCurrent } = await environmentRegistry.revokeDevice(
        environment.id,
        device.id
      );
      // The environment is gone when this app signed itself out; this list unmounts with it.
      if (!wasCurrent) {
        setDevices(await environmentRegistry.devices(environment.id));
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setRevoking(null);
      setConfirming(null);
    }
  };

  return (
    <div className="border-border/60 mt-1 flex flex-col gap-1 border-l pl-3">
      <p className="text-metadata text-muted-foreground">
        {t("settings.environmentDevicesHint")}
      </p>
      {error != null && (
        <p role="alert" className="text-metadata text-destructive">
          {error}
        </p>
      )}
      {devices === null ? (
        <p className="text-metadata text-muted-foreground">
          {t("settings.environmentDevicesLoading")}
        </p>
      ) : devices.length === 0 ? (
        <p className="text-metadata text-muted-foreground">
          {t("settings.environmentDevicesEmpty")}
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {devices.map((device) => (
            <li
              key={device.id}
              data-device-id={device.id}
              className="flex items-center gap-2 py-1"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="truncate">{device.name}</span>
                  {device.current && (
                    <Badge variant="secondary">
                      {t("settings.environmentDeviceThis")}
                    </Badge>
                  )}
                </div>
                <span className="text-metadata text-muted-foreground block">
                  {t("settings.environmentDeviceLastSeen", {
                    time: lastSeenText(
                      device.lastSeen,
                      t("settings.environmentDeviceNever")
                    ),
                  })}
                </span>
                {confirming === device.id && device.current && (
                  <span className="text-metadata text-destructive block">
                    {t("settings.environmentDeviceConfirmSelf")}
                  </span>
                )}
              </div>
              {confirming === device.id ? (
                <>
                  <Button
                    variant="destructive"
                    size="sm"
                    disabled={revoking !== null}
                    onClick={() => void revoke(device)}
                  >
                    {revoking === device.id ? (
                      <Spinner />
                    ) : (
                      t("settings.environmentDeviceConfirm")
                    )}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={revoking !== null}
                    onClick={() => setConfirming(null)}
                  >
                    {t("settings.environmentDeviceCancel")}
                  </Button>
                </>
              ) : (
                <Button
                  variant="outline"
                  size="sm"
                  aria-label={`${t("settings.environmentDeviceRevoke")} ${device.name}`}
                  onClick={() => setConfirming(device.id)}
                >
                  {t("settings.environmentDeviceRevoke")}
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function EnvironmentRow({ environment }: { environment: RemoteEnvironment }) {
  const t = useT();
  const { statuses } = useEnvironments();
  const status = statuses[environment.id];
  const [workspace, setWorkspace] = useState(environment.workspace ?? "");
  const [showDevices, setShowDevices] = useState(false);
  const state = status?.state ?? "connecting";
  const label =
    state === "online"
      ? t("settings.environmentOnline")
      : state === "offline"
        ? t("settings.environmentOffline")
        : t("settings.environmentConnecting");

  return (
    <>
      <Row
        label={environment.name}
        hint={
          <>
            <span className="font-mono">{environment.baseUrl}</span>
            {status?.error != null && state === "offline" && (
              <span className="text-destructive block">{status.error}</span>
            )}
          </>
        }
        controlClassName="gap-2"
      >
        <Badge variant={state === "online" ? "secondary" : "outline"}>
          {label}
        </Badge>
        <Button
          variant="outline"
          size="sm"
          onClick={() => environmentRegistry.remove(environment.id)}
        >
          {t("settings.environmentRemove")}
        </Button>
      </Row>
      <Row
        compact
        label={t("settings.environmentWorkspace")}
        hint={t("settings.environmentWorkspaceHint")}
      >
        <Input
          aria-label={`${environment.name} · ${t("settings.environmentWorkspace")}`}
          placeholder="/home/you/project"
          autoComplete="off"
          spellCheck={false}
          value={workspace}
          onChange={(event) => setWorkspace(event.target.value)}
          onBlur={() =>
            environmentRegistry.update(environment.id, {
              workspace: workspace.trim() === "" ? null : workspace,
            })
          }
        />
      </Row>
      <Row
        compact
        label={t("settings.environmentDevices")}
        hint={showDevices ? undefined : t("settings.environmentDevicesHint")}
      >
        <Button
          variant="outline"
          size="sm"
          aria-expanded={showDevices}
          onClick={() => setShowDevices((open) => !open)}
        >
          {showDevices
            ? t("settings.environmentDevicesHide")
            : t("settings.environmentDevicesShow")}
        </Button>
      </Row>
      {showDevices && <DeviceList environment={environment} />}
    </>
  );
}

export function RemoteEnvironmentsSettingsPage() {
  const t = useT();
  const { environments } = useEnvironments();
  const [pairingLink, setPairingLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const connect = async () => {
    if (pairingLink.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await environmentRegistry.add(pairingLink);
      setPairingLink("");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page
      title={t("settings.environments")}
      description={t("settings.environmentsHint")}
    >
      <GroupHeading>{t("settings.environmentPairingLink")}</GroupHeading>
      <form
        className="flex items-center gap-2 pt-1.5"
        onSubmit={(event) => {
          event.preventDefault();
          void connect();
        }}
      >
        <Input
          aria-label={t("settings.environmentPairingLink")}
          placeholder={t("settings.environmentPairingPlaceholder")}
          autoComplete="off"
          spellCheck={false}
          value={pairingLink}
          onChange={(event) => setPairingLink(event.target.value)}
        />
        <Button
          type="submit"
          size="sm"
          disabled={busy || pairingLink.trim() === ""}
        >
          {busy ? (
            <>
              <Spinner />
              {t("settings.environmentAdding")}
            </>
          ) : (
            t("settings.environmentAdd")
          )}
        </Button>
      </form>
      {error != null && (
        <p role="alert" className="text-metadata text-destructive pt-1.5">
          {error}
        </p>
      )}
      <p className="text-metadata text-muted-foreground pt-1.5">
        {t("settings.environmentsHelp")}
      </p>

      {environments.length === 0 ? (
        <p className="text-metadata text-muted-foreground pt-3">
          {t("settings.environmentEmpty")}
        </p>
      ) : (
        environments.map((environment) => (
          <EnvironmentRow key={environment.id} environment={environment} />
        ))
      )}
    </Page>
  );
}
