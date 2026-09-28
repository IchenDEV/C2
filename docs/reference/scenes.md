# Agent Scenes

Status: **1.0.0 implemented and normative**. Schemas are frozen at
`crates/core/schemas/agent-scenes/1.0.0/`; runtime behavior described here governs the current
implementation. The accepted but not yet implemented Scenes 2.0 direction is tracked separately in
[`design/scenes-v2.md`](../design/scenes-v2.md) and does not supersede this contract. Built-in scene
and pipeline definitions live next to the schemas under `examples/` and double as conformance
fixtures.

A **scene** is a pure-data bundle that configures a session for one *stage of work*: execution
posture, active skills, a task-brief skeleton, the artifacts the stage must produce, exit criteria,
and declarative hooks. A **pipeline** chains scenes into a lifecycle (the built-in one:
research → develop → test → fix → acceptance, with a test/fix loop).

Scenes exist because every ingredient already shipped separately — permission modes, memory
presets, worktree baselines, the skill library — but the user had to reassemble them
by hand for every kind of work. A scene is the packaging object; it introduces **no new
execution capability**. Like plugins, installing or activating a scene never runs a script.

## Concepts

| Term | Meaning |
|---|---|
| Scene | Data bundle configuring a session for one stage of work. `*.scene.json`. |
| Pipeline | Ordered stages, each bound to a scene, with gated transitions and artifact carry. `*.pipeline.json`. |
| Brief | The task-book skeleton a scene offers in the composer (template + typed slots). |
| Artifact | A declared output of a scene (report, plan, test report, checklist, …), persisted in the artifact store. |
| Exit criteria | Conditions under which the host surfaces "this stage looks done". |
| Gate | Approval level for entering a stage: `suggest`, `confirm`, or `auto`. |
| Hook | Declarative event → action binding (suggest a scene, run a macro, notify). |
| Carry | Compiling an earlier stage's artifact into a later stage's context. |

## Files, locations, precedence

Scenes and pipelines are JSON documents validated against the 1.0.0 schemas:

- `scene.schema.json` — one file per scene, named `<name>.scene.json`.
- `pipeline.schema.json` — one file per pipeline, named `<name>.pipeline.json`.

Discovery locations, highest precedence first:

