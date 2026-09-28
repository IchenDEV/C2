import { useState } from "react";

import { Button } from "@/components/ui/button";
import { FileText, FolderTree, X } from "@/components/ui/icons";
import { TooltipButton } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { useT } from "../i18n";
import { dirtyKey, useDirtyPaths } from "./dirty";
import { FilePanel } from "./FilePanel";
import { FileViewer } from "./FileViewer";
import type { FileRevealTarget } from "./FileViewer";

interface FileDockContentProps {
  cwd: string | null;
  openFiles: string[];
  activeFile: string | null;
  reveal: FileRevealTarget | null;
  highlightFile?: string | null;
  onActiveFile: (path: string) => void;
  onCloseFile: (path: string) => void;
  onInsertFile: (path: string) => void;
  onOpenFile: (path: string) => void;
  onSendText: (text: string) => void;
}

/** File tabs, viewer, and tree composed as one content module for the generic Dock container. */
export function FileDockContent({
  cwd,
  openFiles,
  activeFile,
  reveal,
  highlightFile,
  onActiveFile,
  onCloseFile,
  onInsertFile,
  onOpenFile,
  onSendText,
}: FileDockContentProps) {
  const t = useT();
  const dirtyPaths = useDirtyPaths();
  const [browsing, setBrowsing] = useState(false);
  const showTree = browsing || activeFile == null || activeFile === "";

  return (
    <div
      className="flex min-h-0 min-w-0 flex-1"
      data-file-dock-view={showTree ? "browser" : "editor"}
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {(openFiles.length > 0 || !showTree) && (
          <div
            data-file-tabs
            className="flex shrink-0 items-center gap-1 overflow-x-auto p-2"
          >
            <TooltipButton
              label={t("action.open_files")}
              variant="ghost"
              size="icon"
              aria-pressed={showTree}
              onClick={() => setBrowsing((current) => !current)}
            >
              <FolderTree className="size-3.5" />
            </TooltipButton>
            {openFiles.map((path) => {
              const name = path.split("/").pop() ?? path;
              const active = path === activeFile;
              return (
                <Button
                  key={path}
                  type="button"
                  variant="selectable"
                  size="row"
                  focusStyle="inset"
                  data-selected={active ? "true" : "false"}
                  onClick={() => {
                    onActiveFile(path);
                    setBrowsing(false);
                  }}
                  title={path}
                  className={cn(
                    "group px-module-inset text-metadata h-control relative max-w-48 shrink-0 gap-1.5",
                    active
                      ? "text-foreground"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                >
                  <FileText className="size-3.5 shrink-0" />
                  <span className="truncate">{name}</span>
                  {cwd != null &&
                    cwd !== "" &&
                    dirtyPaths.has(dirtyKey(cwd, path)) && (
                      <span className="bg-warning size-1.5 shrink-0 rounded-full" />
                    )}
                  <X
                    className="hover:text-destructive size-3 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                    onClick={(event) => {
                      event.stopPropagation();
                      onCloseFile(path);
                    }}
                  />
                  {active && (
                    <span className="bg-primary absolute inset-x-1.5 -bottom-px h-0.5 rounded-none" />
                  )}
                </Button>
              );
            })}
          </div>
        )}

        {showTree ? (
          <FilePanel
            cwd={cwd}
            onInsert={onInsertFile}
            onOpen={(path) => {
              setBrowsing(false);
              onOpenFile(path);
            }}
            openPath={activeFile ?? highlightFile ?? null}
          />
        ) : activeFile != null &&
          activeFile !== "" &&
          cwd != null &&
          cwd !== "" ? (
          <FileViewer
            key={activeFile}
            cwd={cwd}
            path={activeFile}
            onInsert={onInsertFile}
            onOpen={onOpenFile}
            onComment={onSendText}
            reveal={reveal}
          />
        ) : (
          <div className="flex min-h-0 flex-1 items-center justify-center p-6">
            <p className="text-metadata text-muted-foreground text-center">
              {t("files.noneOpen")}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
