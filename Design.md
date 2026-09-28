# C2 Desktop Design Standard

Status: accepted, 2026-09-09. Applies to the desktop renderer. This file owns visual design decisions; [the component contract](docs/design/system.md) owns token architecture, reusable-component APIs and implementation constraints. Existing release and platform verification boundaries remain unchanged. Values below are C2 decisions, not extracted Codex or ChatGPT internals.

## Scope and intent

Make controls compact, reading comfortable, navigation identifiable and repeated layouts predictable. This standard covers the workspace/composer, session rail, settings, shared buttons, fields, menus, tabs, dialogs and the panels composed from them. Preserve existing capabilities, permissions, provider routing, saved preferences and task data. No navigation feature is removed. Low-frequency options remain in the existing settings/popover controls.

## Shape and hierarchy

- Minimum control/menu-item radius: **12px**, explicitly confirmed by the user. Cards and dialogs: **16px**. Main composer: **24px**. Standalone icon-only buttons are fully circular at every size; text buttons keep the control radius. Responsive toolbar buttons become circular when their label is hidden; joined split-button segments retain their shared outline. Small marks naturally clamp their radius.
- Persistent content surfaces are flat, except the floating main workspace, composer and right tool panel, which use the shared raised elevation. Menus and dialogs may use the shared elevation/material. Hover and press never translate, scale, glow or increase shadow.
- Keep one primary action per local group. Use a neutral surface for ordinary controls and C2 blue for primary actions and links. Success, warning and destructive colors convey actual status.
- Page title, explanatory copy, content and action appear in that order. Headings and actions use the same content alignment grid.

## Color and interaction states

Default light palette: background `#FFFFFF`, foreground `#202123`, accent `#356DE6`. Default dark palette: background `#18191D`, foreground `#F2F4F8`, accent `#77A7FF`. Custom palettes remain supported by the same resolver; no page hardcodes these values.

Neutral fills are generated with `color-mix(in oklch, foreground N%, background)`. This defines exact behavior across palettes rather than maintaining a competing list of approximate hex colors.

| Role | Light N | Dark N | Purpose |
| --- | ---: | ---: | --- |
| quiet | 2.5 | 4 | Low-emphasis/read-only region |
| rest | 4 | 6 | Persistent input or secondary control |
| hover | 7 | 12 | Pointer or menu highlight |
| selected | 12 | 18 | Persistent current item |
| selected-hover | 16 | 23 | Current item under pointer |
| pressed | 20 | 28 | Pointer held down |

- Ordinary ghost, secondary and legacy outline buttons share this neutral ladder. Do not tint routine hover blue or multiply a state by opacity.
- Selection survives hover and mouse exit. Navigation exposes `aria-current` and a persistent neutral leading mark, so macOS vibrancy cannot hide the current item; tabs and menu selections retain their indicator/checkmark. Hover alone does not imply selection.
- Primary hover and press mix the foreground into the accent by 8% and 16%, preserving the paired foreground. Destructive actions keep their own semantic color pair.
- Keyboard focus is an independent **2px neutral high-contrast indicator**. It can coexist with selection and hover. Keep the existing neutral focus convention.
- Disabled controls do not respond to hover/press and remain inoperable. Their existing 50% opacity treatment remains; do not use it for normal supporting text.
- Ordinary text and essential descriptions must reach 4.5:1 against their actual surface. Do not stack opacity on supporting text or editor placeholders. Custom themes must be checked rather than assumed conformant.
- Feedback duration: **120ms**; respect Reduced Motion. A row without a primary action does not get an interactive hover treatment.

## Typography

Use the system UI stack (`-apple-system`, `BlinkMacSystemFont`, `Segoe UI`, sans-serif). Keep the user's selected family and scale. Code stays independently configurable; preserve saved code sizes.

| Role | Default size / line height | Weight |
| --- | --- | --- |
| Large title | 28 / 34px | 600 |
| Page title | 20 / 28px | 600 |
| Content section | 18 / 24px | 600 |
| Dialog title | 16 / 22px | 600 |
| UI / controls | 14 / 20px | 400; emphasis 500 |
| Reading / editor prose | **15 / 24px** | 400 |
| Supporting copy | 13 / 20px | 400 |
| Metadata | 12 / 16px | 400 |
| Caption / keycap only | 11 / 14px | 400 |
| New-install code default | **13 / 20px** | 400 |

The typography resolver owns derived sizes; product components never calculate a second scale. Group headings in settings use the UI role with emphasis, not spaced-out uppercase metadata. Task titles remain legible even when metadata is compact.

## Spacing and geometry

Use the existing 2/4/6/8/12/16/24/32px scale: icon/text 6–8px, related controls 8px, label/field 8px, rows 16px combined separation, groups 24–32px, page inset 24px. Preserve 28/32/36px control heights and 32px navigation rows. Responsive layouts may stack fields; they must not hide labels or force horizontal scrolling.

