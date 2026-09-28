import { describe, expect, test } from "bun:test";

import type { SessionActivity } from "../src/bridge";
import { mergeCommandResults } from "../src/palette/merge";
import {
  executionPolicyChangeDisabled,
  sessionExecutionPolicy,
  sessionMode,
  withSessionExecutionPolicy,
} from "../src/session/mode";
import {
  activeSessionWorktreeState,
  activityIsBusy,
  enqueuePermission,
  isTerminalSessionEvent,
  latestActivity,
  matchesSessionCreation,
  paneBoundToSession,
  permissionQueueAfterAnswer,
  permissionQueueAfterActivity,
  pendingInputsForSession,
  permissionsFromSessions,
  sessionCreationBaseline,
  sessionCreationBaselineSha,
  sessionCreationReceipt,
  sessionCreationSource,
  sessionProjectPath,
  sessionShellWithReceipt,
  shouldRenderSessionEvent,
  worktreeGatingReason,
} from "../src/session/sessionEvents";
import {
  applyEvent,
  isRunning,
  matchesSubmittedEditorRevision,
  newTurn,
  sameDocBlocks,
  transcriptTailState,
  withRunningSession,
  withoutUnacceptedTurn,
} from "../src/session/turns";

describe("session event isolation", () => {
  test("does not render a background session into the active transcript", () => {
    expect(
      shouldRenderSessionEvent(
        {
          event: "agent_text",
          session: "session-a",
          message_id: "1",
          text: "late A",
        },
        "session-b"
      )
    ).toBe(false);
    expect(
      shouldRenderSessionEvent(
        {
          event: "agent_text",
          session: "session-b",
          message_id: "2",
          text: "current B",
        },
        "session-b"
      )
    ).toBe(true);
  });

  test("accumulates a background pane's transcript when its session is tiled", () => {
    const late = {
      event: "agent_text",
      session: "session-a",
      message_id: "1",
      text: "late A",
    } as const;
    // With no pane set, a background session's turn is still dropped from the focused transcript.
    expect(shouldRenderSessionEvent(late, "session-b")).toBe(false);
    // Once session-a is bound to a (background) pane, its turns render into that pane.
    expect(
      shouldRenderSessionEvent(
        late,
        "session-b",
        null,
        new Set(["session-a", "session-b"])
      )
    ).toBe(true);
    // A session bound to no pane at all stays filtered out.
    expect(
      shouldRenderSessionEvent(late, "session-b", null, new Set(["session-b"]))
    ).toBe(false);
  });

  test("finds an existing pane binding so session selection cannot duplicate it", () => {
    const panes = {
      "pane-a": { sessionId: "session-a" },
      "pane-b": { sessionId: "session-b" },
      "pane-c": { sessionId: null },
    };
    expect(paneBoundToSession(panes, "session-b")).toBe("pane-b");
    expect(paneBoundToSession(panes, "session-c")).toBeNull();
  });

  test("keeps global errors and permissions actionable", () => {
    expect(
      shouldRenderSessionEvent(
        {
          event: "error",
          session: null,
          message: "startup failed",
          terminal: true,
        },
        null
      )
    ).toBe(true);
    expect(
      shouldRenderSessionEvent(
        {
          event: "permission_request",
          session: "session-a",
          request_id: "permission-1",
          title: "Write a file",
          options: [],
        },
        "session-b"
      )
    ).toBe(true);
  });

  test("renders only the creation error owned by this client", () => {
    const error = {
      event: "error",
      session: null,
      message: "session creation failed",
      terminal: true,
      request_id: "remote-1",
    } as const;
    expect(shouldRenderSessionEvent(error, "session-a", "desktop-1")).toBe(
      false
    );
    expect(shouldRenderSessionEvent(error, "session-a", "remote-1")).toBe(true);
  });

  test("only terminal errors end a running turn", () => {
    const warning = {
      event: "error",
      session: "session-a",
      message: "fallback",
      terminal: false,
    } as const;
    const afterWarning = applyEvent([newTurn("prompt")], warning);
    expect(isTerminalSessionEvent(warning)).toBe(false);
    expect(isRunning(afterWarning[0])).toBe(true);

    const failure = {
      ...warning,
      message: "provider stopped",
      terminal: true,
    } as const;
    const afterFailure = applyEvent(afterWarning, failure);
    expect(isTerminalSessionEvent(failure)).toBe(true);
    expect(isRunning(afterFailure[0])).toBe(false);
  });

  test("claims only the session created for this window's request", () => {
    expect(
      matchesSessionCreation(
        { event: "session_created", session: "ours", request_id: "desktop-1" },
        "desktop-1"
      )
    ).toBe(true);
    expect(
      matchesSessionCreation(
        { event: "session_created", session: "remote", request_id: "remote-1" },
        "desktop-1"
      )
    ).toBe(false);
    expect(
      matchesSessionCreation(
        { event: "session_created", session: "late", request_id: "desktop-1" },
        null
      )
    ).toBe(false);
  });

  test("queues concurrent permission requests without overwriting either session", () => {
    const first = {
      session: "session-a",
      requestId: "request-1",
      title: "A",
      options: [],
    };
    const second = {
      session: "session-b",
      requestId: "request-2",
      title: "B",
      options: [],
    };
    const queue = enqueuePermission(enqueuePermission([], first), second);

    expect(queue.map((request) => request.session)).toEqual([
      "session-a",
      "session-b",
    ]);
    expect(
      enqueuePermission(queue, { ...first, title: "A updated" })[0].title
    ).toBe("A updated");
  });

  test("projects pending inputs onto the active chat", () => {
    const queue = [
      {
        session: "session-a",
        requestId: "request-a1",
        title: "A1",
        options: [],
      },
      {
        session: "session-b",
        requestId: "request-b1",
        title: "B1",
        options: [],
      },
      {
        session: "session-a",
        requestId: "request-a2",
        title: "A2",
        options: [],
      },
    ];

    expect(
      pendingInputsForSession(queue, "session-a").map((item) => item.requestId)
    ).toEqual(["request-a1", "request-a2"]);
    expect(
      pendingInputsForSession(queue, "session-b").map((item) => item.requestId)
    ).toEqual(["request-b1"]);
    expect(pendingInputsForSession(queue, null)).toEqual([]);
  });

  test("projects revisioned core activity and restores globally ordered pending input", () => {
    const running: SessionActivity = {
      revision: 4,
      state: { kind: "running", turn_id: "turn-a", prompt_request_id: null },
    };
    const staleIdle: SessionActivity = { revision: 3, state: { kind: "idle" } };
    expect(latestActivity(running, staleIdle)).toBe(running);
    expect(activityIsBusy(running)).toBe(true);

    const awaiting: SessionActivity = {
      revision: 5,
      state: {
        kind: "awaiting_input",
        turn_id: "turn-a",
        prompt_request_id: null,
        pending: [
          {
            input_id: "permission-later",
            kind: "permission",
            title: "Later",
            options: [],
            sequence: 8,
          },
          {
            input_id: "permission-first",
            kind: "permission",
            title: "First",
            options: [["allow", "Allow"]],
            sequence: 2,
          },
        ],
      },
    };
    const queue = permissionsFromSessions([
      { id: "session-a", activity: awaiting },
    ]);
    expect(activityIsBusy(awaiting)).toBe(true);
    expect(queue.map((item) => item.requestId)).toEqual([
      "permission-first",
      "permission-later",
    ]);
  });

  test("an activity update keeps an elicitation renderable as a question", () => {
    const form = {
      message: "Allow Browser use to access http://127.0.0.1:4173?",
      tool_call_id: "browser-call",
      fields: [
        {
          key: "persist",
          kind: "text" as const,
          title: "Approval scope",
          required: true,
          options: [],
        },
      ],
    };
    const activity: SessionActivity = {
      revision: 10,
      state: {
        kind: "awaiting_input",
        turn_id: "turn-a",
        pending: [
          {
            input_id: "question-1",
            kind: "elicitation",
            title: form.message,
            options: [],
            sequence: 10,
            context: { kind: "acp" },
            form,
          },
        ],
      },
    };

    const queue = permissionQueueAfterActivity(
      [
        {
          session: "session-b",
          requestId: "permission-b",
          title: "Other session",
          options: [],
          sequence: 20,
        },
      ],
      "session-a",
      activity
    );

    expect(queue[0]).toMatchObject({
      session: "session-a",
      requestId: "question-1",
      context: { kind: "acp" },
      form,
    });
    expect(queue[1]?.session).toBe("session-b");
  });

  test("keeps a pending input when the host rejects its answer", () => {
    const pending = {
      session: "session-a",
      requestId: "question-1",
      title: "Question",
      options: [],
    };
    const other = {
      session: "session-b",
      requestId: "permission-2",
      title: "Permission",
      options: [],
    };

    expect(
      permissionQueueAfterAnswer(
        [pending, other],
        "session-a",
        "question-1",
        false
      )
    ).toEqual([pending, other]);
    expect(
      permissionQueueAfterAnswer(
        [pending, other],
        "session-a",
        "question-1",
        true
      )
    ).toEqual([other]);
  });

  test("tracks a remotely started turn without creating an idle warning turn", () => {
    const started = applyEvent([], {
      event: "turn_started",
      session: "session-a",
    });
    expect(started).toHaveLength(1);
    expect(isRunning(started[0])).toBe(true);

    const idleWarning = applyEvent([], {
      event: "error",
      session: "session-a",
      message: "hook failed",
      terminal: false,
    });
    expect(idleWarning).toHaveLength(1);
    expect(isRunning(idleWarning[0])).toBe(false);
    expect(idleWarning[0].error).toBe("hook failed");
  });

  test("keeps a competing accepted turn separate from this client's rejected prompt", () => {
    let turns = [newTurn("losing prompt", "losing-request")];
    turns = applyEvent(turns, {
      event: "turn_started",
      session: "session-a",
      request_id: "winning-request",
    });
    turns = applyEvent(turns, {
      event: "error",
      session: "session-a",
      request_id: "losing-request",
      message: "a turn is already running for this session",
      terminal: false,
    });
    turns = applyEvent(turns, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-1",
      text: "winner answer",
    });

    expect(turns).toHaveLength(2);
    expect(turns[0].prompt).toBe("losing prompt");
    expect(turns[0].text).toBe("");
    expect(turns[0].error).toContain("already running");
    expect(isRunning(turns[0])).toBe(false);
    expect(turns[1].requestId).toBe("winning-request");
    expect(turns[1].text).toBe("winner answer");
    expect(isRunning(turns[1])).toBe(true);
  });

  test("routes an identified running turn away from an unrelated optimistic prompt", () => {
    const turns = applyEvent(
      [newTurn("losing prompt", "losing-request")],
      {
        event: "agent_text",
        session: "session-a",
        message_id: "chunk-1",
        text: "winner answer",
      },
      "winning-request"
    );

    expect(turns).toHaveLength(2);
    expect(turns[0].text).toBe("");
    expect(turns[1].requestId).toBe("winning-request");
    expect(turns[1].text).toBe("winner answer");
  });

  test("keeps queued prompts pending until their own native turn starts", () => {
    const active = { ...newTurn("active", "active-request"), accepted: true };
    let turns = [
      active,
      { ...newTurn("next", "queued-request"), delivery: "queued" as const },
    ];
    turns = applyEvent(turns, {
      event: "prompt_queued",
      session: "session-a",
      request_id: "queued-request",
      position: 2,
    });
    expect(turns[1]).toMatchObject({
      delivery: "queued",
      queuePosition: 2,
      accepted: false,
    });

    turns = applyEvent(turns, {
      event: "turn_started",
      session: "session-a",
      request_id: "queued-request",
    });
    expect(turns[1]).toMatchObject({
      accepted: true,
      streamBoundaryKnown: true,
    });
    expect(turns[1].delivery).toBeUndefined();
    expect(turns[1].queuePosition).toBeUndefined();
  });

  test("moves the stream boundary to a provider-accepted steering prompt", () => {
    const active = { ...newTurn("active", "active-request"), accepted: true };
    const steer = {
      ...newTurn("change course", "steer-request"),
      delivery: "steer" as const,
    };
    let turns = applyEvent([active, steer], {
      event: "steer_accepted",
      session: "session-a",
      request_id: "steer-request",
      transcript_seq: 42,
      outcome: "injected",
    });
    turns = applyEvent(
      turns,
      {
        event: "agent_text",
        session: "session-a",
        message_id: "chunk-1",
        text: "steered answer",
      },
      "steer-request"
    );

    expect(isRunning(turns[0])).toBe(false);
    expect(turns[1]).toMatchObject({
      accepted: true,
      delivery: "steer",
      transcriptStartSeq: 42,
      text: "steered answer",
    });
  });

  test("compares the submitted editor revision structurally", () => {
    const submitted = [
      { type: "text", text: "ship it" },
      {
        type: "skill",
        skill_id: "reviewer",
        params: { depth: "full", tone: "direct" },
      },
    ] as const;
    expect(
      sameDocBlocks(submitted, [
        { type: "text", text: "ship it" },
        {
          type: "skill",
          skill_id: "reviewer",
          params: { tone: "direct", depth: "full" },
        },
      ])
    ).toBe(true);
    expect(
      sameDocBlocks(submitted, [
        { type: "text", text: "ship it, then preserve this new draft" },
        {
          type: "skill",
          skill_id: "reviewer",
          params: { depth: "full", tone: "direct" },
        },
      ])
    ).toBe(false);
    expect(matchesSubmittedEditorRevision(submitted, 7, submitted, 7)).toBe(
      true
    );
    expect(matchesSubmittedEditorRevision(submitted, 8, submitted, 7)).toBe(
      false
    );
    expect(
      sameDocBlocks(
        [{ type: "session", session_id: "source", through_seq: 11 }],
        [{ type: "session", session_id: "source", through_seq: 12 }]
      )
    ).toBe(false);
  });

  test("updates the running snapshot immutably", () => {
    const initial = new Set(["session-a"]);
    const withSecond = withRunningSession(initial, "session-b", true);
    const withoutFirst = withRunningSession(withSecond, "session-a", false);

    expect([...initial]).toEqual(["session-a"]);
    expect([...withSecond]).toEqual(["session-a", "session-b"]);
    expect([...withoutFirst]).toEqual(["session-b"]);
  });

  test("removes only the unaccepted turn for a cancelled creation", () => {
    const cancelled = newTurn("cancelled draft", "cancelled-request");
    const accepted = newTurn("accepted elsewhere", "accepted-request");
    accepted.accepted = true;
    const unrelated = newTurn("unrelated draft", "unrelated-request");

    expect(
      withoutUnacceptedTurn(
        [cancelled, accepted, unrelated],
        "cancelled-request"
      )
    ).toEqual([accepted, unrelated]);
    expect(withoutUnacceptedTurn([accepted], "accepted-request")).toEqual([
      accepted,
    ]);
  });

  test("does not reopen a persisted tail for optimistic pre-acceptance state", () => {
    expect(transcriptTailState(true, false, "previous-request")).toEqual({
      running: false,
      requestId: undefined,
    });
    expect(transcriptTailState(true, true, "accepted-request")).toEqual({
      running: true,
      requestId: "accepted-request",
    });
    expect(transcriptTailState(false, false, "completed-request")).toEqual({
      running: false,
      requestId: "completed-request",
    });
  });

  test("keeps a worktree session grouped under its source project", () => {
    expect(
      sessionProjectPath({
        cwd: "/tmp/repo-worktree/packages/app",
        worktree_path: "/tmp/repo-worktree",
        project_path: "/repo/packages/app",
      })
    ).toBe("/repo/packages/app");
    expect(
      sessionProjectPath({
        cwd: "/repo",
        worktree_path: null,
        project_path: null,
      })
    ).toBe("/repo");
    expect(
      sessionProjectPath({
        cwd: "/legacy-worktree",
        worktree_path: "/legacy-worktree",
        project_path: null,
      })
    ).toBeNull();
  });

  test("uses the recorded project as the source for a worktree session", () => {
    expect(
      sessionCreationSource(null, "/tmp/repo-worktree", {
        cwd: "/tmp/repo-worktree",
        worktree_path: "/tmp/repo-worktree",
        project_path: "/repo",
      })
    ).toBe("/repo");
  });

  test("requires an explicit project for a legacy worktree without provenance", () => {
    expect(
      sessionCreationSource(null, "/legacy-worktree", {
        cwd: "/legacy-worktree",
        worktree_path: "/legacy-worktree",
        project_path: null,
      })
    ).toBeNull();
  });

  test("prefers an active project and otherwise uses a normal session cwd", () => {
    const normalSession = {
      cwd: "/repo-from-session",
      worktree_path: null,
      project_path: null,
    };
    expect(
      sessionCreationSource("/selected-project", "/stale-cwd", normalSession)
    ).toBe("/selected-project");
    expect(sessionCreationSource(null, "/stale-cwd", normalSession)).toBe(
      "/repo-from-session"
    );
    expect(sessionCreationSource(null, "/plain-cwd", null)).toBe("/plain-cwd");
  });

  test("fails closed when an active id has neither a list shell nor a receipt", () => {
    expect(
      sessionCreationSource("/selected-project", "/isolated/cwd", null, true)
    ).toBeNull();
  });

  test("uses a correlated worktree receipt instead of its isolated cwd", () => {
    const receipt = sessionCreationReceipt({
      event: "session_created",
      session: "created",
      cwd: "/tmp/session-worktree/packages/app",
      project_path: "/repo/packages/app",
      worktree_path: "/tmp/session-worktree",
      worktree_baseline: {
        kind: "current",
        ref: "HEAD",
        sha: "0123456789abcdef",
        display: "HEAD",
      },
      request_id: "ours",
    });
    expect(receipt).not.toBeNull();
    expect(sessionCreationSource(null, receipt!.cwd, receipt, true)).toBe(
      "/repo/packages/app"
    );
    expect(sessionCreationBaseline(receipt)).toBeUndefined();
    expect(
      sessionShellWithReceipt("created", undefined, {
        session: "created",
        shell: receipt!,
      })?.worktree_baseline?.kind
    ).toBe("current");
    expect(
      sessionShellWithReceipt("other", undefined, {
        session: "created",
        shell: receipt!,
      })
    ).toBeUndefined();
  });

  test("renders worktree state from a matched receipt while the session rail is unavailable", () => {
    const knownReceipt = {
      cwd: "/tmp/session-worktree/packages/app",
      project_path: "/repo/packages/app",
      worktree_path: "/tmp/session-worktree",
      worktree_baseline: {
        kind: "current" as const,
        ref: "HEAD",
        sha: "0123456789abcdef",
        display: "HEAD",
      },
    };
    expect(
      activeSessionWorktreeState("created", undefined, {
        session: "created",
        shell: knownReceipt,
      })
    ).toEqual({
      baseline: knownReceipt.worktree_baseline,
      legacyUnknown: false,
    });

    expect(
      activeSessionWorktreeState("created", undefined, {
        session: "created",
        shell: { ...knownReceipt, worktree_baseline: null },
      })
    ).toEqual({ baseline: null, legacyUnknown: true });

    expect(
      activeSessionWorktreeState("other", undefined, {
        session: "created",
        shell: knownReceipt,
      })
    ).toEqual({ baseline: null, legacyUnknown: false });
  });

  test("does not trust cwd-only events from legacy producers as provenance", () => {
    expect(
      sessionCreationReceipt({
        event: "session_created",
        session: "legacy",
        cwd: "/possibly-isolated",
        request_id: "ours",
      })
    ).toBeNull();
  });

  test("carries a baseline into a new draft only when its meaning is known", () => {
    expect(sessionCreationBaseline(null)).toBeUndefined();
    expect(
      sessionCreationBaseline({
        worktree_path: "/legacy-worktree",
        worktree_baseline: null,
      })
    ).toBeUndefined();
    expect(
      sessionCreationBaseline({
        worktree_path: null,
        worktree_baseline: null,
      })
    ).toBeNull();
    expect(
      sessionCreationBaseline({
        worktree_path: "/repo/.codetwo-worktrees/session",
        worktree_identity: { kind: "unix", device: 1, inode: 2 },
        worktree_baseline: {
          kind: "origin_default",
          ref: "refs/remotes/origin/main",
          sha: "0123456789abcdef",
          display: "origin/main",
        },
      })
    ).toBe("origin_default");
    expect(
      sessionCreationBaseline({
        worktree_path: "/legacy-with-baseline",
        worktree_identity: null,
        worktree_baseline: {
          kind: "current",
          ref: "HEAD",
          sha: "0123456789abcdef",
          display: "HEAD",
        },
      })
    ).toBeUndefined();
  });

  test("pins worktree creation to the SHA in the settled baseline preview", () => {
    const options = [
      {
        kind: "current" as const,
        resolved: {
          kind: "current" as const,
          ref: "HEAD",
          sha: "0123456789abcdef",
          display: "HEAD",
        },
        unavailable_reason: null,
      },
      {
        kind: "origin_default" as const,
        resolved: null,
        unavailable_reason: "origin/HEAD is unavailable",
      },
    ];

    expect(sessionCreationBaselineSha(null, options, true)).toBeNull();
    expect(
      sessionCreationBaselineSha("current", options, true)
    ).toBeUndefined();
    expect(sessionCreationBaselineSha("current", options, false)).toBe(
      "0123456789abcdef"
    );
    expect(
      sessionCreationBaselineSha("origin_default", options, false)
    ).toBeUndefined();
    expect(
      sessionCreationBaselineSha("origin_default", [], false)
    ).toBeUndefined();
  });

  test("gates the worktree picker only when every baseline is unavailable", () => {
    const unavailable = (
      kind: "current" | "origin_default",
      reason: string
    ) => ({
      kind,
      resolved: null,
      unavailable_reason: reason,
    });
    const gated = [
      unavailable("current", "not a git repository"),
      unavailable("origin_default", "not a git repository"),
    ];
    const partial = [
      {
        kind: "current" as const,
        resolved: {
          kind: "current" as const,
          ref: "HEAD",
          sha: "0123456789abcdef",
          display: "HEAD",
        },
        unavailable_reason: null,
      },
      unavailable("origin_default", "origin/HEAD is unavailable"),
    ];

    expect(worktreeGatingReason(false, gated, false)).toBe(
      "not a git repository"
    );
    // One usable baseline keeps the picker open; loading and empty results decide nothing yet.
    expect(worktreeGatingReason(false, partial, false)).toBeNull();
    expect(worktreeGatingReason(false, gated, true)).toBeNull();
    expect(worktreeGatingReason(false, [], false)).toBeNull();
    // An existing session renders its recorded worktree state and is never gated.
    expect(worktreeGatingReason(true, gated, false)).toBeNull();
  });
});

