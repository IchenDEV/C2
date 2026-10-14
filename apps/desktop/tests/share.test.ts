// @ts-nocheck
import { describe, expect, test } from "bun:test";

import {
  DEFAULT_VISIBILITY,
  authorForSeq,
  authorLabel,
  followerSummary,
  actorNameResolver,
  matchesRequest,
  ownClientRequestId,
  scopedRequestId,
  shareEventEffects,
  visibilityLabel,
} from "../src/session/share";

const record = {
  creator: "device:a",
  visibility: "public",
  followers: ["device:a", "device:b"],
  journal: [{ actor: "device:b", nickname: "Bo", kind: "prompt" }],
  message_authors: { "3": { actor: "device:a", nickname: "Ann" } },
  revision: 4,
};

describe("share ui logic", () => {
  test("new tasks default public", () => {
    expect(DEFAULT_VISIBILITY).toBe("public");
  });
  test("request correlation is exact per actor namespace", () => {
    expect(scopedRequestId("device:a", "r1")).toBe("device:a/r1");
    expect(scopedRequestId("", "r1")).toBe("r1");
    expect(matchesRequest("device:a", "r1", "device:a/r1")).toBe(true);
    expect(matchesRequest("device:a", "r1", "device:b/r1")).toBe(false);
    expect(matchesRequest("device:a", "r1", "xdevice:a/r1")).toBe(false);
  });
  test("revoked drops content; author refreshes transcript", () => {
    expect(shareEventEffects({ kind: "revoked" })).toEqual(["drop"]);
    expect(shareEventEffects({ kind: "message_author" })).toContain(
      "refresh-transcript"
    );
    expect(shareEventEffects({ kind: "follow" })).toEqual(["refresh-share"]);
  });
  test("author comes from canonical association only", () => {
    expect(authorForSeq(3, null, record).nickname).toBe("Ann");
    expect(authorForSeq(9, null, record)).toBeNull();
    expect(authorLabel({ nickname: "  " })).toBeNull();
  });
  test("followers never expose ids", () => {
    const summary = followerSummary(
      record,
      { id: "device:a", nickname: "Ann" },
      actorNameResolver(record)
    );
    expect(summary).toEqual({ following: true, names: ["You", "Bo"] });
  });
  test("legacy is local only", () => {
    expect(visibilityLabel(null, true)).toBe("Local only");
    expect(visibilityLabel(record, false)).toBe("Public");
  });
});

import { turnsFromTranscript } from "../src/session/turns";

describe("canonical author in transcript turns", () => {
  test("user row author becomes turn author by seq only", () => {
    const turns = turnsFromTranscript([
      {
        seq: 1,
        role: "user",
        part: { kind: "text", text: "hi" },
        author: { actor: "device:a", nickname: "Ann" },
      },
      { seq: 2, role: "agent", part: { kind: "text", text: "yo" } },
      { seq: 3, role: "user", part: { kind: "text", text: "legacy" } },
    ]);
    expect(turns[0].authorName).toBe("Ann");
    expect(turns[1].authorName).toBeUndefined();
  });
});

describe("own request correlation", () => {
  const device = { id: "device:a", nickname: "A" };
  const host = { id: "host", nickname: "Host" };
  test("paired device resolves only its own scoped id", () => {
    expect(ownClientRequestId(device, "device:a/c1")).toBe("c1");
    expect(ownClientRequestId(device, "device:b/c1")).toBeNull();
    expect(ownClientRequestId(device, "c1")).toBeNull();
  });
  test("host-local ignores device-scoped ids", () => {
    expect(ownClientRequestId(host, "c1")).toBe("c1");
    expect(ownClientRequestId(host, "device:a/c1")).toBeNull();
  });
  test("unknown identity passes through; null stays null", () => {
    expect(ownClientRequestId(null, "x")).toBe("x");
    expect(ownClientRequestId(device, undefined)).toBeNull();
  });
  test("denied revoked label is not Local only", () => {
    expect(visibilityLabel(null, true, true)).toBe("No access");
    expect(visibilityLabel(null, true)).toBe("Local only");
  });
});

describe("scopeEventToSelf", () => {
  const { scopeEventToSelf } = require("../src/session/share");
  const device = { id: "device:a", nickname: "A" };
  test("rewrites own ids and nulls foreign ones without mutating", () => {
    const own = { event: "x", request_id: "device:a/c1" };
    expect(scopeEventToSelf(own, device).request_id).toBe("c1");
    expect(own.request_id).toBe("device:a/c1");
    expect(
      scopeEventToSelf({ event: "x", request_id: "device:b/c1" }, device)
        .request_id
    ).toBeNull();
    const plain = { event: "x" };
    expect(scopeEventToSelf(plain, device)).toBe(plain);
  });
  test("prompts_cancelled.request_ids keeps only own ids as client ids", () => {
    const event = {
      event: "prompts_cancelled",
      session: "s",
      request_ids: ["device:a/c1", "device:b/c1", "device:a/c2"],
    };
    expect(scopeEventToSelf(event, device).request_ids).toEqual(["c1", "c2"]);
    expect(event.request_ids).toHaveLength(3);
    const local = { event: "prompts_cancelled", session: "s", request_ids: ["c1"] };
    expect(scopeEventToSelf(local, null)).toBe(local);
  });
});
