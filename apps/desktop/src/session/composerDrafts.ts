import type {
  DocBlock,
  MemoryAccess,
  PermissionMode,
  Sandbox,
  WorktreeBaselineKind,
} from "../bridge";
import { asJsonObject } from "../lib/jsonValue";

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

export type ComposerDraftScope =
  | { kind: "project"; projectPath: string }
  | { kind: "session"; sessionId: string; projectPath: string | null };

export interface ComposerDraftAttachment {
  id: string;
  kind: "appshot" | "attachment";
  name: string;
}

export interface ComposerDraftPosture {
  provider: string;
  model: string | null;
  mode: PermissionMode;
  sandbox: Sandbox;
  worktreeBase: WorktreeBaselineKind | null;
  memoryRead: MemoryAccess;
  memoryWrite: MemoryAccess;
  scene: string | null;
  autoScene: boolean;
}

export interface ComposerDraft {
  id: string;
  scope: ComposerDraftScope;
  doc: DocBlock[];
  attachments: ComposerDraftAttachment[];
  posture: ComposerDraftPosture;
  updatedAt: number;
}

export type ComposerDraftLoadWarning = "corrupt" | "unavailable";

export interface ComposerDraftCollection {
  drafts: Map<string, ComposerDraft>;
  warning: ComposerDraftLoadWarning | null;
}

export type ComposerDraftPromotion =
  | { outcome: "moved"; drafts: Map<string, ComposerDraft> }
  | { outcome: "missing" | "conflict"; drafts: Map<string, ComposerDraft> };

export const COMPOSER_DRAFT_STORAGE_KEY = "codetwo.composerDrafts:v1";
const COMPOSER_DRAFT_VERSION = 1 as const;
const MAX_DRAFTS = 100;
const MAX_RAW_BYTES = 4 * 1024 * 1024;
const MAX_BLOCKS = 2000;
const MAX_ATTACHMENTS = 64;

function serializedBytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

interface ComposerDraftSnapshot {
  version: typeof COMPOSER_DRAFT_VERSION;
  drafts: ComposerDraft[];
}

function stringWithin(value: unknown, max: number): value is string {
  return typeof value === "string" && value.length <= max;
}

function nullableStringWithin(
  value: unknown,
  max: number
): value is string | null {
  return value === null || stringWithin(value, max);
}

function parseScope(value: unknown): ComposerDraftScope | null {
  const scope = asJsonObject(value);
  if (scope == null) return null;
  if (
    scope.kind === "project" &&
    stringWithin(scope.projectPath, 4096) &&
    typeof scope.projectPath === "string" &&
    scope.projectPath
  ) {
    return { kind: "project", projectPath: scope.projectPath };
  }
  if (
    scope.kind === "session" &&
    stringWithin(scope.sessionId, 256) &&
    typeof scope.sessionId === "string" &&
    scope.sessionId &&
    nullableStringWithin(scope.projectPath, 4096)
  ) {
    return {
      kind: "session",
      sessionId: scope.sessionId,
      projectPath:
        typeof scope.projectPath === "string" || scope.projectPath === null
          ? scope.projectPath
          : null,
    };
  }
  return null;
}

function stringRecord(value: unknown): value is Record<string, string> {
  const record = asJsonObject(value);
  if (record == null) return false;
  return Object.entries(record).every(
    ([key, item]) => stringWithin(key, 256) && stringWithin(item, 16_384)
  );
}

function parseDocBlock(value: unknown): DocBlock | null {
  const block = asJsonObject(value);
  if (block == null) return null;
  switch (block.type) {
    case "text": {
      return stringWithin(block.text, 1_048_576)
        ? { type: "text", text: block.text }
        : null;
    }
    case "skill": {
      return stringWithin(block.skill_id, 512) && stringRecord(block.params)
        ? {
            type: "skill",
            skill_id: block.skill_id,
            params: { ...block.params },
          }
        : null;
    }
    case "file":
    case "image": {
      return stringWithin(block.path, 16_384)
        ? { type: block.type, path: block.path }
        : null;
    }
    case "appshot": {
      return stringWithin(block.id, 256) &&
        (block.title === undefined || stringWithin(block.title, 512))
        ? {
            type: "appshot",
            id: block.id,
            ...(block.title != null && block.title !== ""
              ? { title: block.title }
              : {}),
          }
        : null;
    }
    case "attachment": {
      return stringWithin(block.id, 256) &&
        (block.name === undefined || stringWithin(block.name, 512))
        ? {
            type: "attachment",
            id: block.id,
            ...(block.name != null && block.name !== ""
              ? { name: block.name }
              : {}),
          }
        : null;
    }
    case "canvas": {
      const revision = block.frozen_revision;
      const pixelPolicy = block.pixel_policy;
      return stringWithin(block.id, 256) &&
        typeof revision === "number" &&
        Number.isSafeInteger(revision) &&
        revision >= 0 &&
        (pixelPolicy === undefined ||
          pixelPolicy === "required" ||
          pixelPolicy === "structure_only")
        ? {
            type: "canvas",
            id: block.id,
            frozen_revision: revision,
            ...(pixelPolicy ? { pixel_policy: pixelPolicy } : {}),
          }
        : null;
    }
    case "session": {
      const throughSeq = block.through_seq;
      return stringWithin(block.session_id, 256) &&
        (throughSeq === undefined ||
          (typeof throughSeq === "number" &&
            Number.isSafeInteger(throughSeq) &&
            throughSeq > 0))
        ? {
            type: "session",
            session_id: block.session_id,
            ...(typeof throughSeq === "number"
              ? { through_seq: throughSeq }
              : {}),
          }
        : null;
    }
    case "issue": {
      return stringWithin(block.source, 256) &&
        stringWithin(block.id, 512) &&
        stringWithin(block.title, 4096) &&
        stringWithin(block.url, 16_384) &&
        stringWithin(block.body, 1_048_576)
        ? {
            type: "issue",
            source: block.source,
            id: block.id,
            title: block.title,
            url: block.url,
            body: block.body,
          }
        : null;
    }
    default: {
      return null;
    }
  }
}

