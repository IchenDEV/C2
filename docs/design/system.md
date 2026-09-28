# C2 desktop design system

Status: **0.9.0 candidate**. The visual foundation is structurally frozen; proven business
patterns may still be added through the admission rules below. The system becomes 1.0.0 only after
the preview has been checked on Windows with Segoe UI/Cascadia in light and dark mode.

[Design.md](../../Design.md) owns the accepted visual standard and scope. This document owns its component implementation contract.

This document applies to `apps/desktop` only. The website has its own system; only brand and logo
assets may be shared. The machine-readable sources are
`apps/desktop/src/design/tokens.css` for static foundations and
`apps/desktop/src/design/theme.ts` for runtime palette resolution. Do not copy either contract
into product modules.

Phase 1 established the system and development preview. Phase 2 migrates production UI in coherent
feature cohorts. Each cohort lands or reuses the shared pattern first and replaces every selected
legacy call site. Maintained source uses ordinary lint rules rather than a generated debt baseline.

Open the development-only preview with:

```text
bun run dev:renderer
http://localhost:1420/?design-system=1
```

The preview is not linked from production navigation and is removed from the production bundle by
the `import.meta.env.DEV` gate.

## Design direction

Keep the existing C2 visual language while adopting the readable Codex desktop rhythm. It is a
desktop tool, not a mobile layout and not a showcase for decorative effects.

- One calm desktop density. Chrome, supporting copy, and reading content share a coordinated scale.
- Quiet neutral planes establish hierarchy. C2 blue identifies the primary action.
- Persistent panels, cards, inputs, and dialogs are borderless and separated by solid surface tones
  plus controlled elevation. Menu-like transient layers add one restrained shared material.
- Hover changes surface or text tone only. It never lifts, scales, or blooms a shadow.
- Icons use Phosphor's rounded regular weight and `currentColor`, imported through the shared icon
  adapter. Built-in skills, market entries, and Scenes omit emoji icon metadata so the renderer's
  semantic Phosphor defaults remain authoritative. Provider marks and explicitly authored external
  content may keep their shape, never their brand color.
- Official schemes are light, dark, and system. Built-in and custom palettes pass through the same
  theme resolver; product modules never branch on a palette name.

## Token architecture

There are three levels, in this order:

1. `--ds-foundation-*` stores raw numbers and colors. Product code must never consume it.
2. Semantic roles such as `--ds-color-surface`, `--ds-space-section`, and
   `--ds-type-body-size` describe intent.
3. Shared-component aliases such as `--ds-button-height` and `--ds-dialog-radius` belong only to
   reusable primitives.

Runtime palettes cross one additional seam: `resolveThemeColorProperties(palette, scheme,
contrast)` owns every legacy and semantic color property written to the renderer. `appearance.ts`
selects a validated palette and applies that result; it does not calculate component colors.

Tailwind v4 is the public styling API for product TypeScript. `src/styles.css` maps approved
semantic and shared-component tokens through `@theme inline`; product `.ts`/`.tsx` files consume
the resulting named utilities and never spell `--ds-*` directly. This includes arbitrary-variable
forms such as `rounded-(--ds-radius-control)` and inline styles such as
`var(--ds-color-surface)`. Add a semantic mapping to the theme bridge when a role is missing.

| contract | named Tailwind examples |
| --- | --- |
| surface and content | `bg-canvas`, `bg-surface`, `bg-raised`, `text-content`, `text-content-muted` |
| status and interaction fill | `text-status-success`, `text-status-warning`, `bg-fill-hover` |
| geometry and control size | `rounded-control`, `rounded-module`, `h-control-field`, `h-navigation-row`, `h-titlebar`, `h-panel-strip`, `size-icon-control` |
| layer and motion | `shadow-raised`, `shadow-modal`, `duration-feedback`, `ease-enter` |
| interaction focus | `focus-visible:focus-ring` |
| clipped row focus | `focus-visible:focus-ring-inset` |

`Button` owns this choice through `focusStyle="default" | "inset"`; callers do not layer competing focus utilities onto it.

Token and hand-written CSS infrastructure may consume semantic custom properties directly. The
runtime theme resolver, appearance font bridge, internal token preview, and explicitly allowlisted
third-party adapters are file-scoped exceptions because they write, enumerate, or transport token
names rather than style a product module.

