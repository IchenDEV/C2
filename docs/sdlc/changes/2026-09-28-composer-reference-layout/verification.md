---
id: 2026-09-28-composer-reference-layout
schema: 5
stage: verification
status: passed
owner: Codex
created: 2026-09-28
based_on: plan.md
revision: "ba9102a8 plus the scoped composer-reference-layout worktree diff"
verification_mode: owner
verified_by: Codex
verified_at: 2026-09-28
release_target: none
cleanup_status: complete
---

# Verification: Composer Reference Layout

## Verification

- AC-1: PASS — `cua_repl` rendered the actual App through the task-owned Vite renderer at 127.0.0.1:1438. Checked 1280×720 light, 800×600 light/dark, and Ocean palette. Real Composer with deterministic provider props additionally checked at 400×600: configuration wraps above right-aligned actions, checkout/branch wrap without clipping. Screenshots retained in `/private/tmp/codetwo-composer-reference/` (composer-light.png, narrow-light.png, narrow-dark.png, ocean.png, component-narrow.png).
- AC-2: PASS — `bun test tests/composerLayoutRendered.test.tsx tests/composerGeometryContract.test.ts tests/shellChromeContract.test.ts tests/sceneChip.test.tsx tests/checkoutPickerRendered.test.tsx tests/composerDrafts.test.ts tests/modelFavorites.test.ts tests/modelPreferences.test.ts --timeout 15000` passed 49 tests / 232 assertions. `cua_repl` also filtered the rendered real ModelPicker to Review fast and selected it, observing the trigger update; model-search.png records the filtered list. That menu check used fixture metadata, not a live provider.
- AC-3: PASS — The same test run verified editor identity and draft preservation, direct disabled permission control without duplication, attachment availability and send/loading/stop callbacks. `cua_repl` entered Chinese text into the actual BlockNote editor and observed it after expansion and collapse. `bun run build:renderer -- --outDir /private/tmp/codetwo-composer-reference/build` passed lint, TypeScript and Vite production compilation. `bun script/verify/docs.ts`, `bun script/verify/sdlc.ts --worktree`, and `git diff --check` passed.

- AC-4: PASS — `bun test tests/windowChromeContract.test.ts tests/sessionHeaderActionsRendered.test.tsx tests/shellChromeContract.test.ts --timeout 15000` passed 31 tests. Actual light/dark toolbar rendering shows matching text and icon colors. Open remained theme-muted while hovered and expanded; disabled source control retained 0.5 opacity. See toolbar-color follow-up below.

- AC-5: PASS — `bun test tests/sessionRailRendered.test.tsx tests/windowChromeContract.test.ts --timeout 15000` passed 46 tests / 390 assertions. Actual light/dark rendering confirmed matching navigation text/icon colors, hover tone and persistent selected fill/marker. Collapse/expand remained functional.

- AC-6: PASS — `bun test tests/composerGeometryContract.test.ts tests/composerLayoutRendered.test.tsx tests/checkoutPickerRendered.test.tsx tests/sceneChip.test.tsx --timeout 15000` passed 29 tests / 148 assertions. Actual compact/document and light/dark controls share the muted toolbar color; stylesheet scope preserves non-neutral semantic variants.

Verdict: verified.
Residual risk: browser renderer and deterministic component interactions were verified; no native Core/provider turn, remote CI, Windows rendering, merge or release was exercised during local verification. Vite reported existing large-chunk advisories. Initial checks exposed obsolete geometry assertions and test fixture labels, corrected before the passing run. Development reloads invalidated the first draft observation; the stable subsequent expand/collapse check retained the text.

## Contrast follow-up — 2026-09-28

User review rejected the previous visual contrast. The earlier screenshots remain prior layout evidence, superseded for contrast by contrast-light.png, contrast-composer.png and contrast-dark.png in the same screenshot directory. The composer now uses the existing muted surface token, the footer uses rest, primary controls use full foreground, and chevrons no longer attenuate opacity. Permission warning colors and disabled behavior are preserved. Placeholder text is upright.

`cua_repl` inspected actual computed styles and screenshots: light card `oklch(0.954845 0.000289743 none)` with primary text `rgb(32,33,35)`; dark card `oklch(0.319455 0.00766666 none)` with primary text `rgb(242,244,248)`. Placeholder is normal style and fully opaque. Light placeholder `oklch(0.49583 0.00272918 none)`, dark placeholder `oklch(0.74 0.008 260)`. Both schemes show clear input/canvas separation.

