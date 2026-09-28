import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Puzzle } from "@/components/ui/icons";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import type { PluginUiSlotId } from "../bridge";
import type { ActivePluginUiContribution } from "./contributions";

export function PluginUiSlot({
  slot,
  contributions,
  onInvoke,
  activeCommand,
}: {
  slot: Exclude<PluginUiSlotId, "host.actions">;
  contributions: ActivePluginUiContribution[];
  onInvoke: (contribution: ActivePluginUiContribution) => Promise<void>;
  activeCommand?: string;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  if (contributions.length === 0) return null;

  const invoke = async (contribution: ActivePluginUiContribution) => {
    const key = `${contribution.pluginId}:${contribution.id}`;
    if (busy != null && busy !== "") return;
    setBusy(key);
    try {
      await onInvoke(contribution);
    } finally {
      setBusy(null);
    }
  };

  if (slot === "rail.features") {
    return (
      <div
        data-plugin-ui-slot={slot}
        role="group"
        aria-label="Plugin actions"
        className="flex flex-col gap-0.5"
      >
        {contributions.map((contribution) => {
          const key = `${contribution.pluginId}:${contribution.id}`;
          const current = contribution.command === activeCommand;
          return (
            <Button
              key={key}
              type="button"
              data-selected={current || undefined}
              aria-current={current ? "page" : undefined}
              variant="ghost"
              size="row"
              focusStyle="inset"
              className={cn(
                "h-control text-foreground/75 w-full gap-2 px-2",
                current && "bg-fill-rest text-foreground"
              )}
              title={
                contribution.description ||
                `${contribution.pluginName}: ${contribution.label}`
              }
              aria-label={`${contribution.pluginName}: ${contribution.label}`}
              disabled={busy !== null}
              onClick={() => void invoke(contribution)}
            >
              {busy === key ? (
                <Spinner className="size-4 shrink-0" aria-hidden="true" />
              ) : (
                <Puzzle
                  className="text-muted-foreground size-4 shrink-0"
                  aria-hidden="true"
                />
              )}
              <span className="min-w-0 flex-1 truncate">
                {contribution.label}
              </span>
            </Button>
          );
        })}
      </div>
    );
  }

  if (slot === "session.header") {
    return (
      <div
        data-plugin-ui-slot={slot}
        className="flex min-w-0 items-center gap-1 overflow-x-auto"
      >
        {contributions.map((contribution) => {
          const key = `${contribution.pluginId}:${contribution.id}`;
          return (
            <Button
              key={key}
              type="button"
              variant="ghost"
              size="compact"
              className="session-header-plugin-action text-muted-foreground hover:text-muted-foreground max-w-44 shrink-0"
              title={
                contribution.description ||
                `${contribution.pluginName}: ${contribution.label}`
              }
              aria-label={`${contribution.pluginName}: ${contribution.label}`}
              disabled={busy !== null}
              onClick={() => void invoke(contribution)}
            >
              {busy === key ? (
                <Spinner data-icon="inline-start" />
              ) : (
                <Puzzle data-icon="inline-start" />
              )}
              <span className="session-header-action-label truncate">
                {contribution.label}
              </span>
            </Button>
          );
        })}
      </div>
    );
  }

  if (slot === "transcript.before" || slot === "composer.above") {
    return (
      <section
        data-plugin-ui-slot={slot}
        aria-label="Plugin actions"
        className={slot === "composer.above" ? "shrink-0 px-6" : "pb-3"}
      >
        <div className="mx-auto flex w-full max-w-3xl flex-wrap items-center gap-1">
          {contributions.map((contribution) => {
            const key = `${contribution.pluginId}:${contribution.id}`;
            return (
              <Tooltip key={key}>
                <TooltipTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      size="compact"
                      className="text-muted-foreground"
                      aria-label={`${contribution.pluginName}: ${contribution.label}`}
                      disabled={busy !== null}
                      onClick={() => void invoke(contribution)}
                    >
                      {busy === key ? (
                        <Spinner data-icon="inline-start" />
                      ) : (
                        <Puzzle data-icon="inline-start" />
                      )}
                      {contribution.label}
                    </Button>
                  }
                />
                <TooltipContent>
                  {contribution.description || contribution.label}
                </TooltipContent>
              </Tooltip>
            );
          })}
        </div>
      </section>
    );
  }

  if (slot === "composer.toolbar") {
    return (
      <div
        data-plugin-ui-slot={slot}
        role="group"
        aria-label="Plugin actions"
        className="flex items-center gap-0.5"
      >
        {contributions.map((contribution) => {
          const key = `${contribution.pluginId}:${contribution.id}`;
          const label = `${contribution.pluginName}: ${contribution.label}`;
          return (
            <Tooltip key={key}>
              <TooltipTrigger
                render={
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="text-muted-foreground size-7 shrink-0"
                    aria-label={label}
                    disabled={busy !== null}
                    onClick={() => void invoke(contribution)}
                  >
                    {busy === key ? (
                      <Spinner className="size-3.5" />
                    ) : (
                      <Puzzle className="size-3.5" />
                    )}
                  </Button>
                }
              />
              <TooltipContent>
                {contribution.description || label}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </div>
    );
  }

  const unhandledSlot: never = slot;
  return unhandledSlot;
}
