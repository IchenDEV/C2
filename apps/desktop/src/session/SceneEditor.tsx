import { useEffect, useState } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field";
import { Plus, Save, Trash2 } from "@/components/ui/icons";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Spinner } from "@/components/ui/spinner";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { TooltipButton } from "@/components/ui/tooltip";

import {
  deleteScene as bridgeDeleteScene,
  getScene as bridgeGetScene,
  saveScene as bridgeSaveScene,
} from "../bridge";
import type { ProviderInfo, SceneSaveScope, SkillInfo } from "../bridge";
import { useT } from "../i18n";
import { td } from "../i18n/dynamic";
import { useToast } from "../ui/toast";
import type {
  SceneArtifactDef,
  SceneDocument,
  SceneExitCriterion,
  SceneHook,
  SceneInfo,
  SceneSlotDef,
} from "./scene";
import {
  createSceneDocument,
  defaultExitCriterion,
  defaultSceneHook,
  duplicateSceneDocument,
  formatSceneJson,
  joinSceneList,
  parseSceneJson,
  splitSceneList,
  validateSceneDocument,
} from "./sceneEditorModel";

export type SceneEditorRequest =
  | { kind: "create" }
  | { kind: "edit"; scene: SceneInfo }
  | { kind: "duplicate"; scene: SceneInfo };

interface SelectOption<T extends string = string> {
  label: string;
  value: T;
}

function OptionalSelectField<T extends string>({
  id,
  label,
  description,
  value,
  options,
  inheritLabel,
  onChange,
}: {
  id: string;
  label: string;
  description?: string;
  value?: T;
  options: readonly SelectOption<T>[];
  inheritLabel: string;
  onChange: (value: T | undefined) => void;
}) {
  const items = [{ label: inheritLabel, value: null }, ...options];
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {description != null && description !== "" && (
        <FieldDescription>{description}</FieldDescription>
      )}
      <Select
        items={items}
        value={value ?? null}
        onValueChange={(next) => {
          if (typeof next !== "string") {
            onChange(undefined);
            return;
          }
          const match = options.find((option) => option.value === next);
          onChange(match?.value);
        }}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent position="popper">
          <SelectGroup>
            {items.map((item) => (
              <SelectItem key={item.value ?? "inherit"} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  );
}

function ListField({
  id,
  label,
  description,
  value,
  multiline = false,
  onChange,
}: {
  id: string;
  label: string;
  description?: string;
  value: string[] | undefined;
  multiline?: boolean;
  onChange: (value: string[]) => void;
}) {
  const shared = {
    id,
    value: multiline ? (value ?? []).join("\n") : joinSceneList(value),
    onChange: (
      event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>
    ) => onChange(splitSceneList(event.currentTarget.value)),
  };
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {description != null && description !== "" && (
        <FieldDescription>{description}</FieldDescription>
      )}
      {multiline ? (
        <Textarea {...shared} className="min-h-24" />
      ) : (
        <Input {...shared} />
      )}
    </Field>
  );
}

function SectionIntro({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="flex flex-col gap-1">
      <h3 className="text-body font-semibold">{title}</h3>
      <p className="text-callout text-muted-foreground">{description}</p>
    </div>
  );
}

function RemoveButton({
  label,
  onClick,
}: {
  label: string;
  onClick: () => void;
}) {
  return (
    <TooltipButton
      label={label}
      variant="ghost"
      size="icon-sm"
      onClick={onClick}
    >
      <Trash2 />
    </TooltipButton>
  );
}

function addAtEnd<T>(current: readonly T[] | undefined, item: T): T[] {
  return [...(current ?? []), item];
}

function updateAt<T>(
  current: readonly T[] | undefined,
  index: number,
  patch: Partial<T>
): T[] {
  return (current ?? []).map((entry, at) =>
    at === index ? { ...entry, ...patch } : entry
  );
}

function removeAt<T>(current: readonly T[] | undefined, index: number): T[] {
  return (current ?? []).filter((_, at) => at !== index);
}

function InlineFragmentEditor({
  scene,
  onChange,
  labels,
}: {
  scene: SceneDocument;
  onChange: (scene: SceneDocument) => void;
  labels: {
    add: string;
    remove: string;
    name: string;
    text: string;
    empty: string;
  };
}) {
  const inline = scene.skills?.inline ?? [];
  const setInline = (next: typeof inline) =>
    onChange({ ...scene, skills: { ...scene.skills, inline: next } });
  return (
    <FieldSet>
      <div className="flex items-center justify-between gap-3">
        <FieldLegend variant="label">{labels.text}</FieldLegend>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setInline(addAtEnd(inline, { name: "", text: "" }))}
        >
          <Plus data-icon="inline-start" />
          {labels.add}
        </Button>
      </div>
      {inline.length === 0 ? (
        <p className="text-callout text-muted-foreground">{labels.empty}</p>
      ) : (
        <div className="flex flex-col gap-3">
          {inline.map((fragment, index) => (
            <div
              key={index}
              className="rounded-control bg-fill-quiet flex gap-2 p-3"
            >
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <Input
                  aria-label={labels.name}
                  placeholder={labels.name}
                  value={fragment.name}
                  onChange={(event) =>
                    setInline(
                      updateAt(inline, index, {
                        name: event.currentTarget.value,
                      })
                    )
                  }
                />
                <Textarea
                  aria-label={labels.text}
                  placeholder={labels.text}
                  value={fragment.text}
                  className="min-h-20"
                  onChange={(event) =>
                    setInline(
                      updateAt(inline, index, {
                        text: event.currentTarget.value,
                      })
                    )
                  }
                />
              </div>
              <RemoveButton
                label={labels.remove}
                onClick={() => setInline(removeAt(inline, index))}
              />
            </div>
          ))}
        </div>
      )}
    </FieldSet>
  );
}

