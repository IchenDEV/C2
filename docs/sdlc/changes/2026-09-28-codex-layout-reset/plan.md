---
id: 2026-09-28-codex-layout-reset
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-09-28
based_on: spec.md
scope: apps/desktop/src/files/FileDockContent.tsx, apps/desktop/src/files/FilePanel.tsx, apps/desktop/tests/fileDockContentRendered.test.tsx, apps/desktop/src/design/ui-lab/UiLab.tsx, apps/desktop/src/plugins/PluginUiSlot.tsx, apps/desktop/tests/pluginManagerRendered.test.tsx, apps/desktop/src/components/ui/button.tsx, apps/desktop/src/design/tokens.css, docs/design/system.md, apps/desktop/src/dock/Dock.tsx, apps/desktop/src/session/SessionHeaderActions.tsx, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/dockPluginGateRendered.test.tsx, apps/desktop/tests/sessionHeaderActionsRendered.test.tsx, apps/desktop/tests/composerGeometryContract.test.ts, apps/desktop/src/App.tsx, apps/desktop/src/sidebar/SessionRail.tsx, apps/desktop/src/styles.css, apps/desktop/src/session/Composer.tsx, apps/desktop/tests/sessionRailRendered.test.tsx, apps/desktop/tests/windowChromeContract.test.ts, apps/desktop/tests/checkoutPickerRendered.test.tsx, Design.md, docs/sdlc/changes/2026-09-28-codex-layout-reset/
---

# Plan: Codex Layout Reset

## Plan

Codex owns the shell composition, affected tests and design contract. Move existing feature/utility controls to a persistent icon strip, compact the task header/search, add window drag strip, and bottom-align compact composition without remounting it. Preserve task organization, pane and provider logic.

Check existing rendered rail interactions and shell/composer/dock contracts, desktop lint/types and renderer build. Inspect the actual renderer in light/dark/narrow states and exercise navigation, sidebar and composer draft retention. Run documentation and SDLC worktree checks. Core, provider execution, packaging and remote CI are outside this renderer-only change.

Temporary resources: task-owned renderer server on 127.0.0.1:1428 and `.codex/run/codex-layout-reset/` for compact visual evidence. Stop the server before handoff, remove disposable build outputs; retain screenshots for this review until the next continuation. Installed workspace dependencies are shared reusable tooling.

Rollback: revert only this change's scoped files; no persisted schema or state migration.

Follow-up: reuse NavigationRow for flat panel choices, remove the old responsive card anatomy, use existing Ghost button states and keep one constant composer elevation. No new visual tokens, routing, dependencies or backend changes. Reuse valid previous behavior evidence and recheck the affected renderer suites and actual light/dark/narrow UI.

Floating-panel follow-up: reuse the existing raised Card for the dock, keeping its gutter inside the current width allocation. Apply existing raised tokens to the composer and expanded control bar. Do not add drag-window state, new tokens or another surface system. Update the explicit design exceptions and rerun affected contracts plus rendered checks.

Corner refinement: use existing rounded-card and overflow clipping on the main workspace, with the same 8px inset as the tool panel. Preserve the editor tree and existing 24px composer corners. This cosmetic adjustment needs existing checks and rendered inspection, not new behavior tests.

Shadow refinement: apply the existing shadow-raised utility to the main workspace; the dock and composer already use that token. Keep radius, gutters, backgrounds and all interaction state unchanged. Update the existing chrome contract and verify actual computed shadows and light/dark rendering.

Top chrome refinement: add scoped geometry tokens, reuse activeTitle in the drag strip and preserve pane titles for split layouts. Compact the workspace headers only, raise the toolbar label breakpoint and retain selected dock-tab text. Existing handlers and tab ownership remain authoritative; no window-manager or native changes. Verify chrome/action/dock contracts, build/style checks and actual rendering.

Sidebar chrome follow-up: reuse IconAction in the window strip with existing search and toggle handlers. Remove obsolete rail props/header/styles and per-page duplicate expansion entries. Reserve 168px minimum action-group width for native controls; align to inline sidebar width when expanded. Verify rail/chrome tests, live search, collapse/expand, overlay and feature-page navigation; no native code changes.

Shadow weight follow-up: reduce the existing raised elevation token in root/light/dark definitions. Reuse the established shared surface contract instead of creating panel-specific overrides; menus/popovers also inherit the lighter token. No geometry or interaction changes. Verify style/build checks and live shadow equality in both themes; reuse prior behavior evidence.

Icon shape follow-up: change the shared icon-size Button variants to rounded-full, remove the Quick Chat radius override and migrate dock close buttons to icon size. Responsive toolbar icons use square bounds and full rounding; connected split-button segments are excluded. Inspect rendered rest/focus/selected states and existing control/rail/dock contracts; no behavior changes or new test suite.

Visual simplification: merge composer-above and transcript-before action presentation at the existing PluginUiSlot boundary. Remove nested cards, duplicate Run controls and always-visible descriptions; reuse Ghost buttons and Tooltip. Omit only the new-task pane title already represented by the strip and greeting. Shorten empty-sidebar copy, retaining Add project. Keep routing, slots and persisted state unchanged.

Project-entry refinement: replace the empty-state block with one shared Ghost row, using the existing plus icon and onAddProject handler. Remove unused empty-copy translations; reuse the existing click test and verify actual rendering.

Dock titlebar refinement: reuse a shell-owned DOM destination and React portal for the existing Dock tab controls, preserving their Tabs context and content tree. Dock remains the width and tab-presentation owner; portaled chrome uses its applied width and resize state. Share one header for home/selected modes; no duplicate top-bar state or new tab system. Verify portal tab/close behavior and shell/dock contracts plus live rendering.

Spacing refinement: remove the doubled workspace/dock margin only while open, remove the extra page inset inside CheckoutBar, and reuse mini-control height for window-strip tabs. Reduce stacked plugin/composer spacing through the existing owner classes; no new layout state or spacing tokens. Recheck affected chrome/composer/dock/checkout contracts, style/build checks and actual normal/narrow bounds.

File-panel refinement: condition the existing document-tab row on actual open documents or an active editor, retaining the tree-return control when needed. Remove the empty folder-only row and decorative divider. Use shared compact Input and normal icon Button sizes rather than a conflicting height override; apply one 8px toolbar inset. Add a focused rendered check for empty/populated document navigation and reuse dock checks.