Business components may override layout—width, flex/grid, alignment, positioning, responsive
behavior, and semantic spacing. They may not override typography, color, radius, elevation,
control height, motion, or focus appearance. A missing visual treatment becomes a semantic variant
in the shared primitive, not a local class string.

## System composition

The desktop system has five product-facing parts and one verification layer:

1. **Foundation tokens** are private raw inputs in `src/design/tokens.css`.
2. **Semantic tokens** name color, type, spacing, geometry, elevation, and motion by purpose.
3. **UI primitives** in `src/components/ui` own styling and interaction contracts. Product code
   imports each primitive directly; there is no barrel and no direct Base UI import.
4. **Business components** in `src/components/business` compose primitives into repeated C2
   patterns. They accept semantic content and behavior, not visual escape hatches.
5. **Product surfaces** own domain state, copy, data loading, and feature-specific layout.
6. **Preview, tests, and lint** make the contract visible and prevent new local styling.

The design checker requires `src/design`, `src/components/ui`, and
`src/components/business` to remain free of unallowlisted visual debt. Shared modules may not use
browser-native `title` bubbles as tooltips: visible descriptions stay visible, while hover-only
help uses the shared themed `Tooltip`.

`src/main.tsx` loads `styles.css` once for every renderer path; that Tailwind entry imports
`tokens.css`. Feature modules must not import the global token source as a side effect of mounting
a particular screen.

A business component is admitted only when at least two real callers already implement the same
pattern. Its interface must remove visual and accessibility decisions from callers, while domain
state mapping remains in the feature. The change that introduces it also replaces the proven
callers; do not add speculative wrappers or maintain two shared implementations of one pattern.

Transient loading motion uses the shared `Spinner` primitive. It is decorative inside a labelled
button or an existing status region; pass `label` only when the spinner itself owns the status
announcement. Product code must not add `animate-spin` to Lucide icons. Compose `Button` and
`Spinner` for busy actions rather than adding a loading prop to the button. `ActivityOrb` remains
reserved for agent, provider, search, voice, and shaping activity; `LoadFeedback` owns content-
blocking loading and recoverable failures.

Anchored rich content uses the shared `Popover` even when feature state controls whether it is
open. The primitive owns the trigger relationship, portal, focus handling, Escape, and outside-
press dismissal; the feature owns only its domain state, content, anchor, width, and alignment.
External controllers that already own positioning or keyboard selection, such as BlockNote's `@`
menu and the Usage chart hit targets, keep that controller but use the same `raised-material`,
semantic radius, and raised elevation. They do not recreate opacity, backdrop filters, static
borders, decorative rings, arbitrary radii, widths, or shadows; the wide menu measure is a shared
component token.

Product code uses the shared `Textarea` for multiline input. Its default density serves forms and
dialogs; `compact` serves embedded editors and inline comment cards. Feature code owns rows,
placeholders, values, and validation, but not local radius, fill, focus ring, typography, or
padding. Native `<textarea>` belongs only inside the primitive.

Visible product actions use the shared `Button` with an explicit semantic variant and size. This
includes toolbar icons, disclosure controls, tabs, compact status chips, picker rows, and buttons
composed as menu, popover, tooltip, or dialog triggers. A feature may pass layout classes, but it
does not recreate button typography, color, radius, height, focus, disabled, selected, or loading
states. Native `<button>` is limited to explicit third-party, canvas, terminal, injected-document,
or generated isolated-document boundaries and must have a file-scoped scanner exception; it is not
an escape hatch for a visual variant.

Icon-only actions use `TooltipButton` whenever explanatory hover/focus text is useful. Its single
`label` supplies both the accessible name and the themed tooltip, so product code does not combine
`Button`, native `title`, and local tooltip timing. Icon Buttons used as menu or popover triggers
must still carry an explicit `aria-label`; they do not add a competing native title bubble.

`ControlChip` is the single compact text-control primitive for composer and toolbar status rows.
`CompositeActionRow` owns rows whose full surface is one primary button while independent trailing
actions remain operable; product code must not emulate that contract with a clickable `div` or
nested buttons. Destructive browser-shell confirmation uses `AlertDialog`, never `window.confirm`.