describe("command palette result union", () => {
  test("replaces a metadata session row with its conversation match", () => {
    const base = [
      { id: "settings" },
      { id: "session-a", identity: "session-a", label: "metadata" },
    ];
    const matches = [
      { id: "conversation-a", identity: "session-a", label: "matched snippet" },
    ];

    expect(mergeCommandResults(base, matches)).toEqual([
      { id: "settings" },
      { id: "conversation-a", identity: "session-a", label: "matched snippet" },
    ]);
  });
});

describe("execution policy projection", () => {
  test("hydrates both durable axes when a stored session is selected", () => {
    expect(
      sessionExecutionPolicy({
        permission_mode: "accept_edits",
        sandbox_policy: "read_only",
      })
    ).toEqual({ mode: "accept_edits", sandbox: "read_only" });
    expect(sessionExecutionPolicy(null)).toBeNull();
  });

  test("projects even legacy non-preset pairs without hiding the sandbox veto", () => {
    expect(sessionMode("accept_edits", "read_only")).toBe("read_only");
    expect(sessionMode("ask", "danger_full_access")).toBe("full_access");
  });

  test("reconciles only the session named by an authoritative policy event", () => {
    const rows = [
      {
        id: "session-a",
        permission_mode: "ask" as const,
        sandbox_policy: "workspace_write" as const,
      },
      {
        id: "session-b",
        permission_mode: "yolo" as const,
        sandbox_policy: "danger_full_access" as const,
      },
    ];

    expect(
      withSessionExecutionPolicy(rows, "session-a", {
        mode: "accept_edits",
        sandbox: "read_only",
      })
    ).toEqual([
      {
        id: "session-a",
        permission_mode: "accept_edits",
        sandbox_policy: "read_only",
      },
      rows[1],
    ]);
  });

  test("freezes the policy picker from creation capture through its authoritative receipt", () => {
    expect(executionPolicyChangeDisabled(true, null, new Set())).toBe(true);
    expect(
      executionPolicyChangeDisabled(false, "session-a", new Set(["session-a"]))
    ).toBe(true);
    expect(
      executionPolicyChangeDisabled(false, "session-b", new Set(["session-a"]))
    ).toBe(false);
  });
});

test("provider replacement is metadata and never creates or mutates a turn", () => {
  const turns = [newTurn("Keep this draft")];
  expect(
    applyEvent(turns, {
      event: "provider_changed",
      session: "s",
      provider: "pi",
      model: null,
    })
  ).toBe(turns);
  expect(
    applyEvent([], {
      event: "provider_changed",
      session: "s",
      provider: "pi",
      model: null,
    })
  ).toEqual([]);
});
