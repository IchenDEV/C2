import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, extname, join } from "node:path";

import Electrobun, {
  ApplicationMenu,
  BrowserView,
  BrowserWindow,
  ContextMenu,
  PATHS,
  Screen,
  Utils,
} from "electrobun/bun";

import { showAssistantNotification } from "../assistant/notification";
import type { AutomationAlert } from "../bridge";
import { macOSApplicationMenu } from "./applicationMenu";
import { AppshotManager } from "./appshots";
import { automationAlertNotification } from "./automationAlert";
import {
  nativeContextMenuAction,
  nativeContextMenuConfig,
} from "./contextMenuHost";
import { NativeHost } from "./nativeHost";
import { PluginHostActionController } from "./pluginHostActions";
import type {
  CodeTwoRPC,
  DesktopPetState,
  DesktopEvent,
  DialogFilter,
  OpenDialogOptions,
  SaveDialogOptions,
  WorkspaceOpenTarget,
} from "./rpc";
import { readSystemProfileAvatar } from "./systemProfile";
import { createMacOSTouchBar } from "./touchBar";
import { getAppUpdateStatus, startAppUpdateCheck } from "./update";
import {
  configureMacOSWindowEffects,
  performMacOSTitlebarDoubleClick,
  setMacOSSystemBadgeCount,
} from "./windowEffects";
import { workspaceOpenCommand } from "./workspaceOpen";

function filterExtensions(filters: DialogFilter[] | undefined): string {
  const extensions = filters?.flatMap((filter) => filter.extensions) ?? [];
  return extensions.length > 0 ? extensions.join(",") : "*";
}

async function openDialog(options: OpenDialogOptions): Promise<string[]> {
  const selected = await Utils.openFileDialog({
    allowedFileTypes: filterExtensions(options.filters),
    canChooseFiles: options.directory !== true,
    canChooseDirectory: options.directory ?? false,
    allowsMultipleSelection: options.multiple ?? false,
  });
  return selected.filter(Boolean);
}

async function processText(
  command: string[],
  env?: Record<string, string>
): Promise<string | null> {
  try {
    const process = Bun.spawn(command, {
      stdout: "pipe",
      stderr: "ignore",
      env: { ...Bun.env, ...env },
    });
    const output = await new Response(process.stdout).text();
    return (await process.exited) === 0 ? output.trim() || null : null;
  } catch {
    return null;
  }
}

async function saveDialog(options: SaveDialogOptions): Promise<string | null> {
  const defaultPath = options.defaultPath ?? "untitled";
  const title = options.title ?? "Save";
  if (process.platform === "darwin") {
    const script = [
      "on run argv",
      "try",
      "set chosenFile to choose file name with prompt (item 1 of argv) default name (item 2 of argv)",
      "return POSIX path of chosenFile",
      "on error number -128",
      'return ""',
      "end try",
      "end run",
    ].join("\n");
    return await processText([
      "/usr/bin/osascript",
      "-e",
      script,
      "--",
      title,
      basename(defaultPath),
    ]);
  }
  if (process.platform === "win32") {
    const extension = extname(defaultPath).slice(1);
    const filter = extension
      ? `${extension.toUpperCase()} (*.${extension})|*.${extension}|All files (*.*)|*.*`
      : "All files (*.*)|*.*";
    const script = [
      "Add-Type -AssemblyName System.Windows.Forms",
      "$dialog = New-Object System.Windows.Forms.SaveFileDialog",
      "$dialog.Title = $env:CODETWO_SAVE_TITLE",
      "$dialog.FileName = $env:CODETWO_SAVE_NAME",
      "$dialog.Filter = $env:CODETWO_SAVE_FILTER",
      "if ($dialog.ShowDialog() -eq 'OK') { Write-Output $dialog.FileName }",
    ].join("; ");
    return await processText(
      ["powershell.exe", "-NoProfile", "-Command", script],
      {
        CODETWO_SAVE_TITLE: title,
        CODETWO_SAVE_NAME: basename(defaultPath),
        CODETWO_SAVE_FILTER: filter,
      }
    );
  }
  return await processText([
    "zenity",
    "--file-selection",
    "--save",
    "--confirm-overwrite",
    `--title=${title}`,
    `--filename=${join(Utils.paths.documents, basename(defaultPath))}`,
  ]);
}

async function openWorkspace(
  path: string,
  target: WorkspaceOpenTarget
): Promise<boolean> {
  if (target === "finder") return Utils.openPath(path);
  const command = workspaceOpenCommand(path, target);
  if (!command) return false;

  try {
    const child = Bun.spawn(command, {
      stdin: "ignore",
      stdout: "ignore",
      stderr: "ignore",
    });
    return (await child.exited) === 0;
  } catch {
    return false;
  }
}