Single-line forms use the shared `Input`, `Select`, `Checkbox`, `RadioGroup`, and `Field` primitives. Default
inputs and select triggers are 36px; compact inputs and small select triggers are 32px. Checkbox
visuals remain 16px with an expanded pointer target. `FieldGroup` owns the 16px form-section gap,
while `Field` owns label, description, invalid, and error relationships. Every `SelectItem` is
composed inside `SelectGroup`; product surfaces do not place items directly in `SelectContent`.
`Command` uses a 36px search field and 32px menu rows. The development preview renders these real
shared components rather than maintaining CSS lookalikes.

Native `<input>` remains valid only for browser-owned file, color, range, radio, checkbox, and
hidden controls; ordinary text, search, number, URL, password, and similar fields use `Input`.
Native `<select>` belongs nowhere in product UI because the shared Select owns its popup material,
keyboard behavior, geometry, and theme state.

Viewport-bound dialogs use `max-h-dialog-max`; their bounded file, issue, and preview collections
use `max-h-dialog-content`. Both resolve through the Tailwind theme bridge, so product code does not
repeat viewport calculations or read shared-component variables directly. Scrollable rows use the
shared `ScrollArea` and an inset-focus `Button` row; the primitive owns radius, focus, and scrollbar
visuals while the feature keeps its result content and keyboard-selection state.

Structural divisions use the shared `Separator` rather than local `border-b`, `border-t`, or
inline-edge border utilities. The separator is semantic when it divides content; use
`aria-hidden` only when an adjacent resize handle already owns the separator role. Table, diff,
code, document, and native scrollbar structure remain file-scoped content exceptions.

The shared business set is:

| module | owns | current callers |
| --- | --- | --- |
| `PageHeader` | page heading hierarchy, description measure, responsive action placement | Automations, Plugin Hub, Scene Studio, Task Board, settings pages |
| `SearchField` | labelled search input, icon geometry, optional accessible clear action | Automations, Docker, Task Board, Plugin Manager, Memory, Trajectory |
| `Empty` primitive | empty-state hierarchy, media, description, and action composition | Automations, Pull Requests |
| `SelectableRow` | compact picker choice, visible selection mark, accessible selected/disabled state, description and metadata layout | Composer mode, memory, collaboration, worktree, provider, and model pickers; Scene picker; Checkout picker |
| `NavigationRow` | navigation/current-page state, inset focus, truncation, and optional themed tooltip | Settings navigation and right-panel picker |
| `CompositeActionRow` | semantic full-row primary action plus independent trailing controls | Mission Control, Browser tabs, File tree |
| `ChoiceRow` | labelled radio or checkbox choice, description/preview layout, selected and disabled presentation | Agent question dialog, design-system preview |
| `MasterDetailRow` | selected master-list row, leading mark, description, and metadata layout | Automations, Pull Requests |
| `DetailMetric` | aligned label/value metadata row | Automation detail, Pull-request detail |
| `StatusBadge` | neutral, success, warning, and destructive status-pill treatment | Automations, Turn Card, Plugin Manager and bundle administration, Scene chip/popover, GitHub pull-request detail |
| `StatusIndicator` | semantic dot-and-label status with theme-managed tones | Docker, Device connections |
| `SettingToggle` | visible label and description association, immediate boolean control, disabled presentation, and row layout | Project actions, Memory, Sync, Project scheduling, Appshots, Pets |

The settings cohort composes those primitives once, in `src/settings/SettingsPrimitives.tsx`: `Page`
wraps `SettingsPanel`/`PageHeader`, `Row` and `ProjectRow` wrap `SettingRow` (the project variant
adding the shared trailing control lane), and `GroupHeading` owns the 14px/600 group label used by
the pages whose sections already own their spacing. Settings pages consume that module instead of
re-deriving the anatomy; the module is cohort-local rather than a business primitive, and it is
subject to the same lint restrictions as every other product file. Current callers: General,
Import, Keybindings, Project, Worktrees, Memory, Providers, Appshots, and the Appearance, Pets and
Sync pages that are wrapped through `SettingsPage`.

