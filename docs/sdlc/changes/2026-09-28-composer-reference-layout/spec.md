---
id: 2026-09-28-composer-reference-layout
schema: 5
stage: spec
status: accepted
owner: Codex
created: 2026-09-28
based_on: intent.md
---

# Spec: Composer Reference Layout

## Design

Reuse Composer, ModelPicker, SessionModePicker and CheckoutBar as the unique state owners. Recent shell cleanup put checkout above the card and permission inside secondary settings. This is a local presentation issue: changing composition costs less to implement, validate and maintain than replacing these stateful modules or rewriting the editor. No data migration or transaction changes.

Keep the editor subtree mounted across compact/document mode. Move checkout after the card, using existing surface tokens and 24px radius. Keep permission directly visible; secondary settings contain memory and worktree where applicable. Use a named provider icon rail, preserving search, favorites and callbacks. Wrap controls at narrow widths.

## Acceptance criteria

- [x] AC-1: Rendered light, dark and narrow layouts have more writing room, direct model/reasoning/permission controls, right-aligned actions and an inset checkout footer. The default light input stays white; a fixed neutral 0.5px edge and soft shadow distinguish it from the reading canvas without changing geometry on focus/hover; neutral controls use theme-muted gray and chevrons do not attenuate opacity.
- [x] AC-2: Provider/model search and switching, reasoning, permission disabling, checkout and secondary settings preserve existing behavior without duplicate permission controls.
- [x] AC-3: Expand/collapse preserves the editor and draft; attachment, send/loading and running controls remain available. Applicable desktop and repository checks pass.

- [x] AC-4: Session-header text and icons share the theme-muted gray in rest, hover and open states; disabled actions retain their reduced-opacity and inoperable state. Composer styling stays unchanged.

- [x] AC-5: Left window controls, icon navigation and task sidebar use the same theme-muted text/icon tone as the session toolbar. Hover and selection remain identifiable through surfaces, markers and focus; status badges preserve semantic colors.

- [x] AC-6: Composer configuration, neutral action icons and checkout labels share the toolbar medium gray in compact/document modes and interaction states. White fill and 0.5px edge remain; warnings, recording and filled submit states preserve their semantic colors.
