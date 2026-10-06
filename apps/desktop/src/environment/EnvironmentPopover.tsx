import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderPlus,
  GitBranch,
  GitCommitHorizontal,
  Globe2,
  Laptop,
  Monitor,
  Server,
  Settings,
  SlidersHorizontal,
  SquarePlus,
} from "@/components/ui/icons";
import type { AppIcon } from "@/components/ui/icons";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";

import { getArtifact } from "../bridge";
import type { GitStatus, Project } from "../bridge";
import { environmentRegistry } from "../coreTransport";
import { GitSyncStatus } from "../git/GitSyncStatus";
import { useT } from "../i18n";
import type { InteractiveToolPreview } from "../session/toolActivity";
import { useEnvironments } from "./useEnvironments";

function EnvironmentRow({
  icon: Icon,
  label,
  description,
  detail,
  onClick,
  active = false,
  disabled = false,
}: {
  icon: AppIcon;
  label: string;
  description?: string;
  detail?: ReactNode;
  onClick?: () => void;
  active?: boolean;
  disabled?: boolean;
}) {
  const content = (
    <>
      <Icon
        className={cn(
          "text-muted-foreground size-4 shrink-0",
          description != null && description !== "" && "mt-0.5 self-start"
        )}
      />
      <span className="min-w-0 flex-1">
        <span className="text-body block truncate">{label}</span>
        {description != null && description !== "" && (
          <span className="text-callout text-muted-foreground block truncate">
            {description}
          </span>
        )}
      </span>
      {detail !== undefined && (
        <span className="text-metadata text-muted-foreground shrink-0">
          {detail}
        </span>
      )}
      {active && <span className="bg-primary size-1.5 shrink-0 rounded-full" />}
    </>
  );

  if (!onClick) {
    return (
      <div className="min-h-control gap-module-inset px-module-inset py-control-group flex items-center">
        {content}
      </div>
    );
  }

  return (
    <Button
      type="button"
      variant={active ? "selectable" : "ghost"}
      size="row"
      focusStyle="inset"
      data-selected={active ? "true" : "false"}
      aria-pressed={active || undefined}
      disabled={disabled}
      onClick={onClick}
      className={
        description != null && description !== "" ? "items-start" : undefined
      }
    >
      {content}
    </Button>
  );
}