`SelectableRow` is deliberately limited to persistent selection inside compact pickers. Radio or
checkbox questions use `ChoiceRow`; navigation/current-page rows, disclosure rows, and master-detail
list rows keep their own interaction contracts. `StatusBadge` is limited to labelled pills; dot-and-label status
uses `StatusIndicator`, while metadata such as source, version, or category remains separate. Domain values
such as `failed`, `paused`, or `trusted` are mapped to its small semantic tone set inside the owning
feature rather than taught to the shared component. `SettingToggle` is limited to immediately
applied boolean preferences. Form selections, batch selection, tri-state policy, and composite rows
with disclosure or secondary actions retain their checkbox, select, or feature-owned contracts.

## Color and surfaces

All app chrome consumes semantic OKLCH tokens. The five neutral planes are fixed:

| role | use |
| --- | --- |
| `canvas` | window content background |
| `sidebar` | application chrome; macOS may use native vibrancy |
| `surface` | cards, fields, persistent panels |
| `raised` | menus, popovers, tooltips |
| `modal` | dialogs and blocking surfaces |

Components cannot mix transparency to invent a sixth plane. The only managed exceptions are macOS
sidebar vibrancy and the shared `raised-material` used by menu-like transient layers. Cards, inputs,
dialogs, tooltips, and toasts remain solid. `raised-material` derives its tint from `raised`; its
opacity and backdrop filter live in the token system rather than component or feature code. Increased
Contrast, Reduced Transparency, and unsupported backdrop-filter engines force the opaque `raised`
fallback.

