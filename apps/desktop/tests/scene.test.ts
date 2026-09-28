import { describe, expect, test } from "bun:test";

import {
  MEMORY_PRESET_POLICY,
  escalationNeeded,
  nextSceneInRing,
  sceneCustomized,
  sceneTitle,
  softApplyPending,
} from "../src/session/scene";
import type { LivePosture, SceneInfo } from "../src/session/scene";

function scene(overrides: Partial<SceneInfo> = {}): SceneInfo {
  return {
    reference: "builtin:develop",
    name: "develop",
    title: "Develop",
    description: "",
    source: "builtin",
    keywords: [],
    has_brief: false,
    localizations: {},
    artifacts: [],
    ...overrides,
  };
}

const live: LivePosture = {
  mode: "ask",
  memoryRead: "inherit",
  memoryWrite: "inherit",
  provider: "claude_code",
  model: "m1",
};

describe("sceneCustomized", () => {
  test("a scene with no execution can never be customized", () => {
    expect(sceneCustomized(scene(), live)).toBe(false);
  });

  test("only fields the scene sets participate", () => {
    const s = scene({ execution: { session_mode: "ask" } });
    // Memory/provider all differ from nothing — the scene doesn't set them.
    expect(sceneCustomized(s, { ...live, provider: "codex" })).toBe(false);
    expect(sceneCustomized(s, { ...live, mode: "auto_edit" })).toBe(true);
  });

  test("memory preset compares as the (read, write) pair", () => {
    const s = scene({ execution: { memory_preset: "private" } });
    expect(
      sceneCustomized(s, { ...live, memoryRead: "deny", memoryWrite: "deny" })
    ).toBe(false);
    expect(
      sceneCustomized(s, { ...live, memoryRead: "allow", memoryWrite: "deny" })
    ).toBe(true);
    expect(MEMORY_PRESET_POLICY.learn_only).toEqual({
      read: "deny",
      write: "allow",
    });
  });
});

describe("softApplyPending (binding matrix)", () => {
  test("session-creation fields pend; posture fields do not", () => {
    const s = scene({
      execution: {
        session_mode: "read_only",
        memory_preset: "standard",
        providers: ["codex"],
        model: "m2",
        reasoning_effort: "high",
        worktree: "current",
      },
    });
    expect(softApplyPending(s, live)).toEqual([
      "providers",
      "model",
      "reasoning_effort",
      "worktree",
    ]);
  });

  test("a matching live value is not pending", () => {
    const s = scene({ execution: { providers: ["claude_code"], model: "m1" } });
    expect(softApplyPending(s, live)).toEqual([]);
  });
});

describe("escalationNeeded", () => {
  test("is asymmetric: loosening needs confirmation, tightening never does", () => {
    const looser = scene({ execution: { session_mode: "full_access" } });
    const tighter = scene({ execution: { session_mode: "read_only" } });
    expect(escalationNeeded(looser, "ask")).toEqual({
      from: "ask",
      to: "full_access",
    });
    expect(escalationNeeded(tighter, "full_access")).toBeNull();
    expect(escalationNeeded(looser, "full_access")).toBeNull();
    expect(escalationNeeded(scene(), "read_only")).toBeNull();
  });
});

describe("nextSceneInRing", () => {
  const scenes = [
    scene({ reference: "builtin:research" }),
    scene({ reference: "builtin:develop" }),
    scene({ reference: "builtin:test" }),
  ];

  test("walks listing order and wraps", () => {
    expect(nextSceneInRing([], scenes, "builtin:research")).toBe(
      "builtin:develop"
    );
    expect(nextSceneInRing([], scenes, "builtin:test")).toBe(
      "builtin:research"
    );
    expect(nextSceneInRing([], scenes, null)).toBe("builtin:research");
  });

  test("an explicit ring wins over listing order", () => {
    expect(
      nextSceneInRing(
        ["builtin:test", "builtin:research"],
        scenes,
        "builtin:test"
      )
    ).toBe("builtin:research");
  });

  test("nothing to cycle to yields null", () => {
    expect(nextSceneInRing([], [], null)).toBeNull();
    expect(nextSceneInRing([], [scenes[0]], "builtin:research")).toBeNull();
  });
});

describe("sceneTitle", () => {
  test("prefers the UI locale and falls back to the authored title", () => {
    const s = scene({ localizations: { "zh-CN": { title: "开发" } } });
    expect(sceneTitle(s, "zh-CN")).toBe("开发");
    expect(sceneTitle(s, "en")).toBe("Develop");
  });
});
