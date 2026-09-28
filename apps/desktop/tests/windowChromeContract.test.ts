import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = (relativePath: string) =>
  readFileSync(new URL(relativePath, import.meta.url), "utf-8").replaceAll(
    "\r\n",
    "\n"
  );

const electrobunHost = source("../src/electrobun/index.ts");
const styles = source("../src/styles.css");
const appSource = source("../src/App.tsx");
const mainSource = source("../src/main.tsx");
const containerSource = source("../src/container.ts");
const railSource = source("../src/sidebar/SessionRail.tsx");
const sceneStudioSource = source("../src/session/SceneStudio.tsx");
const dockSource = source("../src/dock/Dock.tsx");
const tabsSource = source("../src/components/ui/tabs.tsx");
const sessionHeaderActionsSource = source(
  "../src/session/SessionHeaderActions.tsx"
);
const electrobunConfig = source("../electrobun.config.ts");
const prepareElectrobun = source("../scripts/prepare-electrobun.ts");
const patchMacOSInfo = source("../scripts/patch-macos-info.ts");
const nativeWindowEffects = source(
  "../native/window-effects/CodeTwoWindowEffects.m"
);
const themeSource = source("../src/theme.tsx");
const titlebarSource = source("../src/electrobun/titlebar.ts");
const rpcSource = source("../src/electrobun/rpc.ts");