`bun test tests/composerGeometryContract.test.ts tests/composerLayoutRendered.test.tsx tests/checkoutPickerRendered.test.tsx tests/shellChromeContract.test.ts --timeout 15000` passed 20 tests / 117 assertions. `bun run lint:styles` and `bun run build:renderer -- --outDir /private/tmp/codetwo-composer-reference/build` passed (lint, types, compilation). Remaining prior behavior evidence is unchanged. Documentation, SDLC and diff checks were rerun after cleanup.

Follow-up cleanup: Vite session 27002 stopped, port 1438 checked, preview tab closed and C2/System restored. Removed the new 49 MB build and build.log. Nine screenshots remain under the existing Codex ownership and next-continuation cleanup checkpoint; prior screenshots document the rejected comparison baseline.

## White-surface follow-up — 2026-09-28

The latest user instruction supersedes the gray fill. The input now keeps the canvas surface (computed `rgb(255,255,255)` in the default light theme), with a composer-only 1px neutral edge and two fixed soft shadows. Dark mode retains the raised surface and a theme-adjusted neutral edge. The shared panel/menu shadows remain unchanged. The edge is painted through the composer elevation token, preserving the existing geometry and block handles.

`cua_repl` checked the actual App at 1280×720 and 800×600, light/dark. Focused and unfocused composer both measured 768×160 with identical white background and box-shadow. Light shadow values: 1px edge, `0 8px 24px rgba(16,24,40,0.1)` and `0 2px 6px rgba(16,24,40,0.04)`. Current evidence: white-full.png, white-narrow.png and white-dark.png under `/private/tmp/codetwo-composer-reference/`. Previous gray screenshots are comparison history, not the final design.

`bun test tests/composerGeometryContract.test.ts tests/composerLayoutRendered.test.tsx tests/checkoutPickerRendered.test.tsx tests/shellChromeContract.test.ts --timeout 15000` passed 20 tests / 117 assertions; `bun run lint:styles` passed. Renderer lint, TypeScript and production build passed after formatting the updated geometry assertion. Documentation/SDLC/diff checks passed after cleanup. Earlier provider and draft behavior evidence remains applicable.

Cleanup: stopped task Vite session 97621; port 1438 has no listener. Closed the preview, reset viewport, restored System theme. Removed the 49 MB build, build.log and the unusable cropped screenshot. The three new screenshots remain under Codex ownership for the next continuation review; no Core or user process was touched.

## Half-pixel edge refinement — 2026-09-28

User requested 0.5px. Updated only the composer edge spread in all light/dark token declarations; fill, shadow blur/offset/opacity and geometry are unchanged. `cua_repl` confirmed the actual white card is `rgb(255,255,255)` and its computed shadow edge is `0px 0px 0px 0.5px`; screenshot: `/private/tmp/codetwo-composer-reference/half-pixel-edge.png`. `bun run --cwd apps/desktop lint:styles`, documentation/SDLC gates and `git diff --check` passed. Existing behavior/build evidence is reused for this CSS-number-only refinement; Vite rendered the current stylesheet successfully.

Cleanup: Vite session 78352 stopped, port 1438 verified released, task tab closed. Only the new screenshot was added to retained evidence under the same owner and next-continuation review checkpoint; no build output created.

## Toolbar-color follow-up — 2026-09-28

The user explicitly chose a uniform soft medium gray. Removed the icons-only toolbar color utility and assigned the existing theme-muted color to toolbar text, buttons, interaction states and icons through one scoped CSS rule. Portalled menus retain their own styling and disabled controls retain their existing opacity and behavior.

`cua_repl` checked the actual App toolbar in light/dark at 1280×720. All six toolbar buttons and their icons share `oklch(0.49583 0.00272918 none)` in light and `oklch(0.718376 0.00647301 none)` in dark. The Open button retained the light muted color while hovered and expanded. Disabled source control kept 0.5 opacity. Screenshots: `/private/tmp/codetwo-composer-reference/toolbar-gray-light.png` and `toolbar-gray-dark.png`. The white composer and 0.5px edge remain unchanged.

`bun test tests/windowChromeContract.test.ts tests/sessionHeaderActionsRendered.test.tsx tests/shellChromeContract.test.ts --timeout 15000` passed 31 tests / 210 assertions. Final CSS passed stylelint. Renderer lint, TypeScript and production build passed. Documentation/SDLC checks and diff inspection passed after cleanup; earlier composer behavior evidence remains applicable.

Cleanup: stopped task Vite session 12272 (exit 130), confirmed port 1438 released, restored System theme and closed the task preview tab. Removed the temporary renderer build and build.log. Retain the two screenshots under the existing ownership and next-continuation checkpoint. No Core or user process was touched.