Interactive neutral fills follow the `quiet → rest → hover → selected → selected-hover → pressed`
ladder defined in [Design.md](../../Design.md#color-and-interaction-states). Selection remains visible
under hover; focus is independent. Ordinary controls use neutral fills, never blue hover or a local
opacity multiplier. Explicit preview schemes and appearance-generated themes expose the same roles.

In the light scheme, `surface` is only slightly tinted while `raised` and `modal` approach the
palette background; elevation and placement communicate their transient layer. The dark scheme
retains stronger tonal separation. This ordering is resolved centrally and is never reconstructed
inside Popover, Dialog, Select, Input, Textarea, or a feature module.

C2 blue is fixed for primary actions. Use no more than one primary action per local area.
Success, warning, destructive, and neutral keyboard focus have dedicated roles. A color change must
be made centrally, demonstrate the light and dark contrast contract, and include light, dark, and narrow screenshots in the scoped change.

Filled actions always consume an inseparable semantic pair: Primary uses `primary` with
`primary-foreground`, and Destructive uses `destructive` with `destructive-foreground`. Both pairs
must reach 4.5:1 in light and dark. A page or preview shell may inherit the UI font family, but must
not reset a shared control's color, font size, line height, or state styling.

Dark mode separates persistent planes primarily with lightness; floating workspace surfaces may use the shared raised elevation. Do not add a white hairline,
inner glow, or feature-local translucent glass to recover separation.

## Typography

Use the platform UI stack:

```css
-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
```

Monospace uses `ui-monospace`, SF Mono on macOS, and Cascadia/Consolas fallbacks on Windows. Do not
bundle Inter or force SF on Windows.

The default density follows Codex desktop rather than the former compact C2 scale. UI and code
sizes remain independently user-configurable, but every derived role and line height comes from
the shared typography engine instead of page-local arithmetic.

The authoritative type scale is in [Design.md](../../Design.md#typography): UI 14/20, prose 15/24,
supporting copy 13/20, metadata 12/16, and new-install code 13/20. Saved code-size preferences are
preserved. Static CSS fallbacks and the runtime typography resolver must agree.

Content headings use the shared
Dialog, Section, and Page roles. Persistent navigation, settings labels, and descriptive copy may
not use Caption; 10px is not a product UI role.

Reference and comparison specimens use one flat row structure. A metadata column may identify the
role and metric, but it does not receive its own fill, radius, elevation, or control-like container.
Rows use spacing and a quiet structural separator so the label and sample read as one comparison.

Tailwind's `text-xs`, `text-sm`, `text-base`, arbitrary `text-[…]`, local `leading-*` overrides,
and the legacy `text-ui` / `text-fine` / `text-hint` / `text-cap` aliases are not product APIs.
Product code uses `text-body`, `text-prose`, `text-callout`, `text-metadata`, `text-caption`, or a
heading role with its paired line height. Compatibility aliases may exist only inside the design
bridge while older persisted/custom content hydrates.

## Spacing, geometry, and control height

Spacing is a 4px grid with one controlled 2px optical step:

| value | semantic role |
| --- | --- |
| 2 | optical correction |
| 4 | inline and metadata gap |
| 6 | icon/text and grouped controls |
| 8 | row horizontal inset and module gap |
| 12 | card, popover, and persistent-panel inset |
| 16 | section and form group |
| 24 | dialog and page gutter |
| 32 | large page section |

The product radius scale is 0 / 12 / 16 / 24px:

- 0: joined structural edges and straight selection indicators.
- 12: checkbox, status mark, micro element, button, input, menu item, list row, track, and pill.
- 16: card, popover, sidebar module, dialog, and large panel.
- 24: Composer, whose larger content surface keeps its own fixed semantic radius.

Fully round geometry is reserved for intrinsically circular, square objects such as status dots,
avatars, and circular icon controls. Non-square badges, switches, progress tracks, and capsules use
the semantic 12px control radius; CSS naturally clamps it to half the element height. Standalone
surfaces that cannot inherit the app token sheet use exact 12px control and 16px module fallbacks.
Icons are exactly 12 / 14 / 16px for inline, list, and control roles. Standard UI does not use 20px
icons.

Control heights are 28px for compact/icon controls, 32px for normal buttons, menus, and navigation
rows, and 36px for inputs, selects, and important controls. The main titlebar is 46px and pane
strips are 40px. A taller element is content input such as Composer, not a generic large-button
size.

Workspace window chrome uses a 32px drag strip and scoped 40px workspace pane headers. Right-panel tabs and its close control share the window strip through the shell titlebar host, track the dock width, and leave no inner dock header. The focused task title reuses the existing title projection in the window strip; feature pages leave that title empty. Search and the sidebar toggle live in the window strip, aligned with the expanded sidebar and reserving at least 96px before controls for native window buttons. The task list has no duplicate header or product wordmark. Below 48rem, toolbar actions retain accessible names while dropping visible labels; the selected dock tab keeps its label even when inactive tabs become icons. Settings and Scene Studio retain their own header geometry.

Left workspace controls, icon navigation and task sidebar share the session toolbar’s muted foreground for labels and icons. Hover and selected rows use neutral fills, selection markers and visible keyboard focus; status badges retain semantic colors.

Neutral composer configuration, action and checkout controls use the same muted foreground, including hover/open states. Warning chips, voice recording and the filled submit action retain their semantic colors.

Session titlebar tools form one quiet monochrome toolbar. Icons and labels keep the muted foreground
across rest, hover, open, and pressed states; hover may add a neutral surface and keyboard focus
adds the standard ring. A real open or pressed state may use only a neutral fill. Titlebar actions
never switch to the product accent color. Independent 28px controls use a 4px gap; joined
split-button halves keep a zero-width inner gap and a subtle seam.

The session titlebar divider is contextual: an empty pane has no bottom hairline, while a pane with
persisted, running, or loading conversation content restores the semantic border. Rail and dock
titlebars keep their structural separators.

## Elevation and borders

Elevation communicates a real layer and never changes on hover:

| role | use |
| --- | --- |
| `elevation-surface` | no shadow; compatibility role for cards, inputs, and persistent panels |
| `elevation-raised` | 0 2px 8px, 4% light / 12% dark; one restrained short shadow for menu, popover, tooltip, floating main workspace and right tool panel |
| `composer-elevation` | constant 0.5px neutral edge plus 0 8px 24px and 0 2px 6px shadows; compact composer and document toolbar only |
| `elevation-modal` | one restrained wider shadow for dialog, blocking overlay |

Persistent titlebars, sidebars, cards, inputs, and panels are flat by default. The workspace composer (including its document-mode control bar) and right tool panel are explicit floating surfaces. The composer uses the canvas surface in light mode (white in the default palette), the raised surface in dark mode, and its own composer elevation token for a 0.5px neutral edge plus fixed soft shadow. The right tool panel uses the existing raised background and elevation. Both remain constant across focus and hover. The right panel uses the shared raised Card with an 8px outer gutter included in its reserved width; closing it leaves no gutter. The main workspace shares its 16px rounded, clipped boundary and 8px inset, using the same raised shadow as the right panel. Keep its reading background unchanged. Other surfaces use tonal planes, spacing,
and contextual structural separators. Settings pages separate major groups with page-section spacing. Related rows share one tonal module
with internal hairlines; they do not become a stack of individually elevated cards. Choice tiles
such as appearance schemes and themes use tonal hover and a semantic selected ring without surface
elevation. Popovers and dialogs retain only the restrained elevation assigned to their real layer.

Static borders and decorative rings are forbidden outside the following whitelist:

- the composer’s constant 0.5px neutral edge, explicitly requested with its white surface (the composer elevation token also owns its fixed soft shadow);
- neutral keyboard focus;
- error or warning status;
- table, diff, code, or document content structure;
- resize divider or drag target;
- progress track.

## State contracts

Every shared control implements rest, hover, pressed, keyboard focus, disabled, and loading. Selected,
selected-hover and invalid are added where meaningful. The state ladder is owned by Design.md.

- **Focus:** a 2px neutral, high-contrast indicator, visible for keyboard focus only. No blue
  focus ring. Inputs may slightly raise their surface, but must not resize.
- **Invalid:** a persistent 2px destructive indicator plus adjacent error text.
- **Disabled:** 50% opacity and no pointer events; do not invent a disabled gray.
- **Read-only:** quiet fill and normal legibility.
- **Loading:** stable dimensions with a 14px spinner; do not swap to a differently sized control.

Button variants are Primary, Secondary, Ghost, and Destructive. Secondary is a neutral tonal
surface, Ghost has no shadow, and Destructive is red only for a destructive action. Legacy Outline callers share the Secondary neutral visual treatment; no outlined visual system is added. Standalone icon-size Button variants own a full circular radius. Responsive icon-only toolbar controls use equal width/height and the same circle; joined split buttons preserve their connected shape. Default and icon buttons are 32px, compact and mini buttons are 28px, and the 36px field
size is reserved for an action that must align with an input or select. Size changes never
introduce a different radius. Primary and Destructive own their paired foreground tokens; Secondary
and Ghost stay on the neutral text hierarchy so the four variants read as one control family.

Tabs provide three presentations without changing their keyboard or panel contract: Default is a
36px segmented container with 32px triggers, Line is content navigation with a 2px selection mark,
and Toolbar is a 32px application strip. Tabs use `rounded-control`, semantic fills, and the shared
focus ring; callers may control width, overflow, and orientation only.

Dialog and Alert Dialog share `bg-overlay`, `bg-modal`, `rounded-modal`, `shadow-modal`, a 24px
gutter, and the same header/footer typography and spacing. Callers may choose a semantic maximum
width or scrolling behavior, but do not rebuild the overlay, radius, elevation, or close control.

Card owns the solid, flat `surface` plane, 16px radius, 12px inset, title,
description, content, and footer hierarchy. Product callers compose those parts and may change
layout, but do not restate the card fill, radius, elevation, typography, or inset.

Popover, DropdownMenu, ContextMenu, and Select share `raised-material`: a theme-managed mostly
opaque `raised` tint with one restrained backdrop filter and the standard raised elevation. Tooltip
and Toast keep the opaque `raised` plane for maximum legibility. Popover uses the 16px module radius
and 12px inset; Tooltip uses the 12px control radius and waits 600ms before the first open, with a
400ms instant phase between related triggers. `TooltipButton` is the standard icon-action composition
and guarantees that its tooltip text also names the control. Toast uses shared Buttons for actions, announces
errors as alerts, and announces other outcomes as status messages. Callers own content, anchoring,
and layout only.

Inputs use a flat neutral surface without a decorative border or shadow. Rest, hover, keyboard
focus, invalid, disabled, read-only, and loading all preserve the 36px field contract.

## Motion

| role | duration | use |
| --- | --- | --- |
| feedback | 120ms | hover, press, color, opacity |
| layer | 160ms | menu, popover, tooltip |
| dialog | 220ms | dialog, dock, tree |
| page | 280ms | full-page transition |
| pane | 0/160/280ms | tiling pane geometry, scaled by the Window motion preference |

Entrances use `cubic-bezier(0.16, 1, 0.3, 1)` and exits use `ease-in`. Do not add spring, bounce,
hover scaling, or a one-off duration. Reduced Motion collapses all five semantic durations.

The pane role is the one place a transition may move layout: when the tiling split tree changes,
panes and dividers glide between their old and new rectangles. The Appearance → Window motion
preference owns its duration — Instant is 0ms, Fast borrows the layer duration, Smooth borrows the
page duration — so the choice never introduces a new timing. Pointer drags keep 1:1 pointer
tracking with the transition suspended; only discrete layout changes (split, close, keyboard
resize) glide.

## Window behavior

The desktop minimum remains 800×500. Window classes are narrow at 800–999px, standard at
1000–1399px, and wide at 1400px and above. Local modules use container queries when their own
available width—not the window—is the cause of a layout change. There is no mobile application
layout. Dialogs must avoid overflow around 400px for tests and auxiliary windows.

## Shared components and exceptions

Local shadcn/ui components remain the unique styled foundation. Interaction primitives use Base
UI behind `components/ui`; product surfaces use the Base `render` composition API and must not
import primitive libraries directly. Radix-backed wrappers and the direct `radix-ui` dependency
are not part of the desktop UI contract.

AI-native presentation patterns may use selected AI Elements source components under
`components/ai-elements`. Adapt them to C2 tokens and ACP data rather than adding Next.js or
AI SDK transport assumptions. Do not reimplement conversations, messages, reasoning, tools,
plans, or task progress inside product surfaces when an AI Element fits the behavior.

Terminal, Monaco/Shiki, and BlockNote are controlled content-renderer exceptions:

- their surrounding chrome uses C2 semantic tokens;
- xterm may use its fixed ANSI palette;
- Monaco/Shiki may own syntax colors and editor content typography;
- BlockNote may own document typography inside the document surface.

Every lint exception is narrow and carries its reason beside the suppression. There is no generated
design-debt baseline or separate design-system allowlist.

## Accessibility

Version 1 includes Reduced Motion, Reduced Transparency, Increased Contrast, and Bold Text. Bold
Text is represented by `data-ds-bold-text` until Phase 2 wires the native desktop preference. The
Appearance UI-size control resolves the whole semantic type scale through the typography engine;
it is not page-local zoom. Semantic colors are reviewed in both schemes through the development
Preview and rendered accessibility checks.

## Behavioral laws retained from C2

- Live transcript follows the bottom only while the reader is already at the latest content.
  Pointer or keyboard interaction pauses following and exposes “Jump to latest.” Prepending history
  preserves the content under the reader.
- Actions provide perceptible feedback. Use the shared toast for outcomes, prefer undo for
  recoverable actions, and reserve confirmation dialogs for irreversible work.
- Tooltips explain unlabelled icons; they never contain information required to proceed.
- Errors remain visible long enough to read and act on.

## Enforcement and migration

Run from `apps/desktop`:

```bash
bun run lint
bun test
bun run build:renderer
```

ESLint checks TypeScript, React Hook ordering, raw product textareas, inline radius styles, and
restricted Tailwind classes, unmanaged product controls, and direct primitive usage outside the
shared component layer. Stylelint parses maintained CSS, catches structural CSS errors, and
restricts `border-radius` declarations to the documented semantic values. TypeScript and
Vite/Tailwind compilation own type safety and generated CSS validity; there is no second
compiled-CSS scanner. Physical and logical spacing directions share the same scale. Rules that
cannot reliably express a semantic distinction stay part of Preview and rendered review instead
of growing a repository-specific source scanner.

There is no generated design-debt baseline. Lint rules apply to the maintained source directly;
rules that cannot be expressed by mature lint tooling are reviewed through the Preview and rendered
acceptance instead of being recreated as repository-specific source scanners.

Pixel-diff CI is intentionally deferred because platform system-font rasterization differs. CI
enforces lint, type, test, and build rules; the development preview is manually checked in light, dark, narrow,
Reduced Motion, Increased Contrast, and Bold Text. After the migration, evaluate separate macOS
and Windows visual baselines.

## Versioning and review

- Patch: documentation or alias correction with no semantic/visual change.
- Minor: token value change or additive semantic role.
- Major: token removal, rename, or meaning change.

Foundation changes belong in an isolated commit and require light, dark, and narrow screenshots.
A feature PR cannot casually change foundation values. Before 1.0.0, validate the Preview on
Windows in Segoe UI/Cascadia, light and dark; until then the status remains 0.9.x.