describe("macOS window chrome contract", () => {
  test("centers the native macOS traffic lights on the rail's leading content column", () => {
    expect(electrobunHost).toContain('titleBarStyle: "hiddenInset"');
    expect(electrobunHost).not.toContain("trafficLightOffset");
    expect(electrobunHost).toMatch(
      /mainWindow\.webview\.on\("dom-ready", \(\) => \{[\s\S]*?if \(process\.platform === "darwin"\) \{[\s\S]*?mainWindow\.setWindowButtonPosition\(16, 16\);[\s\S]*?\}\s*rendererReady = true;/u
    );
    expect(electrobunHost).toContain(
      'mainWindow.on("resize", () => mainWindow.setWindowButtonPosition(16, 16))'
    );
    expect(electrobunHost).not.toContain("ResizeObserver");
    expect(electrobunHost).not.toContain("getBoundingClientRect");
    expect(
      Array.from(
        electrobunHost.matchAll(/setWindowButtonPosition\(([^)]*)\)/gu),
        (match) => match[1]
      )
    ).toEqual(["16, 16", "16, 16"]);
  });

  test("routes custom titlebar double-clicks through the user's macOS window action", () => {
    expect(mainSource).toContain("installDesktopTitlebarDoubleClick(document");
    expect(mainSource).toContain(
      '!showDesktopPet && currentDesktopPlatform() === "macos"'
    );
    expect(containerSource).toContain("if (!desktopContainerAvailable)");
    expect(containerSource).toContain("return () => {");
    expect(containerSource).toContain("installTitlebarDoubleClick(document");
    expect(containerSource).toContain(
      "performTitlebarDoubleClick().catch(onError)"
    );
    expect(titlebarSource).toContain("electrobun-webkit-app-region-drag");
    expect(titlebarSource).toContain("electrobun-webkit-app-region-no-drag");
    expect(rpcSource).toContain(
      "titlebarDoubleClick: { params: undefined; response: boolean }"
    );
    expect(electrobunHost).toContain("titlebarDoubleClick: () =>");
    expect(electrobunHost).toContain(
      "performMacOSTitlebarDoubleClick(mainWindow.ptr)"
    );
    expect(nativeWindowEffects).toContain("AppleActionOnDoubleClick");
    expect(nativeWindowEffects).toContain("AppleMiniaturizeOnDoubleClick");
    expect(nativeWindowEffects).toContain("performMiniaturize:nil");
    expect(nativeWindowEffects).toContain("performZoom:nil");
    expect(nativeWindowEffects).toContain("screen.visibleFrame");
    expect(nativeWindowEffects).toContain(
      "setFrame:targetFrame display:YES animate:YES"
    );
  });

  test("reserves traffic-light space only on macOS", () => {
    expect(mainSource).toContain("document.documentElement.dataset.platform");
    expect(appSource).toContain("displayedRailCollapsed");
    expect(appSource).toContain('"window-controls-safe-main"');
    expect(appSource).toContain(': "pl-4"');
    expect(railSource).not.toContain("window-controls-safe-rail");
    expect(appSource).toContain("data-workspace-titlebar");
    expect(sceneStudioSource).toContain("window-controls-safe-scene");
    expect(styles).toMatch(
      /html\[data-platform="macos"\] \.window-controls-safe-main\s*\{[^}]*padding-left:\s*6rem/su
    );
    expect(styles).toMatch(
      /\.window-controls-safe-main\s*\{[^}]*padding-left:\s*1rem/su
    );
  });

  test("composes macOS sidebars as transparent blurred glass over an opaque workspace", () => {
    expect(electrobunHost).toContain(
      'transparent: process.platform === "darwin"'
    );
    expect(electrobunHost).toContain(
      'document.documentElement.classList.add("macos-window-glass")'
    );
    expect(styles).toMatch(
      /\.macos-window-glass \.app-shell\s*\{[^}]*background:\s*transparent;/su
    );
    expect(styles).toMatch(
      /\.macos-window-glass \.glass-rail\s*\{[^}]*--appearance-macos-panel-tint-opacity/su
    );
    expect(appSource).toContain(
      '"bg-background rounded-card shadow-raised @container/workspace m-2 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"'
    );
  });

  test("solidifies the glass panes while the app scheme disagrees with the system scheme", () => {
    expect(styles).toMatch(
      /\.macos-window-glass\.scheme-mismatch \.glass-rail[^}]*--appearance-macos-panel-tint-opacity:\s*100%/su
    );
    expect(themeSource).toContain(
      'root.classList.toggle("scheme-mismatch", scheme !== system)'
    );
  });

  test("uses an AppKit material to blur content behind the transparent window", () => {
    expect(electrobunHost).toContain(
      "configureMacOSWindowEffects(mainWindow.ptr)"
    );
    expect(nativeWindowEffects).toContain("NSVisualEffectView");
    expect(nativeWindowEffects).toContain(
      "NSVisualEffectBlendingModeBehindWindow"
    );
    expect(nativeWindowEffects).toContain("NSVisualEffectMaterialSidebar");
    expect(nativeWindowEffects).toContain(
      "NSVisualEffectStateFollowsWindowActiveState"
    );
    expect(nativeWindowEffects).toContain(
      "addSubview:backdrop positioned:NSWindowBelow"
    );
  });

  test("restores the native macOS window shadow disabled by transparent Electrobun windows", () => {
    expect(electrobunHost).toContain("windowEffectsStatus.shadow");
    expect(electrobunHost).toContain(
      "The macOS system window shadow could not be restored"
    );
    expect(electrobunConfig).not.toContain("libCodeTwoWindowEffects.dylib");
    expect(prepareElectrobun).toContain(
      'resolve(desktopRoot, "native", "window-effects")'
    );
    expect(patchMacOSInfo).toContain('"libCodeTwoWindowEffects.dylib"');
    expect(patchMacOSInfo).toContain("windowEffectsLibrary");
    expect(nativeWindowEffects).toContain("window.hasShadow = YES");
    expect(nativeWindowEffects).toContain("[window invalidateShadow]");
    expect(nativeWindowEffects).toContain(
      "dispatch_sync(dispatch_get_main_queue()"
    );
  });

  test("mounts the terminal in the shared right-side work dock", () => {
    expect(appSource).not.toContain("const [terminalOpen, setTerminalOpen]");
    expect(appSource).not.toContain('placement="bottom"');
    expect(appSource).toContain("availableSurfaces={availableDockSurfaces}");
    expect(appSource).toContain('case "toggle_terminal":');
    expect(dockSource).not.toContain("DockPlacement");
    expect(dockSource).toContain('data-dock-placement="right"');
    expect(dockSource).toContain('className="dock-tab-label"');
    expect(styles).toMatch(/@container dock \(max-width: 359px\)/u);
    expect(styles).toMatch(
      /\.glass-panel\s*\{[^}]*--appearance-sidebar-opacity/su
    );
  });

  test("gives the empty greeting spare space above the bottom composer", () => {
    expect(appSource).toContain(
      '"hero-scroll-shell order-2 min-h-0 flex-1 flex-col overflow-y-auto"'
    );
    expect(appSource).toContain("data-workspace-greeting");
    expect(appSource).not.toContain(
      "else if (railCollapsed) setRailCollapsedRaw(0)"
    );
    expect(appSource).toContain('usePersistedNumber("codetwo.docMode", 0)');
    expect(appSource).toContain("viewportWidth - APP_NAV_WIDTH");
    expect(appSource).toContain(
      "reservedWidth={railInlineWidth + APP_NAV_WIDTH}"
    );
  });

  test("stacks composer contributions above the full-page document", () => {
    expect(appSource).toContain(
      'docMode\n                                ? "order-1 min-h-0 min-w-0 flex-1 flex-col"'
    );
    expect(appSource).not.toContain(
      'docMode\n                  ? "order-1 min-h-0 min-w-0 flex-1"'
    );
  });

  test("keeps dock controls in the window strip and workspace actions in the pane header", () => {
    expect(styles).toMatch(
      /\.window-titlebar\s*\{[^}]*height:\s*var\(--ds-titlebar-height\);/su
    );
    expect(styles).toMatch(
      /\.window-titlebar\s*\{[^}]*box-shadow:\s*inset 0 calc\(-1 \* var\(--hairline-width\)\) 0 var\(--border\);/su
    );
    expect(appSource).toContain(
      '"session-header window-titlebar electrobun-webkit-app-region-drag flex min-w-0 shrink-0 items-center gap-2 pr-4"'
    );
    expect(appSource).toContain(
      'className="session-header-toolbar [&_svg]:text-muted-foreground gap-inline flex min-w-0 shrink-0 items-center"'
    );
    expect(sessionHeaderActionsSource).toContain(
      'className="session-header-actions gap-inline flex shrink-0 items-center"'
    );
    expect(sessionHeaderActionsSource).toContain("session-header-action-main");
    expect(styles).not.toMatch(
      /\.session-header-actions\s*\{[^}]*box-shadow:\s*inset 0 0 0 var\(--hairline-width\) var\(--border\);/su
    );
    expect(styles).toMatch(
      /\.session-header-context-label,[\s\S]*?\.session-header-layout-label,[\s\S]*?\[data-plugin-ui-slot="session\.header"\][\s\S]*?\.session-header-action-label\s*\{[\s\S]*?display:\s*none;/su
    );
    expect(styles).toMatch(
      /\.session-header-action-icon,[\s\S]*?\.session-header-context-icon,[\s\S]*?\.session-header-layout-icon,[\s\S]*?\[data-plugin-ui-slot="session\.header"\][\s\S]*?\[data-icon="inline-start"\]\s*\{[\s\S]*?display:\s*block;/su
    );
    expect(styles).toMatch(
      /\.session-header-context-main,[\s\S]*?\.session-header-plugin-action,[\s\S]*?\.session-header-layout-main\s*\{[^}]*width:\s*var\(--ds-control-normal\);[^}]*justify-content:\s*center;/su
    );
    expect(styles).toMatch(
      /@container session-header \(max-width: 48rem\)[\s\S]*?\.session-header-action-label\s*\{\s*display:\s*none;/su
    );
    expect(appSource).toMatch(
      /<EnvironmentPopover[\s\S]*?<SessionHeaderActions[\s\S]*?<PaneLayoutToolbar/u
    );
    expect(appSource).toContain('viewLabel={t("pane.viewMenu")}');
    expect(railSource).not.toContain("data-rail-header");
    expect(appSource).toContain("data-workspace-navigation");
    expect(appSource).not.toContain("railExpandAction");
    expect(styles).toMatch(
      /\.glass-rail\s*\{[^}]*box-shadow:\s*inset calc\(-1 \* var\(--hairline-width\)\) 0 0 var\(--sidebar-border\);/su
    );
    expect(appSource).toContain("data-dock-titlebar-host");
    expect(appSource).toContain("titlebarHost={dockTitlebarHost}");
    expect(dockSource).toContain("createPortal(header, titlebarHost)");
    expect(dockSource).toContain(
      'height: "var(--ds-workspace-windowbar-height)"'
    );
    expect(dockSource).toContain('size="icon-xs"');
    expect(dockSource).toContain("onClick={onClose}");
  });

  test("shows the session titlebar divider only when conversation content exists", () => {
    expect(appSource).toContain("const hasConversationContent =");
    expect(appSource).toContain(
      "turns.length > 0 || running || sessionLoading;"
    );
    expect(appSource).toContain("data-has-conversation={");
    expect(appSource).toContain('hasConversationContent ? "true" : undefined');
    expect(appSource).toContain("{hasConversationContent && (");
    expect(styles).toMatch(/\.session-header\s*\{[^}]*box-shadow:\s*none;/su);
    expect(styles).toMatch(
      /\.session-header\[data-has-conversation="true"\]\s*\{[^}]*box-shadow:\s*inset 0 calc\(-1 \* var\(--hairline-width\)\) 0 var\(--border\);/su
    );
  });

  test("uses the shared compact toolbar treatment for dock tabs", () => {
    expect(dockSource).toMatch(/<TabsList\s+variant="toolbar"/u);
    expect(dockSource).not.toContain("data-[state=active]");
    expect(tabsSource).toContain("toolbar:");
    expect(tabsSource).toContain(
      "group-data-[orientation=horizontal]/tabs:data-[variant=toolbar]:h-control gap-1 bg-transparent p-0"
    );
    expect(tabsSource).toContain(
      "group-data-[variant=toolbar]/tabs-list:data-active:bg-fill-selected"
    );
    expect(tabsSource).toContain(
      "group-data-[variant=toolbar]/tabs-list:data-active:text-primary"
    );
  });

  test("keeps the sidebar resize target invisible on hover", () => {
    expect(styles).toMatch(
      /\.rail-grip\s*\{[^}]*width:\s*6px;[^}]*cursor:\s*col-resize;/su
    );
    expect(styles).not.toMatch(/\.rail-grip(?:::after|:hover)/u);
  });

  test("aligns the visible dock resize affordance with the panel edge", () => {
    expect(styles).toMatch(
      /\.dock-grip::after\s*\{[^}]*left:\s*0;[^}]*width:\s*2px;/su
    );
  });
});
