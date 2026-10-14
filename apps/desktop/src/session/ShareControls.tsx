import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";

import { remotePairingLink } from "../bridge";
import { listenCore } from "../coreTransport";
import {
  actorNameResolver,
  followerSummary,
  getShare,
  isShareChangedEvent,
  setNickname,
  setShareFollow,
  setShareVisibility,
  shareEventEffects,
  visibilityLabel,
  whoami,
} from "./share";
import type { ShareParticipant, ShareRecord, ShareVisibility } from "./share";

/**
 * Ordinary-UI sharing controls for one session: visibility, nonexclusive follow, participant
 * names, nickname edit and the one-time pairing link. No locks: public controls elsewhere stay
 * available to everyone regardless of follow state.
 */
export function ShareControls({
  session,
  onRevoked,
}: {
  session: string;
  /** Called when the server revokes this session; the host must discard cached content. */
  onRevoked?: (session: string) => void;
}) {
  const [me, setMe] = useState<ShareParticipant | null>(null);
  const [record, setRecord] = useState<ShareRecord | null>(null);
  const [legacy, setLegacy] = useState(false);
  // Set once the server revokes this session: an unreadable record is then "no access", never
  // mislabelled as a legacy local session.
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nickname, setNicknameDraft] = useState("");
  const [linkState, setLinkState] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setRecord(await getShare(session));
      setLegacy(false);
      setDenied(false);
      setError(null);
    } catch {
      // Legacy (no creator) and denied sessions both read as not shared; never auto-publish.
      setRecord(null);
      setLegacy(true);
    }
  }, [session]);

  useEffect(() => {
    let live = true;
    whoami()
      .then((value) => {
        if (!live) return;
        setMe(value);
        setNicknameDraft(value.nickname);
      })
      .catch((e: unknown) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    void refresh();
    return listenCore<unknown>("engine-event", (event) => {
      if (!isShareChangedEvent(event) || event.session !== session) return;
      for (const effect of shareEventEffects(event)) {
        if (effect === "drop") {
          setRecord(null);
          setDenied(true);
          onRevoked?.(session);
          void refresh();
        } else void refresh();
      }
    });
  }, [session, refresh, onRevoked]);

  const run = async (action: () => Promise<ShareRecord | void>) => {
    try {
      const next = await action();
      if (next) setRecord(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const names = actorNameResolver(record);
  const followers = record ? followerSummary(record, me, names) : null;
  const isCreator = record != null && me != null && record.creator === me.id;
  const next: ShareVisibility =
    record?.visibility === "public" ? "private" : "public";

  const copyLink = async () => {
    try {
      const link = await remotePairingLink();
      if (!link) {
        setLinkState("Sharing links need the desktop app");
        return;
      }
      await navigator.clipboard.writeText(link.url);
      setLinkState("One-time link copied");
    } catch (e) {
      setLinkState(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div
      className="share-controls gap-inline flex max-w-full min-w-0 shrink-0 flex-wrap items-center text-xs"
      role="group"
      aria-label="Sharing"
    >
      <span
        className="share-visibility text-muted-foreground"
        data-visibility={
          record?.visibility ?? (denied ? "denied" : "local")
        }
      >
        {visibilityLabel(record, legacy, denied)}
      </span>
      {isCreator && (
        <Button
          type="button"
          variant="ghost"
          size="compact"
          onClick={() => void run(() => setShareVisibility(session, next))}
        >
          Make {next}
        </Button>
      )}
      {record && followers && (
        <Button
          type="button"
          variant="ghost"
          size="compact"
          aria-pressed={followers.following}
          onClick={() =>
            void run(() => setShareFollow(session, !followers.following))
          }
        >
          {followers.following ? "Following" : "Follow"}
          {followers.names.length > 0 && ` · ${followers.names.join(", ")}`}
        </Button>
      )}
      <input
        className="share-nickname bg-transparent w-24 max-w-full min-w-0 rounded border px-1"
        aria-label="Your nickname"
        value={nickname}
        onChange={(e) => setNicknameDraft(e.target.value)}
        onBlur={() => {
          if (me && nickname.trim() && nickname !== me.nickname)
            void setNickname(nickname)
              .then(setMe)
              .catch((e: unknown) => setError(String(e)));
        }}
      />
      <Button
        type="button"
        variant="ghost"
        size="compact"
        onClick={() => void copyLink()}
      >
        Share link
      </Button>
      {linkState && <span className="text-muted-foreground">{linkState}</span>}
      {error && (
        <span role="alert" className="text-destructive">
          {error}
        </span>
      )}
    </div>
  );
}