function SlotEditor({
  scene,
  onChange,
  t,
}: {
  scene: SceneDocument;
  onChange: (scene: SceneDocument) => void;
  t: ReturnType<typeof useT>;
}) {
  const slots = scene.brief?.slots ?? [];
  const setSlots = (next: SceneSlotDef[]) =>
    onChange({ ...scene, brief: { ...scene.brief!, slots: next } });
  const kinds: SelectOption[] = [
    "text",
    "multiline",
    "select",
    "file",
    "artifact",
  ].map((value) => ({
    value,
    label: td(t, `sceneEditor.slotKind.${value}`),
  }));
  return (
    <FieldSet>
      <div className="flex items-center justify-between gap-3">
        <FieldLegend variant="label">{t("sceneEditor.slots")}</FieldLegend>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() =>
            setSlots(
              addAtEnd(slots, {
                id: `slot-${slots.length + 1}`,
                label: "",
                kind: "text",
              })
            )
          }
        >
          <Plus data-icon="inline-start" />
          {t("sceneEditor.addSlot")}
        </Button>
      </div>
      {slots.length === 0 ? (
        <p className="text-callout text-muted-foreground">
          {t("sceneEditor.slotsEmpty")}
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {slots.map((slot, index) => (
            <div
              key={index}
              className="rounded-control bg-fill-quiet flex gap-2 p-3"
            >
              <div className="grid min-w-0 flex-1 grid-cols-2 gap-2">
                <Input
                  aria-label={t("sceneEditor.id")}
                  placeholder={t("sceneEditor.id")}
                  value={slot.id}
                  onChange={(event) =>
                    setSlots(
                      updateAt(slots, index, { id: event.currentTarget.value })
                    )
                  }
                />
                <Input
                  aria-label={t("sceneEditor.label")}
                  placeholder={t("sceneEditor.label")}
                  value={slot.label}
                  onChange={(event) =>
                    setSlots(
                      updateAt(slots, index, {
                        label: event.currentTarget.value,
                      })
                    )
                  }
                />
                <Select
                  items={kinds}
                  value={slot.kind}
                  onValueChange={(value) =>
                    value &&
                    setSlots(
                      updateAt(slots, index, {
                        kind: value,
                      })
                    )
                  }
                >
                  <SelectTrigger
                    aria-label={t("sceneEditor.kind")}
                    className="w-full"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent position="popper">
                    <SelectGroup>
                      {kinds.map((kind) => (
                        <SelectItem key={kind.value} value={kind.value}>
                          {kind.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Input
                  aria-label={t("sceneEditor.options")}
                  placeholder={t("sceneEditor.options")}
                  disabled={slot.kind !== "select"}
                  value={joinSceneList(slot.options)}
                  onChange={(event) =>
                    setSlots(
                      updateAt(slots, index, {
                        options: splitSceneList(event.currentTarget.value),
                      })
                    )
                  }
                />
                <Input
                  aria-label={t("sceneEditor.defaultValue")}
                  placeholder={t("sceneEditor.defaultValue")}
                  value={slot.default ?? ""}
                  onChange={(event) =>
                    setSlots(
                      updateAt(slots, index, {
                        default: event.currentTarget.value || undefined,
                      })
                    )
                  }
                />
                <label className="text-body flex items-center gap-2">
                  <Checkbox
                    checked={slot.required ?? false}
                    onCheckedChange={(checked) =>
                      setSlots(updateAt(slots, index, { required: checked }))
                    }
                  />
                  {t("sceneEditor.required")}
                </label>
              </div>
              <RemoveButton
                label={t("sceneEditor.removeSlot")}
                onClick={() => setSlots(removeAt(slots, index))}
              />
            </div>
          ))}
        </div>
      )}
    </FieldSet>
  );
}

function ArtifactEditor({
  scene,
  onChange,
  t,
}: {
  scene: SceneDocument;
  onChange: (scene: SceneDocument) => void;
  t: ReturnType<typeof useT>;
}) {
  const artifacts = scene.artifacts ?? [];
  const setArtifacts = (next: SceneArtifactDef[]) =>
    onChange({ ...scene, artifacts: next });
  const kinds = [
    "document",
    "plan",
    "report",
    "test_report",
    "checklist",
    "diff",
    "link",
    "custom",
  ].map((value) => ({
    value,
    label: td(t, `sceneEditor.artifactKind.${value}`),
  }));
  return (
    <FieldSet>
      <div className="flex items-center justify-between gap-3">
        <FieldLegend variant="label">{t("sceneEditor.artifacts")}</FieldLegend>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() =>
            setArtifacts(
              addAtEnd(artifacts, {
                id: `artifact-${artifacts.length + 1}`,
                title: "",
                kind: "document",
              })
            )
          }
        >
          <Plus data-icon="inline-start" />
          {t("sceneEditor.addArtifact")}
        </Button>
      </div>
      {artifacts.length === 0 ? (
        <p className="text-callout text-muted-foreground">
          {t("sceneEditor.artifactsEmpty")}
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {artifacts.map((artifact, index) => (
            <div
              key={index}
              className="rounded-control bg-fill-quiet flex gap-2 p-3"
            >
              <div className="grid min-w-0 flex-1 grid-cols-2 gap-2">
                <Input
                  aria-label={t("sceneEditor.id")}
                  placeholder={t("sceneEditor.id")}
                  value={artifact.id}
                  onChange={(event) =>
                    setArtifacts(
                      updateAt(artifacts, index, {
                        id: event.currentTarget.value,
                      })
                    )
                  }
                />
                <Input
                  aria-label={t("sceneEditor.titleField")}
                  placeholder={t("sceneEditor.titleField")}
                  value={artifact.title}
                  onChange={(event) =>
                    setArtifacts(
                      updateAt(artifacts, index, {
                        title: event.currentTarget.value,
                      })
                    )
                  }
                />
                <Select
                  items={kinds}
                  value={artifact.kind}
                  onValueChange={(value) =>
                    value &&
                    setArtifacts(
                      updateAt(artifacts, index, {
                        kind: value,
                      })
                    )
                  }
                >
                  <SelectTrigger
                    aria-label={t("sceneEditor.kind")}
                    className="w-full"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent position="popper">
                    <SelectGroup>
                      {kinds.map((kind) => (
                        <SelectItem key={kind.value} value={kind.value}>
                          {kind.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <label className="text-body flex items-center gap-2">
                  <Checkbox
                    checked={artifact.required ?? false}
                    onCheckedChange={(checked) =>
                      setArtifacts(
                        updateAt(artifacts, index, {
                          required: checked,
                        })
                      )
                    }
                  />
                  {t("sceneEditor.required")}
                </label>
                <Textarea
                  className="min-h-20"
                  aria-label={t("sceneEditor.artifactDescription")}
                  placeholder={t("sceneEditor.artifactDescription")}
                  value={artifact.description ?? ""}
                  onChange={(event) =>
                    setArtifacts(
                      updateAt(artifacts, index, {
                        description: event.currentTarget.value || undefined,
                      })
                    )
                  }
                />
                <Textarea
                  className="min-h-20 font-mono"
                  aria-label={t("sceneEditor.artifactTemplate")}
                  placeholder={t("sceneEditor.artifactTemplate")}
                  value={artifact.template ?? ""}
                  onChange={(event) =>
                    setArtifacts(
                      updateAt(artifacts, index, {
                        template: event.currentTarget.value || undefined,
                      })
                    )
                  }
                />
              </div>
              <RemoveButton
                label={t("sceneEditor.removeArtifact")}
                onClick={() => setArtifacts(removeAt(artifacts, index))}
              />
            </div>
          ))}
        </div>
      )}
    </FieldSet>
  );
}

function CriterionEditor({
  scene,
  onChange,
  t,
}: {
  scene: SceneDocument;
  onChange: (scene: SceneDocument) => void;
  t: ReturnType<typeof useT>;
}) {
  const criteria = scene.exit?.criteria ?? [];
  const setCriteria = (next: SceneExitCriterion[]) =>
    onChange({ ...scene, exit: { ...scene.exit, criteria: next } });
  const kinds = [
    "required_artifacts",
    "checklist_complete",
    "tests_pass",
    "user_confirm",
    "custom",
  ].map((value) => ({
    value,
    label: td(t, `sceneEditor.criterion.${value}`),
  }));
  return (
    <FieldSet>
      <div className="flex items-center justify-between gap-3">
        <FieldLegend variant="label">{t("sceneEditor.criteria")}</FieldLegend>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() =>
            setCriteria(addAtEnd(criteria, defaultExitCriterion()))
          }
        >
          <Plus data-icon="inline-start" />
          {t("sceneEditor.addCriterion")}
        </Button>
      </div>
      {criteria.length === 0 ? (
        <p className="text-callout text-muted-foreground">
          {t("sceneEditor.criteriaDefault")}
        </p>
      ) : (
        <div className="flex flex-col gap-2">
          {criteria.map((criterion, index) => (
            <div
              key={index}
              className="rounded-control bg-fill-quiet flex items-center gap-2 px-3 py-2"
            >
              <Select
                items={kinds}
                value={criterion.kind}
                onValueChange={(value) =>
                  value &&
                  setCriteria(
                    updateAt(criteria, index, {
                      kind: value,
                    })
                  )
                }
              >
                <SelectTrigger
                  aria-label={t("sceneEditor.kind")}
                  className="w-52"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent position="popper">
                  <SelectGroup>
                    {kinds.map((kind) => (
                      <SelectItem key={kind.value} value={kind.value}>
                        {kind.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              {criterion.kind === "checklist_complete" && (
                <Input
                  aria-label={t("sceneEditor.artifactId")}
                  placeholder={t("sceneEditor.artifactId")}
                  value={criterion.artifact ?? ""}
                  onChange={(event) =>
                    setCriteria(
                      updateAt(criteria, index, {
                        artifact: event.currentTarget.value || undefined,
                      })
                    )
                  }
                />
              )}
              {criterion.kind === "custom" && (
                <Input
                  aria-label={t("sceneEditor.criterionDescription")}
                  placeholder={t("sceneEditor.criterionDescription")}
                  value={criterion.description ?? ""}
                  onChange={(event) =>
                    setCriteria(
                      updateAt(criteria, index, {
                        description: event.currentTarget.value || undefined,
                      })
                    )
                  }
                />
              )}
              <span className="min-w-0 flex-1" />
              <RemoveButton
                label={t("sceneEditor.removeCriterion")}
                onClick={() => setCriteria(removeAt(criteria, index))}
              />
            </div>
          ))}
        </div>
      )}
    </FieldSet>
  );
}

function HookEditor({
  scene,
  onChange,
  t,
}: {
  scene: SceneDocument;
  onChange: (scene: SceneDocument) => void;
  t: ReturnType<typeof useT>;
}) {
  const hooks = scene.hooks ?? [];
  const setHooks = (next: SceneHook[]) => onChange({ ...scene, hooks: next });
  const events = [
    "enter",
    "turn_end",
    "artifact_produced",
    "exit_criteria_met",
    "tests_failed",
    "schedule",
  ].map((value) => ({
    value,
    label: td(t, `sceneEditor.hookEvent.${value}`),
  }));
  const actions = ["suggest_scene", "suggest_next", "run_macro", "notify"].map(
    (value) => ({ value, label: td(t, `sceneEditor.hookAction.${value}`) })
  );
  return (
    <FieldSet>
      <div className="flex items-center justify-between gap-3">
        <FieldLegend variant="label">{t("sceneEditor.hooks")}</FieldLegend>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setHooks(addAtEnd(hooks, defaultSceneHook()))}
        >
          <Plus data-icon="inline-start" />
          {t("sceneEditor.addHook")}
        </Button>
      </div>
      {hooks.length === 0 ? (
        <p className="text-callout text-muted-foreground">
          {t("sceneEditor.hooksEmpty")}
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {hooks.map((hook, index) => (
            <div
              key={index}
              className="rounded-control bg-fill-quiet flex gap-2 p-3"
            >
              <div className="grid min-w-0 flex-1 grid-cols-2 gap-2">
                <Field>
                  <FieldLabel>{t("sceneEditor.hookEvent")}</FieldLabel>
                  <Select
                    items={events}
                    value={hook.on}
                    onValueChange={(value) =>
                      value && setHooks(updateAt(hooks, index, { on: value }))
                    }
                  >
                    <SelectTrigger
                      aria-label={t("sceneEditor.hookEvent")}
                      className="w-full"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent position="popper">
                      <SelectGroup>
                        {events.map((event) => (
                          <SelectItem key={event.value} value={event.value}>
                            {event.label}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
                <Field>
                  <FieldLabel>{t("sceneEditor.hookAction")}</FieldLabel>
                  <Select
                    items={actions}
                    value={hook.action.kind}
                    onValueChange={(value) =>
                      value &&
                      setHooks(
                        updateAt(hooks, index, {
                          action: { kind: value },
                        })
                      )
                    }
                  >
                    <SelectTrigger
                      aria-label={t("sceneEditor.hookAction")}
                      className="w-full"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent position="popper">
                      <SelectGroup>
                        {actions.map((action) => (
                          <SelectItem key={action.value} value={action.value}>
                            {action.label}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
                {hook.on === "artifact_produced" && (
                  <Input
                    aria-label={t("sceneEditor.artifactId")}
                    placeholder={t("sceneEditor.artifactId")}
                    value={hook.artifact ?? ""}
                    onChange={(event) =>
                      setHooks(
                        updateAt(hooks, index, {
                          artifact: event.currentTarget.value || undefined,
                        })
                      )
                    }
                  />
                )}
                {hook.on === "schedule" && (
                  <Input
                    aria-label={t("sceneEditor.schedule")}
                    placeholder="0 9 * * 1"
                    value={hook.schedule ?? ""}
                    onChange={(event) =>
                      setHooks(
                        updateAt(hooks, index, {
                          schedule: event.currentTarget.value || undefined,
                        })
                      )
                    }
                  />
                )}
                {hook.action.kind === "suggest_scene" && (
                  <Input
                    aria-label={t("sceneEditor.sceneReference")}
                    placeholder={t("sceneEditor.sceneReference")}
                    value={hook.action.scene ?? ""}
                    onChange={(event) =>
                      setHooks(
                        updateAt(hooks, index, {
                          action: {
                            ...hook.action,
                            scene: event.currentTarget.value || undefined,
                          },
                        })
                      )
                    }
                  />
                )}
                {hook.action.kind === "run_macro" && (
                  <Input
                    aria-label={t("sceneEditor.macroReference")}
                    placeholder={t("sceneEditor.macroReference")}
                    value={hook.action.macro ?? ""}
                    onChange={(event) =>
                      setHooks(
                        updateAt(hooks, index, {
                          action: {
                            ...hook.action,
                            macro: event.currentTarget.value || undefined,
                          },
                        })
                      )
                    }
                  />
                )}
                {hook.action.kind === "notify" && (
                  <Input
                    aria-label={t("sceneEditor.message")}
                    placeholder={t("sceneEditor.message")}
                    value={hook.action.message ?? ""}
                    onChange={(event) =>
                      setHooks(
                        updateAt(hooks, index, {
                          action: {
                            ...hook.action,
                            message: event.currentTarget.value || undefined,
                          },
                        })
                      )
                    }
                  />
                )}
              </div>
              <RemoveButton
                label={t("sceneEditor.removeHook")}
                onClick={() => setHooks(removeAt(hooks, index))}
              />
            </div>
          ))}
        </div>
      )}
    </FieldSet>
  );
}

export function SceneEditor({
  request,
  scenes,
  providers,
  skills,
  cwd,
  onSaved,
  onDeleted,
  onClose,
  getScene = bridgeGetScene,
  saveScene = bridgeSaveScene,
  deleteScene = bridgeDeleteScene,
}: {
  request: SceneEditorRequest;
  scenes: SceneInfo[];
  providers: ProviderInfo[];
  skills: SkillInfo[];
  cwd: string;
  onSaved: (scene: SceneInfo) => void;
  onDeleted: (reference: string) => void;
  onClose: () => void;
  getScene?: typeof bridgeGetScene;
  saveScene?: typeof bridgeSaveScene;
  deleteScene?: typeof bridgeDeleteScene;
}) {
  const t = useT();
  const toast = useToast();
  const [loading, setLoading] = useState(request.kind !== "create");
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [scope, setScope] = useState<SceneSaveScope>(
    request.kind === "edit" && request.scene.source === "project"
      ? "project"
      : "user"
  );
  const [draft, setDraft] = useState<SceneDocument>(() =>
    createSceneDocument(scenes)
  );
  const [json, setJson] = useState(() => formatSceneJson(draft));
  const [jsonError, setJsonError] = useState<string | null>(null);
  const [jsonDirty, setJsonDirty] = useState(false);
  const [editorTab, setEditorTab] = useState("identity");
  const showIssue = (field: string) => {
    const section = field.split(".")[0];
    const tab =
      section === "hooks"
        ? "automation"
        : section === "brief"
          ? "brief"
          : section === "artifacts" || section === "exit"
            ? "outputs"
            : section === "skills" || section === "constraints"
              ? "skills"
              : section === "execution"
                ? "execution"
                : section === "$schema"
                  ? "json"
                  : "identity";
    setEditorTab(tab);
    const inputId =
      field === "brief.template"
        ? "scene-brief-template"
        : field === "title"
          ? "scene-title"
          : field === "name"
            ? "scene-name"
            : null;
    if (inputId)
      requestAnimationFrame(() =>
        document.querySelector<HTMLElement>(`#${inputId}`)?.focus()
      );
  };

  const editableOriginal =
    request.kind === "edit" &&
    (request.scene.source === "user" || request.scene.source === "project")
      ? request.scene
      : null;

  useEffect(() => {
    let alive = true;
    if (request.kind === "create") {
      const next = createSceneDocument(scenes);
      setDraft(next);
      setJson(formatSceneJson(next));
      setJsonDirty(false);
      setLoading(false);
      return () => {
        alive = false;
      };
    }
    setLoading(true);
    void getScene(request.scene.reference).then((detail) => {
      if (!alive) return;
      if (!detail) {
        setLoadError(true);
        setLoading(false);
        return;
      }
      const next =
        request.kind === "duplicate"
          ? duplicateSceneDocument(detail.scene, scenes)
          : structuredClone(detail.scene);
      setDraft(next);
      setJson(formatSceneJson(next));
      setJsonDirty(false);
      setScope(
        request.kind === "edit" && request.scene.source === "project"
          ? "project"
          : "user"
      );
      setLoading(false);
    });
    return () => {
      alive = false;
    };
  }, [getScene, request]);

  useEffect(() => {
    if ((jsonError == null || jsonError === "") && !jsonDirty)
      setJson(formatSceneJson(draft));
  }, [draft, jsonDirty, jsonError]);

  const issues = validateSceneDocument(draft);
  const issueMessages = issues.map((issue) => td(t, issue.key, issue.vars));
  const providerHint = providers
    .map((provider) => `${provider.id} (${provider.display_name})`)
    .join(", ");
  const skillHint = skills
    .slice(0, 8)
    .map((skill) => skill.id)
    .join(", ");
  const originalName = editableOriginal?.name ?? null;

  const applyJson = () => {
    const parsed = parseSceneJson(json);
    if (!parsed.scene) {
      setJsonError(parsed.error ?? t("sceneEditor.jsonInvalid"));
      return;
    }
    setJsonError(null);
    setJsonDirty(false);
    setDraft(parsed.scene);
  };

  const save = async () => {
    if (
      issues.length > 0 ||
      (jsonError != null && jsonError !== "") ||
      jsonDirty
    )
      return;
    setSaving(true);
    try {
      const saved = await saveScene(scope, cwd || null, originalName, draft);
      toast(t("sceneEditor.saved", { name: saved.title }), "success");
      onSaved(saved);
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!editableOriginal) return;
    const saveScope =
      editableOriginal.source === "user" ||
      editableOriginal.source === "project"
        ? editableOriginal.source
        : null;
    if (saveScope == null) return;
    setDeleting(true);
    try {
      await deleteScene(saveScope, cwd || null, editableOriginal.name);
      onDeleted(editableOriginal.reference);
      toast(
        t("sceneEditor.deleted", { name: editableOriginal.title }),
        "success"
      );
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), "error");
      setDeleting(false);
    }
  };

  const execution = draft.execution ?? {};
  const modeOptions = (
    ["read_only", "ask", "auto_edit", "full_access"] as const
  ).map((value) => ({ value, label: td(t, `mode.${value}`) }));
  const memoryOptions = (
    ["standard", "read_only", "private", "learn_only"] as const
  ).map((value) => ({ value, label: td(t, `sceneEditor.memory.${value}`) }));
  const worktreeOptions = (["off", "current", "origin_default"] as const).map(
    (value) => ({
      value,
      label: td(t, `sceneEditor.worktree.${value}`),
    })
  );
  const editorTitle =
    request.kind === "edit"
      ? t("sceneEditor.editTitle")
      : request.kind === "duplicate"
        ? t("sceneEditor.duplicateTitle")
        : t("sceneEditor.createTitle");

  return (
    <section
      className="bg-background flex min-h-0 flex-1 flex-col"
      aria-label={editorTitle}
    >
      {loading ? (
        <div
          className="text-body text-muted-foreground flex min-h-0 flex-1 items-center justify-center gap-2"
          role="status"
        >
          <Spinner />
          {t("sceneEditor.loading")}
        </div>
      ) : loadError ? (
        <div className="text-body text-destructive flex min-h-0 flex-1 items-center justify-center px-6">
          {t("sceneEditor.loadError")}
        </div>
      ) : (
        <Tabs
          value={editorTab}
          onValueChange={(value) => {
            if (typeof value === "string") setEditorTab(value);
          }}
          className="min-h-0 flex-1 flex-col gap-0"
        >
          <div className="bg-card/30 shrink-0">
            <div className="mx-auto w-full max-w-4xl px-8 pt-7">
              <div className="flex flex-col gap-1">
                <h1 className="text-page font-semibold tracking-tight">
                  {editorTitle}
                </h1>
                <p className="text-metadata text-muted-foreground">
                  {t("sceneEditor.description")}
                </p>
              </div>
              <TabsList
                variant="line"
                className="mt-5 max-w-full justify-start overflow-x-auto pb-2"
              >
                {(
                  [
                    "identity",
                    "execution",
                    "skills",
                    "brief",
                    "outputs",
                    "automation",
                    "json",
                  ] as const
                ).map((tab) => (
                  <TabsTrigger key={tab} value={tab}>
                    {td(t, `sceneEditor.tab.${tab}`)}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
            <Separator />
          </div>

          <ScrollArea className="min-h-0 flex-1">
            <div className="mx-auto w-full max-w-3xl px-8 py-8">
              {issueMessages.length > 0 && (
                <details className="text-destructive text-body mb-4" open>
                  <summary>{t("sceneEditor.validationIssues")}</summary>
                  <ul>
                    {issues.map((issue, index) => (
                      <li key={`${issue.field}-${index}`}>
                        <Button
                          type="button"
                          variant="link"
                          size="sm"
                          className="text-destructive h-auto text-left whitespace-normal"
                          onClick={() => showIssue(issue.field)}
                        >
                          {issueMessages[index]} · {issue.field}
                        </Button>
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              <TabsContent value="identity">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.identityTitle")}
                    description={t("sceneEditor.identityDescription")}
                  />
                  <Field
                    data-invalid={issues.some(
                      (issue) => issue.field === "title"
                    )}
                  >
                    <FieldLabel htmlFor="scene-title">
                      {t("sceneEditor.titleField")}
                    </FieldLabel>
                    <Input
                      id="scene-title"
                      value={draft.title}
                      aria-invalid={issues.some(
                        (issue) => issue.field === "title"
                      )}
                      onInput={(event) =>
                        setDraft({ ...draft, title: event.currentTarget.value })
                      }
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="scene-description">
                      {t("sceneEditor.descriptionField")}
                    </FieldLabel>
                    <Textarea
                      id="scene-description"
                      className="min-h-24"
                      value={draft.description ?? ""}
                      onChange={(event) =>
                        setDraft({
                          ...draft,
                          description: event.currentTarget.value,
                        })
                      }
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="scene-scope">
                      {t("sceneEditor.scope")}
                    </FieldLabel>
                    <FieldDescription>
                      {editableOriginal
                        ? t("sceneEditor.scopeLocked")
                        : t("sceneEditor.scopeDescription")}
                    </FieldDescription>
                    <Select
                      items={[
                        { value: "user", label: t("scene.source.user") },
                        { value: "project", label: t("scene.source.project") },
                      ]}
                      value={scope}
                      disabled={Boolean(editableOriginal)}
                      onValueChange={(value) => value && setScope(value)}
                    >
                      <SelectTrigger id="scene-scope" className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent position="popper">
                        <SelectGroup>
                          <SelectItem value="user">
                            {t("scene.source.user")}
                          </SelectItem>
                          <SelectItem value="project">
                            {t("scene.source.project")}
                          </SelectItem>
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </Field>
                  <div className="grid grid-cols-2 gap-4">
                    <Field
                      data-invalid={issues.some(
                        (issue) => issue.field === "name"
                      )}
                    >
                      <FieldLabel htmlFor="scene-name">
                        {t("sceneEditor.name")}
                      </FieldLabel>
                      <Input
                        id="scene-name"
                        value={draft.name}
                        aria-invalid={issues.some(
                          (issue) => issue.field === "name"
                        )}
                        onChange={(event) =>
                          setDraft({
                            ...draft,
                            name: event.currentTarget.value,
                          })
                        }
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="scene-version">
                        {t("sceneEditor.version")}
                      </FieldLabel>
                      <Input
                        id="scene-version"
                        value={draft.version ?? ""}
                        onChange={(event) =>
                          setDraft({
                            ...draft,
                            version: event.currentTarget.value || undefined,
                          })
                        }
                      />
                    </Field>
                  </div>
                  <ListField
                    id="scene-keywords"
                    label={t("sceneEditor.keywords")}
                    description={t("sceneEditor.keywordsDescription")}
                    value={draft.keywords}
                    onChange={(keywords) => setDraft({ ...draft, keywords })}
                  />
                </FieldGroup>
              </TabsContent>

              <TabsContent value="execution">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.executionTitle")}
                    description={t("sceneEditor.executionDescription")}
                  />
                  <p className="text-body text-muted-foreground">
                    {t("sceneEditor.providerOrderHint")}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    {providers.map((provider) => {
                      const index =
                        execution.providers?.indexOf(provider.id) ?? -1;
                      return (
                        <Button
                          key={provider.id}
                          variant={index >= 0 ? "secondary" : "outline"}
                          size="sm"
                          aria-pressed={index >= 0}
                          onClick={() =>
                            setDraft({
                              ...draft,
                              execution: {
                                ...execution,
                                providers:
                                  index >= 0
                                    ? execution.providers?.filter(
                                        (id) => id !== provider.id
                                      )
                                    : [
                                        ...(execution.providers ?? []),
                                        provider.id,
                                      ],
                              },
                            })
                          }
                        >
                          {index >= 0 ? `${index + 1}. ` : ""}
                          {provider.display_name}
                        </Button>
                      );
                    })}
                  </div>
                  <details>
                    <summary className="text-metadata text-muted-foreground cursor-pointer">
                      {t("sceneEditor.referencesAdvanced")}
                    </summary>
                    <ListField
                      id="scene-providers"
                      label={t("sceneEditor.providers")}
                      description={t("sceneEditor.providersDescription", {
                        providers:
                          providerHint || t("sceneEditor.noneAvailable"),
                      })}
                      value={execution.providers}
                      onChange={(providers) =>
                        setDraft({
                          ...draft,
                          execution: { ...execution, providers },
                        })
                      }
                    />
                  </details>
                  <div className="grid grid-cols-2 gap-4">
                    <Field>
                      <FieldLabel htmlFor="scene-model">
                        {t("sceneEditor.model")}
                      </FieldLabel>
                      <Input
                        id="scene-model"
                        list="scene-model-options"
                        value={execution.model ?? ""}
                        placeholder={t("sceneEditor.inherit")}
                        onChange={(event) =>
                          setDraft({
                            ...draft,
                            execution: {
                              ...execution,
                              model: event.currentTarget.value || undefined,
                            },
                          })
                        }
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="scene-effort">
                        {t("sceneEditor.reasoningEffort")}
                      </FieldLabel>
                      <Input
                        id="scene-effort"
                        list="scene-effort-options"
                        value={execution.reasoning_effort ?? ""}
                        placeholder={t("sceneEditor.inherit")}
                        onChange={(event) =>
                          setDraft({
                            ...draft,
                            execution: {
                              ...execution,
                              reasoning_effort:
                                event.currentTarget.value || undefined,
                            },
                          })
                        }
                      />
                    </Field>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <OptionalSelectField
                      id="scene-mode"
                      label={t("sceneEditor.permissionMode")}
                      value={execution.session_mode}
                      options={modeOptions}
                      inheritLabel={t("sceneEditor.inherit")}
                      onChange={(session_mode) =>
                        setDraft({
                          ...draft,
                          execution: {
                            ...execution,
                            session_mode:
                              session_mode as typeof execution.session_mode,
                          },
                        })
                      }
                    />
                    <OptionalSelectField
                      id="scene-memory"
                      label={t("sceneEditor.memoryPreset")}
                      value={execution.memory_preset}
                      options={memoryOptions}
                      inheritLabel={t("sceneEditor.inherit")}
                      onChange={(memory_preset) =>
                        setDraft({
                          ...draft,
                          execution: {
                            ...execution,
                            memory_preset:
                              memory_preset as typeof execution.memory_preset,
                          },
                        })
                      }
                    />
                    <OptionalSelectField
                      id="scene-worktree"
                      label={t("sceneEditor.worktreeMode")}
                      value={execution.worktree}
                      options={worktreeOptions}
                      inheritLabel={t("sceneEditor.inherit")}
                      onChange={(worktree) =>
                        setDraft({
                          ...draft,
                          execution: {
                            ...execution,
                            worktree: worktree as typeof execution.worktree,
                          },
                        })
                      }
                    />
                  </div>
                </FieldGroup>
              </TabsContent>

              <datalist id="scene-model-options">
                {[
                  ...new Map(
                    providers
                      .flatMap((provider) => provider.models)
                      .map((model) => [model.id, model])
                  ).values(),
                ].map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.name}
                  </option>
                ))}
              </datalist>
              <datalist id="scene-effort-options">
                {["low", "medium", "high", "xhigh", "max", "ultra"].map(
                  (effort) => (
                    <option key={effort} value={effort} />
                  )
                )}
              </datalist>
              <TabsContent value="skills">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.skillsTitle")}
                    description={t("sceneEditor.skillsDescription")}
                  />
                  <div className="max-h-48 space-y-2 overflow-y-auto">
                    {skills.map((skill) => (
                      <Field key={skill.id} orientation="horizontal">
                        <Checkbox
                          id={`scene-skill-${skill.id}`}
                          checked={
                            draft.skills?.pinned?.includes(skill.id) ?? false
                          }
                          onCheckedChange={(checked) =>
                            setDraft({
                              ...draft,
                              skills: {
                                ...draft.skills,
                                pinned: checked
                                  ? [...(draft.skills?.pinned ?? []), skill.id]
                                  : (draft.skills?.pinned ?? []).filter(
                                      (id) => id !== skill.id
                                    ),
                              },
                            })
                          }
                        />
                        <FieldLabel htmlFor={`scene-skill-${skill.id}`}>
                          {skill.name}
                        </FieldLabel>
                      </Field>
                    ))}
                  </div>
                  <details>
                    <summary className="text-metadata text-muted-foreground cursor-pointer">
                      {t("sceneEditor.referencesAdvanced")}
                    </summary>
                    <ListField
                      id="scene-pinned-skills"
                      label={t("sceneEditor.pinnedSkills")}
                      description={t("sceneEditor.pinnedSkillsDescription", {
                        skills: skillHint || t("sceneEditor.noneAvailable"),
                      })}
                      value={draft.skills?.pinned}
                      onChange={(pinned) =>
                        setDraft({
                          ...draft,
                          skills: { ...draft.skills, pinned },
                        })
                      }
                    />
                  </details>
                  <Field orientation="horizontal">
                    <Checkbox
                      id="scene-suppress-skills"
                      checked={draft.skills?.suppress_unpinned ?? false}
                      onCheckedChange={(checked) =>
                        setDraft({
                          ...draft,
                          skills: {
                            ...draft.skills,
                            suppress_unpinned: checked,
                          },
                        })
                      }
                    />
                    <FieldLabel htmlFor="scene-suppress-skills">
                      {t("sceneEditor.suppressUnpinned")}
                    </FieldLabel>
                  </Field>
                  <Separator />
                  <InlineFragmentEditor
                    scene={draft}
                    onChange={setDraft}
                    labels={{
                      add: t("sceneEditor.addInstruction"),
                      remove: t("sceneEditor.removeInstruction"),
                      name: t("sceneEditor.instructionName"),
                      text: t("sceneEditor.inlineInstructions"),
                      empty: t("sceneEditor.instructionsEmpty"),
                    }}
                  />
                  <Separator />
                  <ListField
                    id="scene-guardrails"
                    label={t("sceneEditor.guardrails")}
                    description={t("sceneEditor.guardrailsDescription")}
                    value={draft.constraints?.guardrails}
                    multiline
                    onChange={(guardrails) =>
                      setDraft({
                        ...draft,
                        constraints: { ...draft.constraints, guardrails },
                      })
                    }
                  />
                  <div className="grid grid-cols-2 gap-4">
                    <ListField
                      id="scene-tools-allow"
                      label={t("sceneEditor.toolsAllow")}
                      description={t("sceneEditor.toolsReferenceHint")}
                      value={draft.constraints?.tools?.allow}
                      onChange={(allow) =>
                        setDraft({
                          ...draft,
                          constraints: {
                            ...draft.constraints,
                            tools: { ...draft.constraints?.tools, allow },
                          },
                        })
                      }
                    />
                    <ListField
                      id="scene-tools-deny"
                      label={t("sceneEditor.toolsDeny")}
                      value={draft.constraints?.tools?.deny}
                      onChange={(deny) =>
                        setDraft({
                          ...draft,
                          constraints: {
                            ...draft.constraints,
                            tools: { ...draft.constraints?.tools, deny },
                          },
                        })
                      }
                    />
                  </div>
                </FieldGroup>
              </TabsContent>

              <TabsContent value="brief">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.briefTitle")}
                    description={t("sceneEditor.briefDescription")}
                  />
                  <Field orientation="horizontal">
                    <Checkbox
                      id="scene-brief-enabled"
                      checked={Boolean(draft.brief)}
                      onCheckedChange={(checked) =>
                        setDraft({
                          ...draft,
                          brief: checked
                            ? {
                                template: "{{task}}",
                                slots: [
                                  {
                                    id: "task",
                                    label: t("taskboard.taskLabel"),
                                    kind: "text",
                                    required: true,
                                  },
                                ],
                                clarify: "multi_choice",
                              }
                            : undefined,
                        })
                      }
                    />
                    <FieldLabel htmlFor="scene-brief-enabled">
                      {t("sceneEditor.enableBrief")}
                    </FieldLabel>
                  </Field>
                  {draft.brief && (
                    <>
                      <Field
                        data-invalid={issues.some(
                          (issue) => issue.field === "brief.template"
                        )}
                      >
                        <FieldLabel htmlFor="scene-brief-template">
                          {t("sceneEditor.briefTemplate")}
                        </FieldLabel>
                        <FieldDescription>
                          {t("sceneEditor.briefTemplateDescription")}
                        </FieldDescription>
                        <Textarea
                          id="scene-brief-template"
                          className="min-h-40 font-mono"
                          aria-invalid={issues.some(
                            (issue) => issue.field === "brief.template"
                          )}
                          value={draft.brief.template}
                          onChange={(event) =>
                            setDraft({
                              ...draft,
                              brief: {
                                ...draft.brief!,
                                template: event.currentTarget.value,
                              },
                            })
                          }
                        />
                      </Field>
                      <OptionalSelectField
                        id="scene-clarify"
                        label={t("sceneEditor.clarify")}
                        value={draft.brief.clarify}
                        options={(
                          ["multi_choice", "free_form", "off"] as const
                        ).map((value) => ({
                          value,
                          label: td(t, `sceneEditor.clarify.${value}`),
                        }))}
                        inheritLabel={t("sceneEditor.inheritDefault")}
                        onChange={(clarify) =>
                          setDraft({
                            ...draft,
                            brief: {
                              ...draft.brief!,
                              clarify: clarify as typeof draft.brief.clarify,
                            },
                          })
                        }
                      />
                      <Separator />
                      <SlotEditor scene={draft} onChange={setDraft} t={t} />
                      <details open>
                        <summary>{t("sceneEditor.preview")}</summary>
                        <pre className="rounded-control bg-fill-rest text-body p-3 break-words whitespace-pre-wrap">
                          {draft.brief.template.replaceAll(
                            /\{\{\s*([\w-]+)\s*\}\}/gu,
                            (match, id: string) => {
                              const slot = draft.brief?.slots?.find(
                                (entry) => entry.id === id
                              );
                              return (
                                slot?.default ??
                                (slot ? `[${slot.label}]` : match)
                              );
                            }
                          )}
                        </pre>
                      </details>
                    </>
                  )}
                </FieldGroup>
              </TabsContent>

              <TabsContent value="outputs">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.outputsTitle")}
                    description={t("sceneEditor.outputsDescription")}
                  />
                  <ArtifactEditor scene={draft} onChange={setDraft} t={t} />
                  <Separator />
                  <CriterionEditor scene={draft} onChange={setDraft} t={t} />
                  <Separator />
                  <Select
                    value={null}
                    onValueChange={(reference) => {
                      if (reference != null && reference !== "")
                        setDraft({
                          ...draft,
                          exit: {
                            ...draft.exit,
                            next: [
                              ...(draft.exit?.next ?? []),
                              { scene: reference },
                            ],
                          },
                        });
                    }}
                  >
                    <SelectTrigger aria-label={t("sceneEditor.nextScenes")}>
                      <SelectValue placeholder={t("sceneEditor.nextScenes")} />
                    </SelectTrigger>
                    <SelectContent>
                      {scenes.map((scene) => (
                        <SelectItem
                          key={scene.reference}
                          value={scene.reference}
                          disabled={draft.exit?.next?.some(
                            (entry) => entry.scene === scene.reference
                          )}
                        >
                          {scene.title}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <ListField
                    id="scene-next"
                    label={t("sceneEditor.nextScenes")}
                    description={t("sceneEditor.nextScenesDescription")}
                    value={(draft.exit?.next ?? []).map((entry) => entry.scene)}
                    onChange={(next) =>
                      setDraft({
                        ...draft,
                        exit: {
                          ...draft.exit,
                          next: next.map(
                            (scene) =>
                              draft.exit?.next?.find(
                                (entry) => entry.scene === scene
                              ) ?? { scene }
                          ),
                        },
                      })
                    }
                  />
                  {(draft.exit?.next ?? []).map((entry, index) => (
                    <fieldset
                      key={`${entry.scene}-${index}`}
                      className="space-y-2"
                    >
                      <legend className="text-body font-medium">
                        {entry.scene} · {t("sceneEditor.carryArtifacts")}
                      </legend>
                      {(draft.artifacts ?? []).map((artifact) => (
                        <Field key={artifact.id} orientation="horizontal">
                          <Checkbox
                            id={`carry-${index}-${artifact.id}`}
                            checked={
                              entry.carry?.includes(artifact.id) ?? false
                            }
                            onCheckedChange={(checked) =>
                              setDraft({
                                ...draft,
                                exit: {
                                  ...draft.exit,
                                  next: draft.exit?.next?.map((next, at) =>
                                    at === index
                                      ? {
                                          ...next,
                                          carry: checked
                                            ? [
                                                ...(next.carry ?? []),
                                                artifact.id,
                                              ]
                                            : (next.carry ?? []).filter(
                                                (id) => id !== artifact.id
                                              ),
                                        }
                                      : next
                                  ),
                                },
                              })
                            }
                          />
                          <FieldLabel htmlFor={`carry-${index}-${artifact.id}`}>
                            {artifact.id}
                          </FieldLabel>
                        </Field>
                      ))}
                      {(draft.artifacts?.length ?? 0) === 0 && (
                        <p className="text-body text-muted-foreground">
                          {t("sceneEditor.artifactsEmpty")}
                        </p>
                      )}
                    </fieldset>
                  ))}
                </FieldGroup>
              </TabsContent>

              <TabsContent value="automation">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.automationTitle")}
                    description={t("sceneEditor.automationDescription")}
                  />
                  <HookEditor scene={draft} onChange={setDraft} t={t} />
                </FieldGroup>
              </TabsContent>

              <TabsContent value="json">
                <FieldGroup>
                  <SectionIntro
                    title={t("sceneEditor.jsonTitle")}
                    description={t("sceneEditor.jsonDescription")}
                  />
                  <Field data-invalid={Boolean(jsonError)}>
                    <FieldLabel htmlFor="scene-json">
                      {t("sceneEditor.jsonDocument")}
                    </FieldLabel>
                    <Textarea
                      id="scene-json"
                      className="text-callout min-h-96 font-mono"
                      value={json}
                      aria-invalid={Boolean(jsonError)}
                      onChange={(event) => {
                        setJson(event.currentTarget.value);
                        setJsonError(null);
                        setJsonDirty(true);
                      }}
                    />
                    {jsonError != null && jsonError !== "" && (
                      <FieldError>{jsonError}</FieldError>
                    )}
                  </Field>
                  <Button type="button" variant="outline" onClick={applyJson}>
                    {t("sceneEditor.applyJson")}
                  </Button>
                </FieldGroup>
              </TabsContent>
            </div>
          </ScrollArea>
        </Tabs>
      )}

      <Separator />
      <footer className="bg-card/40 flex shrink-0 items-center gap-2 px-8 py-3">
        <div className="min-w-0 flex-1 text-left">
          {(issueMessages.length > 0 || jsonDirty) && !loading && (
            <p
              role="alert"
              className="text-callout text-destructive truncate"
              title={
                jsonDirty
                  ? t("sceneEditor.jsonUnapplied")
                  : issueMessages.join("\n")
              }
            >
              {t("sceneEditor.validationSummary", {
                count: issueMessages.length + (jsonDirty ? 1 : 0),
                error: jsonDirty
                  ? t("sceneEditor.jsonUnapplied")
                  : issueMessages[0],
              })}
            </p>
          )}
        </div>
        {editableOriginal && (
          <AlertDialog>
            <AlertDialogTrigger
              render={
                <Button
                  type="button"
                  variant="ghost"
                  className="text-destructive"
                />
              }
            >
              <Trash2 data-icon="inline-start" />
              {t("sceneEditor.delete")}
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  {t("sceneEditor.deleteTitle")}
                </AlertDialogTitle>
                <AlertDialogDescription>
                  {t("sceneEditor.deleteDescription", {
                    name: editableOriginal.title,
                  })}
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{t("sceneEditor.cancel")}</AlertDialogCancel>
                <AlertDialogAction
                  variant="destructive"
                  disabled={deleting}
                  onClick={() => void remove()}
                >
                  {deleting && <Spinner data-icon="inline-start" />}
                  {t("sceneEditor.delete")}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
        <Button type="button" variant="ghost" onClick={onClose}>
          {t("sceneEditor.cancel")}
        </Button>
        <Button
          type="button"
          disabled={
            loading ||
            saving ||
            issues.length > 0 ||
            Boolean(jsonError) ||
            jsonDirty
          }
          onClick={() => void save()}
        >
          {saving ? (
            <Spinner data-icon="inline-start" />
          ) : (
            <Save data-icon="inline-start" />
          )}
          {t("sceneEditor.save")}
        </Button>
      </footer>
    </section>
  );
}
