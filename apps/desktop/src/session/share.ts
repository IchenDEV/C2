import { call } from "../bridge";

/** Mirrors core `share::Visibility`. New tasks default to public; private is creator-only. */
export type ShareVisibility = "public" | "private";
export const DEFAULT_VISIBILITY: ShareVisibility = "public";

/** Trusted backend identity (`share.whoami`). `id` is a backend credential id: never render it. */
export interface ShareParticipant {
  id: string;
  nickname: string;
}

/** Mirrors core `share::ShareRecord` as returned by `share.get`. */
export interface ShareRecord {
  creator: string;
  visibility: ShareVisibility;
  followers: string[];
  journal: ShareOp[];
  message_authors: Record<string, MessageAuthor>;
  revision: number;
}

export interface ShareOp {
  actor: string;
  nickname: string;
  kind: string;
  request_id?: string | null;
}

export interface MessageAuthor {
  actor: string;
  nickname: string;
  request_id?: string | null;
}

export type ShareChangeKind =
  | "message_author"
  | "follow"
  | "visibility"
  | "revoked";

export interface ShareChangedEvent {
  event: "session_share_changed";
  session: string;
  kind: string;
  revision: number;
}

export async function whoami(): Promise<ShareParticipant> {
  return await call<ShareParticipant>("share.whoami", null);
}

export async function setNickname(nickname: string): Promise<ShareParticipant> {
  return await call<ShareParticipant>("share.set_nickname", { nickname });
}

export async function getShare(session: string): Promise<ShareRecord> {
  return await call<ShareRecord>("share.get", { session });
}

export async function setShareVisibility(
  session: string,
  visibility: ShareVisibility
): Promise<ShareRecord> {
  return await call<ShareRecord>("share.set_visibility", {
    session,
    visibility,
  });
}

export async function setShareFollow(
  session: string,
  follow: boolean
): Promise<ShareRecord> {
  return await call<ShareRecord>("share.follow", { session, follow });
}

/**
 * The backend scopes client request ids per actor (`device:<id>/<clientId>`; host-local is
 * unscoped). Centralised here so host scoping can be corrected in one place: correlation must
 * match the exact trusted actor namespace, never a suffix across actors.
 */
export function scopedRequestId(
  actor: string | null | undefined,
  clientId: string
): string {
  return actor === null || actor === undefined || actor === ""
    ? clientId
    : `${actor}/${clientId}`;
}

export function matchesRequest(
  actor: string | null | undefined,
  clientId: string,
  eventRequestId: string | null | undefined
): boolean {
  return (
    eventRequestId != null &&
    eventRequestId === scopedRequestId(actor, clientId)
  );
}

/**
 * Maps an event's backend request id back to this client's own id, or null when it belongs to
 * another actor. Paired devices see `device:<id>/<clientId>`; the host-local UI sees unscoped
 * ids and must not claim any `device:` scoped id. Unknown identity (`me` not loaded) passes
 * through unchanged. Two participants reusing a clientId therefore never resolve each other.
 */
export function ownClientRequestId(
  me: ShareParticipant | null,
  eventRequestId: string | null | undefined
): string | null {
  if (eventRequestId == null) return null;
  if (me == null) return eventRequestId;
  if (me.id.startsWith("device:")) {
    const prefix = `${me.id}/`;
    return eventRequestId.startsWith(prefix)
      ? eventRequestId.slice(prefix.length)
      : null;
  }
  return eventRequestId.startsWith("device:") ? null : eventRequestId;
}

/** Applies `ownClientRequestId` to every request-id field a core event carries. */
export function scopeEventToSelf<T extends object>(
  event: T,
  me: ShareParticipant | null
): T {
  const record: Record<string, unknown> = Object.fromEntries(
    Object.entries(event)
  );
  const fields = ["request_id", "prompt_request_id"] as const;
  let next: Record<string, unknown> | null = null;
  for (const field of fields) {
    const value = record[field];
    if (typeof value !== "string") continue;
    const own = ownClientRequestId(me, value);
    if (own === value) continue;
    next ??= { ...record };
    next[field] = own;
  }
  // `prompts_cancelled.request_ids` carries the store's actor-scoped ids; keep only this actor's
  // own, mapped back to the client ids its queued rows hold (foreign ids must never match).
  const ids = record.request_ids;
  if (Array.isArray(ids)) {
    const own = ids
      .filter((id): id is string => typeof id === "string")
      .map((id) => ownClientRequestId(me, id))
      .filter((id): id is string => id != null);
    if (own.length !== ids.length || own.some((id, i) => id !== ids[i])) {
      next ??= { ...record };
      next.request_ids = own;
    }
  }
  if (next === null) return event;
  const scoped: T = { ...event, ...next };
  return scoped;
}

export function isShareChangedEvent(
  value: unknown
): value is ShareChangedEvent {
  return (
    value != null &&
    typeof value === "object" &&
    (value as { event?: unknown }).event === "session_share_changed"
  );
}

/** What the UI must do for one share event. `drop` = discard cached/selected session content. */
export type ShareEventEffect = "refresh-share" | "refresh-transcript" | "drop";

export function shareEventEffects(
  event: ShareChangedEvent
): ShareEventEffect[] {
  switch (event.kind) {
    case "revoked": {
      return ["drop"];
    }
    case "message_author": {
      return ["refresh-share", "refresh-transcript"];
    }
    default: {
      return ["refresh-share"];
    }
  }
}

/** Canonical per-message author from backend (`sessions.transcript` entry author, else map). */
export function authorForSeq(
  seq: number,
  entryAuthor: MessageAuthor | null | undefined,
  record: ShareRecord | null
): MessageAuthor | null {
  return entryAuthor ?? record?.message_authors[String(seq)] ?? null;
}

export function authorLabel(author: MessageAuthor | null): string | null {
  const name = author?.nickname.trim();
  return name === undefined || name === "" ? null : name;
}

/** Display names of followers; the viewer is shown as "You". Backend ids never surface. */
export function followerSummary(
  record: ShareRecord,
  me: ShareParticipant | null,
  nameOf: (actorId: string) => string | null
): { following: boolean; names: string[] } {
  const following = me != null && record.followers.includes(me.id);
  const names = record.followers
    .filter((id) => id !== me?.id)
    .map((id) => nameOf(id) ?? "Participant");
  return { following, names: following ? ["You", ...names] : names };
}

/** Resolve a display name for an actor from journal/author snapshots (never the raw id). */
export function actorNameResolver(
  record: ShareRecord | null
): (actorId: string) => string | null {
  const names = new Map<string, string>();
  for (const op of record?.journal ?? []) {
    if (op.nickname.trim()) names.set(op.actor, op.nickname);
  }
  for (const author of Object.values(record?.message_authors ?? {})) {
    if (author.nickname.trim()) names.set(author.actor, author.nickname);
  }
  return (id) => names.get(id) ?? null;
}

export function visibilityLabel(
  record: ShareRecord | null,
  legacy: boolean,
  denied = false
): string {
  if (denied && record == null) return "No access";
  if (legacy || record == null) return "Local only";
  return record.visibility === "public" ? "Public" : "Private";
}
