---
id: 2026-09-28-codex-layout-reset
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-09-28
based_on: intent.md
---

# Spec: Codex Layout Reset

## Design

Keep App as the single owner of page, pane and sidebar state. SessionRail composes a persistent icon navigation beside the existing resizable task list; collapsing the list leaves global navigation accessible. A separate window drag strip reserves native controls. Existing handlers remain authoritative; no second routing system.

The previous quiet-shell change adjusted styling but left global destinations mixed with task rows. A local CSS patch cannot separate their visibility/scroll ownership. Replace that composition boundary only; a full App rewrite would unnecessarily migrate session and draft state. Reuse shared controls and semantic tokens without new dependencies. Keep Composer at the same React tree position across document/compact modes. Empty-state heading takes spare space; the input stays at the bottom, with scrolling at short heights. Include navigation width in dock/overlay calculations.

## Acceptance criteria

- [x] AC-1: Persistent icon navigation reaches existing destinations when the task sidebar is collapsed, with accessible labels and selected states; task/new/search/quick-chat controls remain usable.
- [x] AC-2: New-task greeting is centered in available space above the bottom composer; draft survives compact/document toggles and sidebar toggles.
- [x] AC-3: Rendered light, dark and narrow layouts keep controls within the window; sidebar overlay and dock sizing reserve navigation space.
- [x] AC-4: Affected tests, desktop lint/types/renderer build, documentation and worktree scope checks pass; verification distinguishes browser rendering from native/runtime acceptance.

- [x] AC-5: The existing shell remains intact while the dock picker uses a centered flat list, toolbar actions use shared quiet states, and composer focus does not inflate its shadow. Names, selection, focus and disabled states remain clear; descriptions remain available from each panel entry. Verify light/dark and narrow rendering.

- [x] AC-6: Composer and right tool panel use restrained constant raised surfaces. The right panel has an 8px gutter, remains resizable, and reserves no space when closed. Preserve draft and mode behavior; verify light/dark and narrow bounds.

- [x] AC-7: The main workspace and right panel share visible 16px corners with content clipped inside; the workspace has an 8px inset and retains flat elevation. Confirm actual rendering at normal and narrow widths.

- [x] AC-8: Main workspace, right tool panel and composer use the same theme-managed raised shadow, constant across interaction states. This supersedes AC-7's flat workspace elevation. Verify computed shadows and actual light/dark rendering.

- [x] AC-9: Workspace window strip is 32px with the existing focused title; panel headers are 40px. Toolbar labels collapse below 48rem while actions retain names. Selected dock tabs retain text in narrow panels. Verify actual normal/narrow light/dark rendering and existing action routes; native control placement and dedicated page geometry remain unchanged.

- [x] AC-10: Search and sidebar toggle have one persistent home in the window strip, including collapsed/overlay states. Remove the rail header and duplicate pane expand controls; feature pages show no brand fallback. Search and collapse/expand remain usable at normal/narrow widths and controls clear native window space.

- [x] AC-11: Shared raised elevation is lighter: 0 2px 8px at 4% opacity in light mode and 12% in dark mode. Main workspace, dock and composer remain equal; confirm actual light/dark rendering. Shared raised menus/popovers inherit the same restrained treatment.

- [x] AC-12: Shared icon-size buttons are circular. Workspace icon-only controls and inactive icon-only dock tabs remain square in bounds with full rounding, including responsive headers; text buttons and joined split controls retain their shape. Verify actual light/dark rendering and computed geometry.

- [x] AC-13: Empty new-task panes omit the duplicate title and orphan breadcrumb divider. Plugin actions above the composer/transcript use one flat row with named buttons and hover/focus descriptions; existing dispatch and busy states remain. Empty-sidebar copy is concise. Verify actual light/dark normal/narrow rendering and affected tests.

- [x] AC-14: Empty sidebar shows one muted, transparent Add project row with a plus icon aligned to task actions. Remove redundant empty-state copy while retaining the existing click handler and keyboard focus. Inspect actual light/dark rendering. This supersedes AC-13 empty-sidebar copy.

- [x] AC-15: Dock tabs and close control occupy the global 32px titlebar aligned with the panel width; no duplicate inner panel header. Existing tab selection, content ownership, resizing and close/reopen behavior remain. Closed docks expose no header controls. Verify actual light/dark normal/narrow layouts and keyboard tab navigation. Supersedes AC-9 dock header placement.

- [x] AC-16: Titlebar tabs and close control use 28px height centered in the 32px strip. Adjacent workspace/dock panels have one 8px gap; closed layout retains its outer inset. Checkout context uses 8px side inset within the composer measure; plugin-to-checkout spacing is 8px. Verify normal/narrow rendering and preserved keyboard/close behavior. Supersedes earlier summed panel gutters and 32px inactive tab geometry.

- [x] AC-17: Files browser with no open documents shows only its search/action toolbar, without the redundant folder-only navigation row or stacked dividers. Search and action controls share 32px height and 8px toolbar padding. Open documents retain their file tabs and return-to-tree action; closing the last document removes the empty tab row. Verify rendered empty/populated tab states and light/dark normal/narrow layouts.