function ToolPreview({ preview }: { preview: InteractiveToolPreview }) {
  const t = useT();
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const label = t(
    preview.kind === "browser" ? "settings.browserUse" : "settings.computerUse"
  );
  const Icon = preview.kind === "browser" ? Globe2 : Monitor;

  useEffect(() => {
    let alive = true;
    let objectUrl: string | null = null;
    setUrl(null);
    setFailed(false);
    void getArtifact(preview.artifact.id)
      .then((bytes) => {
        if (!alive) return;
        objectUrl = URL.createObjectURL(
          new Blob([Uint8Array.from(bytes).buffer], {
            type: preview.artifact.mime_type,
          })
        );
        setUrl(objectUrl);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
      if (objectUrl != null && objectUrl !== "") URL.revokeObjectURL(objectUrl);
    };
  }, [preview.artifact.id, preview.artifact.mime_type]);

  return (
    <figure
      data-tool-preview={preview.kind}
      data-artifact-id={preview.artifact.id}
      className="rounded-module bg-fill-quiet overflow-hidden"
    >
      <figcaption className="bg-fill-rest text-callout flex min-h-8 items-center gap-2 px-2 py-1.5">
        <Icon className="text-muted-foreground size-3.5 shrink-0" aria-hidden />
        <span className="text-foreground shrink-0 font-medium">{label}</span>
        <span
          className="bg-primary size-1.5 shrink-0 animate-pulse rounded-full"
          aria-hidden
        />
        <span className="text-muted-foreground min-w-0 flex-1 truncate">
          {preview.title}
        </span>
      </figcaption>
      <div className="image-checker flex min-h-32 items-center justify-center">
        {url != null && url !== "" && !failed ? (
          <img
            src={url}
            alt={t("environment.livePreview", { tool: label })}
            width={preview.artifact.width || undefined}
            height={preview.artifact.height || undefined}
            className="max-h-64 w-full object-contain"
            onError={() => setFailed(true)}
          />
        ) : (
          <span
            role="status"
            className={cn(
              "text-callout text-muted-foreground flex items-center gap-2 px-3 py-8",
              failed && "text-destructive"
            )}
          >
            {!failed && <Spinner className="size-3.5" />}
            {t(
              failed
                ? "environment.previewUnavailable"
                : "environment.previewLoading"
            )}
          </span>
        )}
      </div>
    </figure>
  );
}

/**
 * The project environment at a glance. It keeps the compact, frequently checked Git facts in a
 * header-anchored popover. The neighboring panel control owns the dock independently.
 */
export function EnvironmentPopover({
  project,
  projectPath,
  projects,
  git,
  diffStat,
  onRefresh,
  onSelectProject,
  onAddProject,
  onOpenSourceControl,
  onOpenSettings,
  preview = null,
  suppressed = false,
}: {
  project: string | null;
  projectPath: string | null;
  projects: Project[];
  git: GitStatus | null;
  diffStat: { added: number; deleted: number };
  onRefresh: () => void;
  onSelectProject: (path: string) => void;
  onAddProject: () => void;
  onOpenSourceControl: () => void;
  onOpenSettings: () => void;
  preview?: InteractiveToolPreview | null;
  /** Keeps the mounted session workspace from leaking this portal over another full-page surface. */
  suppressed?: boolean;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [projectsOpen, setProjectsOpen] = useState(false);
  const isRepo = git?.is_repo === true;
  const { environments, statuses, activeId } = useEnvironments();
  const activeEnvironment =
    environments.find((environment) => environment.id === activeId) ?? null;

  useEffect(() => {
    if (suppressed) setOpen(false);
  }, [suppressed]);

  const changeDetail =
    git === null ? (
      "…"
    ) : isRepo ? (
      <span className="font-mono">
        <span className="text-success">+{diffStat.added}</span>{" "}
        <span className="text-destructive">−{diffStat.deleted}</span>
      </span>
    ) : (
      "—"
    );

  const openSourceControl = () => {
    setOpen(false);
    onOpenSourceControl();
  };

  const chooseProject = (path: string) => {
    setOpen(false);
    onSelectProject(path);
  };

  const addProject = () => {
    setOpen(false);
    onAddProject();
  };

  return (
    <Popover
      open={!suppressed && open}
      onOpenChange={(next) => {
        if (suppressed) return;
        setOpen(next);
        if (next) onRefresh();
      }}
    >
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="compact"
            className={cn(
              "session-header-context-main text-foreground hover:text-foreground shrink-0",
              open && "bg-fill-rest"
            )}
            aria-label={t("header.environment")}
          >
            <SlidersHorizontal
              className="session-header-context-icon text-muted-foreground size-4"
              aria-hidden
            />
            <span className="session-header-context-label">
              {activeEnvironment?.name ?? t("environment.title")}
            </span>
          </Button>
        }
      />

      <PopoverContent
        align="start"
        sideOffset={16}
        className="max-h-(--available-height) overflow-y-auto p-2"
        initialFocus={false}
      >
        <div className="h-control-field gap-module-inset px-module-inset mb-1 flex items-center">
          <h2 className="text-dialog text-muted-foreground min-w-0 flex-1 truncate font-medium">
            {t("environment.title")}
          </h2>
          <Button
            variant="ghost"
            size="icon"
            className="text-muted-foreground size-7"
            aria-label={t("header.settings")}
            onClick={() => {
              setOpen(false);
              onOpenSettings();
            }}
          >
            <Settings className="size-3.5" />
          </Button>
        </div>

        {environments.length > 0 && (
          <div
            className="mb-1"
            role="group"
            aria-label={t("environment.newSessionIn")}
          >
            <p className="text-metadata text-muted-foreground px-module-inset pb-1">
              {t("environment.newSessionIn")}
            </p>
            <EnvironmentRow
              icon={Laptop}
              label={t("environment.local")}
              active={activeEnvironment === null}
              onClick={() => environmentRegistry.setActive(null)}
            />
            {environments.map((environment) => (
              <EnvironmentRow
                key={environment.id}
                icon={Server}
                label={environment.name}
                description={environment.baseUrl}
                detail={
                  statuses[environment.id]?.state === "offline"
                    ? t("settings.environmentOffline")
                    : undefined
                }
                active={activeEnvironment?.id === environment.id}
                onClick={() => environmentRegistry.setActive(environment.id)}
              />
            ))}
          </div>
        )}

        <EnvironmentRow
          icon={SquarePlus}
          label={t("rail.changes")}
          detail={changeDetail}
          onClick={openSourceControl}
        />

        <Collapsible open={projectsOpen} onOpenChange={setProjectsOpen}>
          <CollapsibleTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                size="row"
                focusStyle="inset"
                title={projectPath ?? undefined}
              >
                <Laptop className="text-muted-foreground size-4 shrink-0" />
                <span className="text-body min-w-0 flex-1 truncate font-medium">
                  {t("environment.local")}
                </span>
                {project != null && project !== "" && (
                  <span className="text-metadata text-muted-foreground max-w-28 truncate">
                    {project}
                  </span>
                )}
                {projectsOpen ? (
                  <ChevronDown className="text-muted-foreground size-3.5 shrink-0" />
                ) : (
                  <ChevronRight className="text-muted-foreground size-3.5 shrink-0" />
                )}
              </Button>
            }
          />
          <CollapsibleContent className="max-h-48 overflow-y-auto pl-3">
            {projects.map((item) => (
              <EnvironmentRow
                key={item.path}
                icon={item.path === projectPath ? Check : Folder}
                label={item.name}
                description={item.path}
                active={item.path === projectPath}
                onClick={() => chooseProject(item.path)}
              />
            ))}
            <EnvironmentRow
              icon={FolderPlus}
              label={t("rail.addProject")}
              onClick={addProject}
            />
          </CollapsibleContent>
        </Collapsible>

        <EnvironmentRow
          icon={GitBranch}
          label={isRepo ? git.branch || "?" : t("rail.notARepo")}
          detail={
            isRepo && (git.ahead > 0 || git.behind > 0) ? (
              <GitSyncStatus
                ahead={git.ahead}
                behind={git.behind}
                className="text-primary font-mono"
              />
            ) : undefined
          }
        />
        <EnvironmentRow
          icon={GitCommitHorizontal}
          label={t("environment.commitOrPush")}
          onClick={openSourceControl}
          disabled={!isRepo}
        />

        {preview && (
          <div className="mt-2">
            <ToolPreview key={preview.artifact.id} preview={preview} />
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