const queuedEvents: DesktopEvent[] = [];
let rendererReady = false;
let rpc: ReturnType<typeof BrowserView.defineRPC<CodeTwoRPC>>;
let desktopPetRpc: ReturnType<typeof BrowserView.defineRPC<CodeTwoRPC>>;
let appshots: AppshotManager;
let pluginHostActions: PluginHostActionController | null = null;
const applicationName = process.env.CODETWO_APP_NAME ?? "C2";
if (process.platform === "darwin") {
  ApplicationMenu.setApplicationMenu(macOSApplicationMenu());
}
const profileDataDir = process.env.CODETWO_PROFILE_DATA_DIR;
const profileSceneSocket = process.env.CODETWO_PROFILE_SCENE_SOCKET;
const dataDir =
  process.env.CODETWO_DATA_DIR ??
  (profileDataDir === "" ? undefined : profileDataDir) ??
  join(
    Utils.paths.appData,
    process.env.CODETWO_APP_IDENTIFIER ?? "dev.codetwo.app.dev"
  );
if (profileSceneSocket != null && profileSceneSocket !== "") {
  process.env.CODETWO_SCENE_SOCKET = profileSceneSocket;
}
const desktopPetPositionPath = join(dataDir, "desktop-pet-window.json");
const desktopPetWidth = 184;
const desktopPetHeights = { small: 156, medium: 180, large: 204 } as const;
const desktopPetBubbleHeight = 64;
let desktopPetState: DesktopPetState = {
  visible: false,
  animation: "idle",
  bubble: null,
  appearance: {
    petActivityEnabled: true,
    petSize: "medium",
    petSource: "builtin",
    petId: "naiwa",
    petName: "Naiwa",
  },
};
let desktopPetWindow: BrowserWindow | null = null;
let desktopPetRendererReady = false;
let desktopPetProgrammaticPosition: { x: number; y: number } | null = null;

function desktopPetHeight() {
  return (
    desktopPetHeights[desktopPetState.appearance.petSize] +
    (desktopPetState.bubble != null && desktopPetState.bubble !== ""
      ? desktopPetBubbleHeight
      : 0)
  );
}

function desktopPetFrame() {
  const display = Screen.getPrimaryDisplay().workArea;
  const height = desktopPetHeight();
  const fallback = {
    x: display.x + display.width - desktopPetWidth - 24,
    y: display.y + display.height - height - 24,
  };
  try {
    const saved = JSON.parse(readFileSync(desktopPetPositionPath, "utf-8")) as {
      x?: unknown;
      y?: unknown;
    };
    if (typeof saved.x !== "number" || typeof saved.y !== "number")
      throw new Error("invalid");
    const { x, y } = saved as { x: number; y: number };
    const workArea = Screen.getAllDisplays()
      .map((item) => item.workArea)
      .find(
        (area) =>
          x >= area.x - desktopPetWidth / 2 &&
          x <= area.x + area.width - desktopPetWidth / 2 &&
          y >= area.y - height / 2 &&
          y <= area.y + area.height - height / 2
      );
    if (!workArea) return { ...fallback, width: desktopPetWidth, height };
    return {
      x: Math.min(
        workArea.x + workArea.width - desktopPetWidth,
        Math.max(workArea.x, x)
      ),
      y: Math.min(
        workArea.y + workArea.height - height,
        Math.max(workArea.y, y)
      ),
      width: desktopPetWidth,
      height,
    };
  } catch {
    return { ...fallback, width: desktopPetWidth, height };
  }
}

function applyDesktopPetState() {
  if (!desktopPetWindow) return;
  const frame = desktopPetWindow.getFrame();
  const height = desktopPetHeight();
  if (frame.width !== desktopPetWidth || frame.height !== height) {
    const y = frame.y + frame.height - height;
    desktopPetProgrammaticPosition = { x: frame.x, y };
    desktopPetWindow.setFrame(frame.x, y, desktopPetWidth, height);
  }
  if (desktopPetRendererReady && desktopPetState.visible)
    desktopPetWindow.showInactive();
  else desktopPetWindow.hide();
  desktopPetRpc.send.event({
    name: "desktop-pet-state",
    payload: desktopPetState,
  });
}

function persistDesktopPetPosition(x: number, y: number) {
  try {
    mkdirSync(dataDir, { recursive: true, mode: 0o700 });
    const normalizedY =
      y +
      (desktopPetState.bubble != null && desktopPetState.bubble !== ""
        ? desktopPetBubbleHeight
        : 0);
    writeFileSync(
      desktopPetPositionPath,
      `${JSON.stringify({ x, y: normalizedY })}\n`,
      {
        mode: 0o600,
      }
    );
  } catch {
    // A read-only app-data directory should not make the companion unusable for this run.
  }
}
const hostExecutable =
  process.platform === "win32"
    ? "codetwo-desktop-host.exe"
    : "codetwo-desktop-host";