- Expanded document content and its floating action bar share a **48rem outer measure**, centered in the available column with 24px side insets. Preserve the editor's block-handle gutter and enough bottom clearance for every line to scroll above the action bar.
- Compact composition remains a bounded input card. Expansion must reuse the same editor tree and preserve its draft.
- Settings use a consistent 24px page inset, aligned trailing controls and section grouping. Narrow content columns stack controls below labels.
- Session names lead the rail. Supporting text is bounded to one line; status, age and provider remain aligned. Routine row actions appear on hover/focus. External resource groups default collapsed only when no saved preference exists.
- The Files surface shows document tabs only when documents are open or an editor needs a route back to the tree. Its search/action toolbar uses 32px controls with 8px padding and no extra folder-only navigation row.
- File/diff/terminal panels retain task-appropriate density; shared chrome and controls follow this standard without constraining code to a prose column.

## Workspace layout

The main workspace has a 32px window drag strip with the focused task title above three independent regions: a persistent
48px icon navigation, the resizable task sidebar, and the content workspace with its optional dock.
The window strip owns search and the sidebar toggle; no product wordmark is displayed there or in the task list. The sidebar starts with new task and quick chat, followed by project/task organization. Global destinations
and utilities remain reachable when the task list is collapsed. Narrow windows show the task list
over the workspace below the window strip; dock width calculations reserve the icon navigation.
Workspace pane headers share a 40px height. Right-panel tabs and the close control live in the global 32px window strip, aligned to the panel width, leaving no inner dock header. Both use a 28px control height with 2px vertical clearance. Toolbar labels collapse to named icons below 48rem so task titles remain readable; the selected tool tab retains its text at narrow widths. Native window controls retain their existing placement. Settings and Scene Studio retain their existing dedicated page layouts.

New installations start in compact mode; saved document-mode preferences remain unchanged. The
checkout context sits below the compact input as an inset footer. Model, reasoning, and permission choices remain directly visible in the input toolbar; attachment and send actions align to the right. An empty compact workspace centers its greeting in the space above the bottom composer. The same
composer/editor tree serves compact and document modes so layout changes preserve the draft. Short
windows scroll without clipping the input; conversations retain their transcript scroll behavior.

The workspace chrome stays quiet: toolbar actions use shared Ghost states, the composer keeps a
constant low elevation when focused, and an unopened right panel offers a centered single-column
list with descriptions available on hover or focus. Keep normal text contrast, persistent selected
states and visible keyboard focus; muted styling does not mean dimming usable controls. Preserve
the established shell regions and reserve decorative color for meaningful state. New-task panes omit a duplicate local title. Plugin actions above the composer or transcript are flat named Ghost controls with descriptions on hover or keyboard focus, rather than nested cards.

The compact composer and document-mode control bar keep a white canvas surface in the default light theme and the raised surface in dark mode. A constant 0.5px neutral edge and a soft 8px/24px shadow distinguish the input from the page. This composer-specific edge is an explicit exception to the borderless surface rule, requested on 2026-09-28; it is painted without changing the editor or control geometry. Neutral model, reasoning, permission, composer action and checkout controls use the same muted foreground as the workspace toolbar; permission warnings, voice recording and filled submit controls retain their semantic colors. Composer chevrons do not add opacity, and prompt placeholders use upright text. The right tool panel floats within an 8px gutter using the shared raised Card, retaining resize and collapse behavior. Gutters are included in its reserved width and disappear when closed. The main workspace uses the same 16px rounded, clipped boundary and 8px outer inset. Adjacent workspace and dock surfaces share one 8px gap instead of summing their margins. Checkout context has a 16px inset within the input width; related plugin/context rows use 8px vertical spacing. The main workspace and right tool panel share the same constant raised shadow token: 0 2px 8px, 4% opacity in light mode and 12% in dark mode. Sidebars retain flat elevation.

## Application rules

Update tokens/resolver and shared primitives before pages. The shared Button, Input, Textarea, Select, menus, Command, Tabs and navigation rows own interaction visuals. Feature components own content and layout only. Existing callers using a legacy outline Button are treated as neutral secondary controls until API migration; they do not introduce an outlined visual system.

Use the existing [component contract](docs/design/system.md) for accessibility, component admission, focus, materials and scanner rules. It must reference this file for visual values; changes to the two documents must remain consistent.

## Acceptance

Check light, dark and one non-default palette at normal and narrow widths. Exercise rest, hover, selected, selected-hover, pressed, keyboard focus and disabled states. Verify actual rendered colors and bounds, draft retention on expand/collapse, long Chinese labels, empty/error states and text scaling. A successful build is not visual acceptance. Implementation status and evidence belong only to the [change record](docs/sdlc/changes/2026-09-08-visual-coherence-standard/verification.md).