function isPermissionMode(value: unknown): value is PermissionMode {
  return value === "ask" || value === "accept_edits" || value === "yolo";
}

function isSandbox(value: unknown): value is Sandbox {
  return (
    value === "read_only" ||
    value === "workspace_write" ||
    value === "danger_full_access"
  );
}

function isMemoryAccess(value: unknown): value is MemoryAccess {
  return value === "inherit" || value === "allow" || value === "deny";
}

function parseAttachment(value: unknown): ComposerDraftAttachment | null {
  const attachment = asJsonObject(value);
  if (attachment == null) return null;
  return stringWithin(attachment.id, 256) &&
    typeof attachment.id === "string" &&
    attachment.id &&
    (attachment.kind === "appshot" || attachment.kind === "attachment") &&
    stringWithin(attachment.name, 512) &&
    typeof attachment.name === "string"
    ? { id: attachment.id, kind: attachment.kind, name: attachment.name }
    : null;
}

function parsePosture(value: unknown): ComposerDraftPosture | null {
  const posture = asJsonObject(value);
  if (posture == null) return null;
  if (
    !stringWithin(posture.provider, 128) ||
    typeof posture.provider !== "string" ||
    !posture.provider ||
    !nullableStringWithin(posture.model, 512) ||
    !isPermissionMode(posture.mode) ||
    !isSandbox(posture.sandbox) ||
    !(
      posture.worktreeBase === null ||
      posture.worktreeBase === "current" ||
      posture.worktreeBase === "origin_default"
    ) ||
    !isMemoryAccess(posture.memoryRead) ||
    !isMemoryAccess(posture.memoryWrite) ||
    !nullableStringWithin(posture.scene, 512) ||
    typeof posture.autoScene !== "boolean"
  ) {
    return null;
  }
  return {
    provider: posture.provider,
    model:
      typeof posture.model === "string" || posture.model === null
        ? posture.model
        : null,
    mode: posture.mode,
    sandbox: posture.sandbox,
    worktreeBase: posture.worktreeBase,
    memoryRead: posture.memoryRead,
    memoryWrite: posture.memoryWrite,
    scene:
      typeof posture.scene === "string" || posture.scene === null
        ? posture.scene
        : null,
    autoScene: posture.autoScene,
  };
}

function parseDraft(value: unknown): ComposerDraft | null {
  const draft = asJsonObject(value);
  if (draft == null) return null;
  const scope = parseScope(draft.scope);
  const posture = parsePosture(draft.posture);
  if (
    !stringWithin(draft.id, 256) ||
    typeof draft.id !== "string" ||
    !draft.id ||
    !scope ||
    !posture ||
    !Array.isArray(draft.doc) ||
    draft.doc.length > MAX_BLOCKS ||
    !Array.isArray(draft.attachments) ||
    draft.attachments.length > MAX_ATTACHMENTS ||
    typeof draft.updatedAt !== "number" ||
    !Number.isSafeInteger(draft.updatedAt) ||
    draft.updatedAt < 0
  ) {
    return null;
  }
  const doc = draft.doc.map(parseDocBlock);
  const attachments = draft.attachments.map(parseAttachment);
  if (
    doc.some((block) => block === null) ||
    attachments.some((item) => item === null)
  )
    return null;
  return {
    id: draft.id,
    scope,
    doc: doc.filter((block): block is DocBlock => block !== null),
    attachments: attachments.filter(
      (item): item is ComposerDraftAttachment => item !== null
    ),
    posture,
    updatedAt: draft.updatedAt,
  };
}

function defaultStorage(): StorageLike | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function cloneBlock(block: DocBlock): DocBlock {
  return block.type === "skill"
    ? { ...block, params: { ...block.params } }
    : { ...block };
}

function cloneDraft(draft: ComposerDraft): ComposerDraft {
  return {
    ...draft,
    scope: { ...draft.scope },
    doc: draft.doc.map(cloneBlock),
    attachments: draft.attachments.map((attachment) => ({ ...attachment })),
    posture: { ...draft.posture },
  };
}