const host = new NativeHost({
  executable: join(PATHS.RESOURCES_FOLDER, "app", "bin", hostExecutable),
  dataDir,
  onEvent: (event) => {
    pluginHostActions?.handleHostEvent(event);
    if (event.name === "automation-alert") {
      const alert = event.payload as Partial<AutomationAlert> | null;
      Utils.showNotification(
        automationAlertNotification(alert, applicationName)
      );
    }
    if (event.name === "engine-event") {
      showAssistantNotification(
        event.payload,
        (notification) => Utils.showNotification(notification),
        (error) =>
          console.warn("Assistant desktop notification unavailable", error)
      );
    }
    if (rendererReady) rpc.send.event(event);
    else queuedEvents.push(event);
  },
});

try {
  await host.start();
} catch (error) {
  await Utils.showMessageBox({
    type: "error",
    title: `${applicationName} could not start`,
    message: error instanceof Error ? error.message : String(error),
    buttons: ["Quit"],
  });
  Utils.quit();
  throw error;
}

rpc = BrowserView.defineRPC<CodeTwoRPC>({
  maxRequestTime: Infinity,
  handlers: {
    requests: {
      call: async ({
        name,
        args,
        projectPath,
      }: {
        name: string;
        args: unknown;
        projectPath: string | null;
      }) => await host.call(name, args, projectPath),
      dialogOpen: openDialog,
      dialogSave: saveDialog,
      confirm: async ({ message, title }) => {
        const result = await Utils.showMessageBox({
          type: "warning",
          title: title ?? applicationName,
          message,
          buttons: ["Cancel", "Continue"],
          defaultId: 0,
          cancelId: 0,
        });
        return result.response === 1;
      },
      contextMenuShow: ({ requestId, items }) => {
        ContextMenu.showContextMenu(nativeContextMenuConfig(items, requestId));
      },
      openExternal: ({ url }) => Utils.openExternal(url),
      openPath: ({ path }) => Utils.openPath(path),
      openWorkspace: async ({ path, target }) =>
        await openWorkspace(path, target),
      showItemInFolder: ({ path }) => {
        Utils.showItemInFolder(path);
        return true;
      },
      systemBadgeSet: ({ count }) => setMacOSSystemBadgeCount(count),
      titlebarDoubleClick: () =>
        performMacOSTitlebarDoubleClick(mainWindow.ptr),
      systemProfileAvatar: readSystemProfileAvatar,
      browserZoom: ({ webviewId, factor }) => {
        BrowserView.getById(webviewId)?.setPageZoom(factor);
      },
      desktopPetUpdate: (state) => {
        desktopPetState = state;
        applyDesktopPetState();
      },
      openDevtools: () => mainWindow.webview.openDevTools(),
      updateStatus: getAppUpdateStatus,
      updateCheck: startAppUpdateCheck,
      appshotsSettings: () => appshots.getSettings(),
      appshotsUpdate: (patch) => appshots.updateSettings(patch),
      appshotsRequestPermissions: ({ kind }) =>
        appshots.requestPermissions(kind),
      appshotsOpenPrivacySettings: ({ kind }) =>
        appshots.openPrivacySettings(kind),
      appshotsCapture: async () => {
        const capture = await appshots.capture();
        rpc.send.appshotCaptured(capture);
        mainWindow.show();
        return capture;
      },
      appshotsGet: ({ id }) => appshots.getCapture(id),
    },
    messages: {},
  },
});

desktopPetRpc = BrowserView.defineRPC<CodeTwoRPC>({
  maxRequestTime: Infinity,
  handlers: {
    requests: {
      contextMenuShow: ({ requestId, items }) => {
        ContextMenu.showContextMenu(nativeContextMenuConfig(items, requestId));
      },
      desktopPetState: () => desktopPetState,
      desktopPetHide: () => {
        desktopPetState = { ...desktopPetState, visible: false };
        applyDesktopPetState();
        rpc.send.event({ name: "desktop-pet-hidden", payload: null });
      },
    },
    messages: {},
  },
});

ContextMenu.on("context-menu-clicked", (event) => {
  const action = nativeContextMenuAction(event);
  if (!action) return;
  rpc.send.event({ name: "native-context-menu-action", payload: action });
  desktopPetRpc.send.event({
    name: "native-context-menu-action",
    payload: action,
  });
});

