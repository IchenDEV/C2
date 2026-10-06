import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

import { environmentRegistry } from "../coreTransport";
import { useEnvironments } from "../environment/useEnvironments";
import { useT } from "../i18n";
import type { RemoteEnvironment } from "../remoteEnvironments";
import { GroupHeading, Page, Row } from "./SettingsPrimitives";

function EnvironmentRow({ environment }: { environment: RemoteEnvironment }) {
  const t = useT();
  const { statuses } = useEnvironments();
  const status = statuses[environment.id];
  const [workspace, setWorkspace] = useState(environment.workspace ?? "");
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