export function composerDraftScopeKey(scope: ComposerDraftScope): string {
  return scope.kind === "project"
    ? `project:${encodeURIComponent(scope.projectPath)}`
    : `session:${encodeURIComponent(scope.sessionId)}`;
}

export function composerDraftIsInvested(
  doc: readonly DocBlock[],
  attachments: readonly ComposerDraftAttachment[]
): boolean {
  return doc.length > 0 || attachments.length > 0;
}

export function updateComposerDraft(
  drafts: ReadonlyMap<string, ComposerDraft>,
  input: Omit<ComposerDraft, "id" | "updatedAt">,
  options: { now?: number; createId?: () => string } = {}
): Map<string, ComposerDraft> {
  const next = new Map(drafts);
  const key = composerDraftScopeKey(input.scope);
  if (!composerDraftIsInvested(input.doc, input.attachments)) {
    next.delete(key);
    return next;
  }
  const existing = drafts.get(key);
  next.set(key, {
    ...input,
    id: existing?.id ?? options.createId?.() ?? globalThis.crypto.randomUUID(),
    scope: { ...input.scope },
    doc: input.doc.slice(0, MAX_BLOCKS).map(cloneBlock),
    attachments: input.attachments
      .slice(0, MAX_ATTACHMENTS)
      .map((item) => ({ ...item })),
    posture: { ...input.posture },
    updatedAt: options.now ?? Date.now(),
  });
  return next;
}

export function promoteComposerDraft(
  drafts: ReadonlyMap<string, ComposerDraft>,
  from: ComposerDraftScope,
  to: ComposerDraftScope,
  now = Date.now()
): ComposerDraftPromotion {
  const fromKey = composerDraftScopeKey(from);
  const toKey = composerDraftScopeKey(to);
  const source = drafts.get(fromKey);
  if (!source) return { outcome: "missing", drafts: new Map(drafts) };
  const destination = drafts.get(toKey);
  if (destination && destination.id !== source.id) {
    return { outcome: "conflict", drafts: new Map(drafts) };
  }
  const next = new Map(drafts);
  next.delete(fromKey);
  next.set(toKey, cloneDraft({ ...source, scope: { ...to }, updatedAt: now }));
  return { outcome: "moved", drafts: next };
}

export function loadComposerDrafts(
  storage?: StorageLike | null
): ComposerDraftCollection {
  const resolved = storage === undefined ? defaultStorage() : storage;
  if (!resolved) return { drafts: new Map(), warning: "unavailable" };
  let raw: string | null;
  try {
    raw = resolved.getItem(COMPOSER_DRAFT_STORAGE_KEY);
  } catch {
    return { drafts: new Map(), warning: "unavailable" };
  }
  if (raw === null) return { drafts: new Map(), warning: null };
  if (raw.length > MAX_RAW_BYTES || serializedBytes(raw) > MAX_RAW_BYTES) {
    return { drafts: new Map(), warning: "corrupt" };
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    const snapshot = asJsonObject(parsed);
    if (snapshot == null) return { drafts: new Map(), warning: "corrupt" };
    if (
      snapshot.version !== COMPOSER_DRAFT_VERSION ||
      !Array.isArray(snapshot.drafts) ||
      snapshot.drafts.length > MAX_DRAFTS
    ) {
      return { drafts: new Map(), warning: "corrupt" };
    }
    const records = snapshot.drafts.map(parseDraft);
    if (records.some((draft) => draft === null))
      return { drafts: new Map(), warning: "corrupt" };
    const drafts = new Map<string, ComposerDraft>();
    const ids = new Set<string>();
    for (const draft of records) {
      if (draft == null) continue;
      const key = composerDraftScopeKey(draft.scope);
      if (
        drafts.has(key) ||
        ids.has(draft.id) ||
        !composerDraftIsInvested(draft.doc, draft.attachments)
      ) {
        return { drafts: new Map(), warning: "corrupt" };
      }
      drafts.set(key, cloneDraft(draft));
      ids.add(draft.id);
    }
    return { drafts, warning: null };
  } catch {
    return { drafts: new Map(), warning: "corrupt" };
  }
}

export function saveComposerDrafts(
  drafts: ReadonlyMap<string, ComposerDraft>,
  storage?: StorageLike | null
): boolean {
  const resolved = storage === undefined ? defaultStorage() : storage;
  if (!resolved) return false;
  try {
    const records = [...drafts.values()]
      .filter((draft) => composerDraftIsInvested(draft.doc, draft.attachments))
      .toSorted((left, right) => right.updatedAt - left.updatedAt)
      .slice(0, MAX_DRAFTS)
      .map(cloneDraft);
    if (records.length === 0 && resolved.removeItem) {
      resolved.removeItem(COMPOSER_DRAFT_STORAGE_KEY);
      return true;
    }
    const snapshot: ComposerDraftSnapshot = {
      version: COMPOSER_DRAFT_VERSION,
      drafts: records,
    };
    const raw = JSON.stringify(snapshot);
    if (serializedBytes(raw) > MAX_RAW_BYTES) return false;
    resolved.setItem(COMPOSER_DRAFT_STORAGE_KEY, raw);
    return true;
  } catch {
    return false;
  }
}