const display = Screen.getPrimaryDisplay().workArea;
const width = 1200;
const height = 800;
const mainWindow = new BrowserWindow({
  title: applicationName,
  frame: {
    x: Math.max(display.x, display.x + Math.round((display.width - width) / 2)),
    y: Math.max(
      display.y,
      display.y + Math.round((display.height - height) / 2)
    ),
    width,
    height,
  },
  url: "views://main/index.html",
  renderer: "native",
  rpc,
  titleBarStyle: "hiddenInset",
  transparent: process.platform === "darwin",
  sandbox: false,
});
desktopPetWindow = new BrowserWindow({
  title: `${applicationName} Pet`,
  frame: desktopPetFrame(),
  url: "views://main/desktop-pet.html",
  renderer: "native",
  rpc: desktopPetRpc,
  titleBarStyle: "hidden",
  transparent: true,
  passthrough: false,
  hidden: true,
  activate: false,
  styleMask: {
    Titled: false,
    Closable: false,
    Miniaturizable: false,
    Resizable: false,
    FullSizeContentView: true,
    UtilityWindow: true,
    NonactivatingPanel: process.platform === "darwin",
  },
  sandbox: false,
});
desktopPetWindow.setAlwaysOnTop(true);
desktopPetWindow.setVisibleOnAllWorkspaces(true);
desktopPetWindow.webview.on("dom-ready", () => {
  desktopPetRendererReady = true;
  applyDesktopPetState();
});
desktopPetWindow.on("move", (event) => {
  const position = (event as { data?: { x?: unknown; y?: unknown } }).data;
  if (typeof position?.x === "number" && typeof position.y === "number") {
    if (
      desktopPetProgrammaticPosition &&
      position.x === desktopPetProgrammaticPosition.x &&
      position.y === desktopPetProgrammaticPosition.y
    ) {
      desktopPetProgrammaticPosition = null;
      return;
    }
    desktopPetProgrammaticPosition = null;
    persistDesktopPetPosition(position.x, position.y);
  }
});
mainWindow.on("close", () => desktopPetWindow?.close());
appshots = new AppshotManager(
  dataDir,
  process.env.CODETWO_APP_IDENTIFIER ?? "dev.codetwo.app.dev",
  (capture) => rpc.send.appshotCaptured(capture),
  (message) => rpc.send.appshotFailed({ message }),
  () => mainWindow.show()
);
if (process.platform === "darwin") {
  const windowEffectsStatus = configureMacOSWindowEffects(mainWindow.ptr);
  if (!windowEffectsStatus.shadow) {
    console.warn("The macOS system window shadow could not be restored");
  }
  if (!windowEffectsStatus.backdrop) {
    console.warn("The macOS system backdrop could not be installed");
  }
  const touchBar = createMacOSTouchBar(
    mainWindow.ptr,
    (contributionKey, itemId) =>
      pluginHostActions?.invoke(contributionKey, itemId)
  );
  if (touchBar) {
    pluginHostActions = new PluginHostActionController(
      async (name, args, projectPath) =>
        await host.call(name, args, projectPath),
      touchBar
    );
    void pluginHostActions.start();
  }
  // AppKit can reset standard-window-button frames during its own resize layout pass. Reapply the
  // same fixed position afterward; the 46px titlebar has no runtime geometry to measure.
  mainWindow.on("resize", () => mainWindow.setWindowButtonPosition(16, 16));
}

mainWindow.webview.on("dom-ready", () => {
  if (process.platform === "darwin") {
    mainWindow.webview.executeJavascript(
      'document.documentElement.classList.add("macos-window-glass")'
    );
    // Share the rail's leading content column (mx-2 + px-2 = 16px) and center the 14px native
    // controls in the shared 46px title row.
    mainWindow.setWindowButtonPosition(16, 16);
  }
  rendererReady = true;
  rpc.send.hostStatus({ ready: true });
  for (const event of queuedEvents.splice(0)) rpc.send.event(event);
});

Electrobun.events.on(`new-window-open-${mainWindow.webview.id}`, (event) => {
  const detail = (event as { data?: { detail?: unknown } }).data?.detail;
  const url =
    typeof detail === "string"
      ? detail
      : typeof detail === "object" &&
          detail !== null &&
          typeof (detail as { url?: unknown }).url === "string"
        ? (detail as { url: string }).url
        : null;
  if (url != null && url !== "") Utils.openExternal(url);
});

let shuttingDown = false;
Electrobun.events.on("before-quit", (event) => {
  if (shuttingDown) return;
  event.response = { allow: false };
  shuttingDown = true;
  appshots.shutdown();
  pluginHostActions?.dispose();
  void host.shutdown().finally(() => Utils.quit());
});
