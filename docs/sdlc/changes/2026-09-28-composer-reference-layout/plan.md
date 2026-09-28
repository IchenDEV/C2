---
id: 2026-09-28-composer-reference-layout
schema: 5
stage: plan
status: accepted
owner: Codex
created: 2026-09-28
based_on: spec.md
scope: apps/desktop/src/App.tsx, apps/desktop/tests/windowChromeContract.test.ts, Design.md, docs/design/system.md, apps/desktop/src/design/tokens.css, apps/desktop/src/styles.css, apps/desktop/tests/composerGeometryContract.test.ts, apps/desktop/tests/checkoutPickerRendered.test.tsx, apps/desktop/src/components/ui/icons.tsx, apps/desktop/src/session/Composer.tsx, apps/desktop/tests/sceneChip.test.tsx, apps/desktop/tests/shellChromeContract.test.ts, apps/desktop/tests/composerLayoutRendered.test.tsx, docs/sdlc/changes/2026-09-28-composer-reference-layout/
---

# Plan: Composer Reference Layout

## Plan

Codex owns layout and regression tests. Reuse menus and tokens without new dependencies or parallel state. Update the design contract for footer placement. Run affected rendered component, provider/model, permission, checkout and draft tests, desktop lint, TypeScript and renderer build, documentation and worktree SDLC gates. Inspect the actual app renderer in light/dark/narrow states and exercise draft retention and menus. Backend, packaging and release checks are outside this presentation-only scope.

Temporary resources: task Vite renderer on 1438 and /private/tmp/codetwo-composer-reference/. Stop the server and remove scratch outputs at handoff; retain a small screenshot set for review, owned by Codex until next continuation. No Core launch or user data ownership. Retain frozen-lockfile desktop dependencies until worktree retirement.

Rollback: revert scoped layout, tests and design wording; no persisted state migration.

Contrast follow-up: user reported insufficient contrast in the rendered result. Reuse the existing muted/rest surface tokens, foreground text and opaque chevrons; preserve permission warning tones. Recheck actual light/dark rendering and measured text contrast, affected geometry tests, lint/types/build and repository gates.

White-surface follow-up: user explicitly requests retaining the white main input and improving separation through shadow/border. Use one composer elevation token with a geometry-preserving 0.5px edge and fixed layered shadow; restore the canvas/raised background for light/dark. Scope the exception to Composer and its floating document toolbar. Check actual light/dark/narrow and focus rendering, affected tests, style lint and renderer build.

Toolbar color: the existing parent colors icons only while text inherits foreground from Ghost buttons. Resolve the inconsistency at the toolbar CSS owner, including control states; remove the icons-only parent utility. This localized composition fix is sufficient, without changing global buttons or palette values. Check header behavior/contracts, style lint, renderer lint/types/build and actual light/dark rendered text/icon colors. Reuse unaffected composer evidence.

Sidebar tone: the existing rail mixes inherited foreground, Ghost/selectable button foreground and muted utilities. Scope the shared muted color to existing left navigation roots and buttons, retaining status descendants and selection geometry. A CSS-only owner fix avoids replacing stateful rail modules. Verify actual light/dark rendering, selection/collapse, relevant rail tests and style lint; reuse unchanged renderer type/build evidence.

Composer toolbar tone: use existing controls/actions/checkout boundaries and neutral button variants as the CSS owner. Remove the checkout label’s foreground override so it inherits this tone. Exclude warning chips and non-neutral recording/submit variants. Check real light/dark compact/document rendering, warning and send states, affected rendered tests, style lint and renderer build.