## Sidebar-color follow-up — 2026-09-28

Extended the existing muted foreground to left window controls, icon navigation and task-sidebar roots/buttons. Status descendants keep their own semantic colors; selection still uses background and the existing marker. No state or behavior code changed.

Actual renderer inspection through `cua_repl` confirmed all 15 visible left controls and their icons share the toolbar tone: light `oklch(0.49583 0.00272918 none)`, dark `oklch(0.718376 0.00647301 none)`. The hovered New task button retained the same light tone. Dark selected Chats retained its neutral fill and fully opaque selection marker. Collapsed the task sidebar, observed the icon navigation remain available, then expanded it. Screenshots: `/private/tmp/codetwo-composer-reference/sidebar-gray-light.png` and `sidebar-gray-dark.png`. Rail rendered tests additionally cover populated task/project lists and state badges; the preview used the empty local renderer dataset.

46 relevant rail/chrome tests and stylelint passed. Reused previous renderer lint/types/build evidence for this CSS-only change; Vite compiled and rendered the current stylesheet. Documentation/SDLC/diff checks passed after cleanup.

Cleanup: task Vite session 55113 stopped (exit 130), port 1438 has no listener, task browser tab closed, System theme and expanded sidebar restored. No Core launched. No build or scratch source created; the two new screenshots remain under Codex ownership until next continuation review. Existing screenshots remain comparison evidence for user refinements.

## Composer-toolbar color follow-up — 2026-09-28

The user's screenshot extends the medium-gray direction to composer configuration, neutral actions and checkout. This supersedes the earlier foreground-label contrast choice. One scoped rule owns neutral Ghost/secondary button color; warning chips are excluded, recording uses destructive, and nonempty submit uses default. Checkout label now inherits its button color. White surface, elevation and geometry are unchanged.

`cua_repl` verified all ten visible neutral buttons, icons and labels share `oklch(0.49583 0.00272918 none)` in light and `oklch(0.718376 0.00647301 none)` in dark. Document-mode controls use the same light tone. Expanded session settings retained the muted color. Screenshots: `/private/tmp/codetwo-composer-reference/composer-gray-light.png` and `composer-gray-dark.png`. Permission/recording/filled-submit exclusions were inspected in source; no real permission grant, microphone recording or provider submission was performed.

29 relevant rendered/contract tests and stylelint passed. The first renderer build identified formatting in the two changed source files; fixed with the repository formatter. Renderer lint, TypeScript and production build then passed. Documentation/SDLC gates and diff checks passed after cleanup. Previous composer draft/attachment/provider evidence remains applicable.

Cleanup: task Vite session 20768 stopped (exit 130), port 1438 checked free, task preview closed, compact mode/System theme restored. Removed the 49 MB temporary build and build.log. Retain the two screenshots under Codex ownership until next continuation review. No Core or user process was touched.

## Cleanup

Removed: task-only `apps/desktop/composer-review.html`, `apps/desktop/composer-review.tsx`, and the 49 MB temporary renderer build under `/private/tmp/codetwo-composer-reference/build`.
Retained: visual evidence screenshots in `/private/tmp/codetwo-composer-reference/`; frozen-lockfile dependencies and Vite dependency cache in `apps/desktop/node_modules` (1.7 GB) for this checkout.
Retention owner: Codex.
Cleanup trigger: review screenshots at the next continuation and remove superseded evidence; dependency/cache lifecycle ends when this worktree is retired.
Processes: task Vite session 51750 stopped through its launcher; `lsof -nP -iTCP:1438 -sTCP:LISTEN` returned no listener. Both task browser tabs closed, viewport reset, and app preview theme restored to C2/System. No Core process was launched or user process stopped.
Evidence: `du -sh /private/tmp/codetwo-composer-reference apps/desktop/node_modules`, exact scratch-file listing, launcher exit 130, port inspection, and final `git status --short` inventory. The retained screenshot directory is 136 KB after cleanup.

## Review and release

Approval: local implementation and PR delivery authorized by the user requests recorded in Intent; no human review or merge approval claimed.
Rollback: See plan.md.
Release: PR delivery requested; merge and release are not authorized. Remote CI results belong to GitHub and are not implied by local verification.

PR preflight: fetched origin/main at ba9102a8988a4a46e8d827a17aca22f66835e5bb, identical to the implementation base. Reused current rendered/test/build evidence. No preview or build process remains; local screenshots remain review evidence under the recorded retention owner/checkpoint.