1. **Project** — `<project>/.codetwo/scenes/` (checked into the repo; the team's scenes).
2. **User** — `~/.config/codetwo/scenes/`.
3. **Plugin** — a plugin's `scenes/` component directory; ids are namespaced
   `<plugin-id>:scene:<name>` (mirroring plugin command components).
4. **Builtin** — compiled into the core (the five R&D scenes and `rnd-lifecycle`).

A bare scene reference (`develop`) resolves through this list top-down; `<source>:<name>`
(`builtin:develop`, `my-plugin:scene:review`) pins the source. Same rule for pipelines. A
malformed file is logged and skipped, never fatal (same posture as `SkillLibrary::load_dir`). A
reference that resolves to nothing degrades to a warning chip in the UI; the session continues
without the scene.

Distribution rides the existing plugin pipeline: a plugin's component counts gain `scenes` and
`pipelines`, validated against these schemas at install time. Installation remains pure data.

## The scene object

Field-by-field semantics. The schema is the authority on shape; this section is the authority on
behavior.

### Identity

`name`, `version`, `title`, `description`, `icon`, `author`, `license`, `keywords`,
`localizations`. `name` is the reference key and must be stable across versions;
`localizations` supplies per-locale `title`/`description` (the host picks the best BCP 47 match,
falling back to the top-level strings — same model as `i18n/strings.ts`).

### `execution` — posture

Every field optional; **unset means inherit** (from the live session when soft-applying, from
project/user defaults when creating a session). Values reuse the existing vocabularies verbatim:

| Field | Vocabulary | Source of truth |
|---|---|---|
| `session_mode` | `read_only` \| `ask` \| `auto_edit` \| `full_access` | `apps/desktop/src/session/mode.ts` |
| `memory_preset` | `standard` \| `read_only` \| `private` \| `learn_only` | composer memory presets |
| `worktree` | `off` \| `current` \| `origin_default` | `crates/core/src/project.rs` |
| `providers` | provider ids, preference order | `crates/core/src/provider.rs` registry |
| `model`, `reasoning_effort` | provider-defined strings | provider capabilities |

Legacy `execution.plan_first` is accepted only when reading older scene files. It has no
effect and is omitted when saving or exporting a scene.

**Binding matrix.** Not everything can change mid-session; the host applies a scene at two
strengths and must show which one happened:

- **Soft-apply** (switching scenes inside a live session): `session_mode`, `memory_preset`,
  skills, brief, guardrails take effect immediately. `providers`, `model`,
  `reasoning_effort` apply from the next session; `worktree` is immutable per session by design.
- **Full-apply** (scene chosen at session creation, or "new session in this scene"): everything
  applies. When a soft-apply leaves fields pending, the scene chip shows a partial indicator and
  offers "restart in this scene" which creates a new session, carries the declared artifacts, and
  closes the gap.

**Escalation rule (normative).** Applying a scene may tighten permissions silently but may never
loosen them silently. If the target scene's `session_mode` is looser than the session's current
mode (order per `SESSION_MODES`, loosest last), the host requires an explicit confirmation naming
both modes — regardless of gate, hook, or pipeline settings. `auto` gates downgrade to `suggest`
in this case. A scene can therefore never be a privilege-escalation vector.

### `skills` — what's on the palette

- `pinned`: skill references surfaced first in the `/` picker while the scene is active. Mcp
  skills among them are attached at `session/new` (full-apply) or at the next session
  (soft-apply), matching how Mcp skills bind today.
- `inline`: fragments defined in-scene, prepended to **every** prompt compiled under the scene.
  Compilation order: project rules (unconditional, as today) → scene guardrails → scene inline
  fragments → the user's document. Scene text never displaces project rules.
- `suppress_unpinned`: focus mode for the `/` picker; a "show all" affordance must remain.

**Skill references** reuse existing id forms: bare library ids (`~/.config/codetwo/skills/`),
`harness:<harness>:<skill-dir>` (auto-discovered SKILL.md), `<plugin-id>:<component>:<slug>`
(plugin components). Scenes add no new skill kind and no new resolution machinery.

### `brief` — the task book

When a scene activates on an **empty** composer document, the host offers (never forces) the
brief: `template` is inserted as BlockNote content with each `{{slot-id}}` rendered as a typed
fillable field (Tab-navigable), per `slots`:

- `text` / `multiline` — free text.
- `select` — dropdown over `options`.
- `file` — workspace file picker (compiles like an `@` file mention).
- `artifact` — picker over artifacts carried into this scene (compiles like a carried-artifact
  reference; empty when nothing was carried).

A non-empty document is never overwritten; the brief is then reachable behind an explicit
"insert brief" action. `clarify` sets how the agent resolves an underspecified brief:
`multi_choice` (default — structured option questions), `free_form`, or `off`. The host injects
the corresponding instruction with the scene fragments.

Unfilled `required` slots produce a composer warning, not a hard block: the brief guides, the
user decides.

### `artifacts` — declared outputs

Artifacts are the scene's contract: what must exist for the stage to be done, and what later
stages can carry. They persist in the existing content-addressed artifact store with a scene
artifact descriptor (scene name, artifact id, session, version).

**Capture (normative).** The host instructs the agent — via a generated fragment listing each
declared artifact's id, title, and `template` — to emit each artifact as a single fenced
markdown document marked with its artifact id. The host recognizes and stores these; the user can
also manually mark any transcript output or file as fulfilling a declared artifact ("pin as
artifact"), and can unpin. Re-emitting an artifact id creates a new version; carry always
resolves to the newest version unless the user pins an older one. Recognition is best-effort;
manual pinning is the guaranteed path, so agents that ignore the convention degrade gracefully.

### `exit` — when the stage is done

`criteria` (all must hold; default `[{required_artifacts}, {user_confirm}]`):

- `required_artifacts` — every `required: true` artifact has at least one stored version.
- `checklist_complete` — the referenced checklist artifact has no unchecked items.
- `tests_pass` — the most recent test signal in this session reports success (see hook events).
- `user_confirm` — always satisfied last, by the user acting on the completion banner. A scene
  with `user_confirm` can never auto-advance.
- `custom` — free-text criterion; evaluated by the agent when asked, surfaced to the user as
  unverified.

When all non-`user_confirm` criteria hold, the host shows a quiet completion banner above the
composer listing the produced artifacts and the `next` suggestions, each with its carry set
(e.g. "Research complete → Start development, carrying: Research report"). Dismissal is
remembered per session; the banner never re-fires for the same state.

### `hooks` — events, declaratively

Events: `enter`, `turn_end`, `artifact_produced` (with `artifact` filter), `exit_criteria_met`,
`tests_failed`, `schedule` (five-field cron, requires `schedule`).

`tests_failed` (and the `tests_pass` criterion) are fed by the engine's existing tool-call
classification: a terminal command the engine recognizes as a test run, with a nonzero exit,
raises `tests_failed` for the turn. Recognition is heuristic and versioned with the engine; hooks
must tolerate the event never firing.

Actions are an **allowlist**, and nothing else: `suggest_scene`, `suggest_next` (first `exit.next`
entry), `run_macro` (a Macro skill, `args` pre-fill slots; `{{artifact:<id>}}` interpolates a
carried/produced artifact's content), `notify`. Suggest/notify actions render UI; they never act.
`run_macro` composes and submits a prompt **within the session's current permission mode** and is
subject to the escalation rule like everything else.

`schedule` hooks are inert until the user enables scheduling for the project (off by default);
scheduled runs land in the session as normal turns, visibly attributed to the hook.

Hook loops are bounded: at most one hook-initiated `run_macro` may be in flight per session, and a
hook may not fire again for a state it already fired for (same debounce rule as the completion
banner).

### `constraints`

`guardrails` are short imperative rules injected with the inline fragments — prompt-level
guidance, deliberately *not* enforcement; `execution.session_mode` remains the enforcement
boundary and the docs must never present guardrails as a sandbox. `tools.allow`/`deny` are
advisory hints forwarded to providers that support tool gating; ignored elsewhere.

## Auto Scene

Auto Scene is an explicit per-session mode. When enabled, the host gives the Agent a short routing
instruction plus bounded `scene_list` and `scene_select` tools. The resolved catalog is not injected
into every prompt: the Agent queries compact candidates on demand, then selects one before
substantive work when no scene is active or a better scene fits. The selected scene remains visible
in the composer as `Auto · <scene>`; the session stores both the Auto flag and the concrete active
scene independently.

`scene_list` accepts an optional short task query and returns at most 50 installed references with
their title and bounded description. It never returns scene instructions; those are loaded only by
`scene_select` after the host validates the exact installed reference.

`scene_select` accepts only an installed scene reference and a short user-visible reason. A
successful call soft-applies that scene and returns its prompt preamble so the Agent follows the
new guardrails, skills, artifact contract, and clarification behavior in the same turn. The tool
cannot create or edit scene definitions, disable Auto Scene, or select an uninstalled reference.

Auto Scene does not bypass the escalation rule. A tighter or equivalent posture can apply
directly. A switch that would loosen the session mode stops at an explicit user approval; until
approval, nothing is applied. The Agent's own text is never accepted as evidence that a scene or
permission changed.

### `extensions`

Namespaced vendor escape hatch, same contract as plugins: hosts must ignore unknown extensions,
and nothing in an extension may weaken a normative rule above.

## Pipelines

A pipeline instance is created when the user starts one (project default pipelines can be offered
at session creation). State per instance: current stage, per-stage artifact versions, and the
transition history — all persisted with the project.

- `stages[]` bind stage ids to scenes. Stage ids, not scene names, are the graph nodes, so one
  scene may appear twice (e.g. two review stages).
- **Default flow**: absent `transitions`, stages advance linearly in array order, firing on
  `exit_criteria_met` with the target stage's `gate`. Listed transitions replace the default
  outgoing edges of their `from` stage.
- **Gates**: `suggest` (banner; default), `confirm` (explicit modal), `auto` (advance without
  asking). `auto` is honored only when the target scene does not loosen permissions and its exit
  criteria did not include `user_confirm` — otherwise it downgrades (escalation rule wins).
- **Carry**: a stage's `carry` names artifacts from earlier stages; each resolves to the newest
  stored version and is compiled into context the way a session mention is today (inlined,
  labeled, `as` overrides the label). A `from` stage that has not run in this instance is skipped
  silently — this is what makes loops (test ⇄ fix) and skipped stages composable.
- **Loops** are ordinary transitions (`test → fix` on `tests_failed`, `fix → test` on
  `exit_criteria_met`). Each re-entry into a stage starts a fresh turn context but carries the
  newest artifacts; the pipeline view renders the loop count.
- Stage transitions may keep the current session (soft-apply) or open a new session (full-apply)
  per the binding matrix; the artifact store, not session memory, is the contract between stages.

## Security model

1. **Pure data.** Scenes and pipelines contain no executable content. Install/activate never runs
   anything.
2. **No silent loosening.** The escalation rule above is absolute: gates, hooks, and pipelines
   cannot bypass it.
3. **Bounded hooks.** Action allowlist only; debounced; one hook-initiated prompt in flight per
   session; schedules off until explicitly enabled per project.
4. **Guardrails ≠ sandbox.** Prompt guidance and permission enforcement are separate layers and
   the UI must not conflate them.
5. **Provenance.** The scene chip always names the active scene and its source
   (builtin/user/project/plugin); hook-initiated turns are attributed in the transcript.

## UI contract (summary)

- The composer keeps scene, provider, and model choices visible. Permission, memory, and worktree
  settings use one adjacent disclosure control and expand into their own wrapping row; when the
  checkout bar is present, it remains the single owner of worktree selection. The **scene chip**
  opens only scene selection, and manual overrides mark it "customized" without mutating the scene
  definition.
- **Shift+Tab** cycles scenes (project-configurable ring); the full picker lists all resolved
  scenes with source badges.
- Completion banner and stage suggestions as specified under `exit`.
- Pipeline instances render as a horizontal stage track (done / current / loop counts), each stage
  expanding to its artifacts and transcript span.

## Interop

- **Agent Skills**: scenes consume SKILL.md skills through the existing harness discovery
  (`harness:` references) and add no competing format at the skill layer. A scene is the layer
  *above* skills — posture + stage + artifacts — which SKILL.md deliberately does not model.
- **Export**: a scene's inline fragments and brief can be exported as a SKILL.md for hosts that
  only speak skills (lossy: posture, artifacts, exit, hooks don't survive; the exporter must say
  so).
- **Domain neutrality**: nothing in the format is code-specific — stages, briefs, artifacts, and
  gates apply unchanged to non-code work (e.g. proposal → draft → review → publish). Office-domain
  scene packs are packaging, not format changes.

## Versioning & conformance

- The `$schema` constant pins the format version; `1.0.0` documents validate against
  `additionalProperties: false` — authoring-time strictness, with `extensions` as the sanctioned
  escape hatch. Future minor versions may add optional fields under a new schema id; hosts read
  older versions forever, and write the version they read.
- A conforming host: validates on load, skips malformed files non-fatally, implements the binding
  matrix and escalation rule, persists artifacts with provenance, and treats every unresolved
  reference as a degradation, never an error.
- The `examples/` definitions are normative fixtures: a conforming implementation must load all
  six files and reproduce the behaviors described here.

## Starter packs

`packs/office-starter/` (in this repository) is the first office-domain scene pack: incident
retrospective, release notes, and tech design review scenes plus the `office-delivery` pipeline.
Install it like any plugin — point the Plugins page GitHub installer at this repository's
`packs/office-starter` path, or copy the directory into your plugins dir — and its scenes appear
as `office-starter:scene:<name>` alongside the builtins. Packs are pure data: installing one
never runs anything.
