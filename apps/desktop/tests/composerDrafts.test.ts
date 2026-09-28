import { describe, expect, test } from "bun:test";

import type { DocBlock } from "../src/bridge";
import {
  COMPOSER_DRAFT_STORAGE_KEY,
  composerDraftScopeKey,
  loadComposerDrafts,
  promoteComposerDraft,
  saveComposerDrafts,
  updateComposerDraft,
} from "../src/session/composerDrafts";
import type {
  ComposerDraftPosture,
  ComposerDraftScope,
  StorageLike,
} from "../src/session/composerDrafts";

class MemoryStorage implements StorageLike {
  values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }
}

const posture: ComposerDraftPosture = {
  provider: "codex",
  model: "gpt-5",
  mode: "ask",
  sandbox: "workspace_write",
  worktreeBase: "current",

  memoryRead: "inherit",
  memoryWrite: "allow",
  scene: "review",
  autoScene: false,
};

const project: ComposerDraftScope = { kind: "project", projectPath: "/work/a" };
const otherProject: ComposerDraftScope = {
  kind: "project",
  projectPath: "/work/b",
};
const doc: DocBlock[] = [{ type: "text", text: "Keep this draft" }];

describe("composer drafts", () => {
  test("assigns identity only to invested drafts and keeps it stable", () => {
    let drafts = updateComposerDraft(
      new Map(),
      {
        scope: project,
        doc: [],
        attachments: [],
        posture,
      },
      { createId: () => "draft-1", now: 1 }
    );
    expect(drafts.size).toBe(0);

    drafts = updateComposerDraft(
      drafts,
      {
        scope: project,
        doc,
        attachments: [],
        posture,
      },
      { createId: () => "draft-1", now: 2 }
    );
    expect(drafts.get(composerDraftScopeKey(project))?.id).toBe("draft-1");

    drafts = updateComposerDraft(
      drafts,
      {
        scope: project,
        doc: [{ type: "text", text: "Changed" }],
        attachments: [],
        posture,
      },
      { createId: () => "must-not-be-used", now: 3 }
    );
    expect(drafts.get(composerDraftScopeKey(project))?.id).toBe("draft-1");

    drafts = updateComposerDraft(drafts, {
      scope: project,
      doc: [],
      attachments: [],
      posture,
    });
    expect(drafts.size).toBe(0);
  });

  test("promotes a project draft without changing identity or overwriting a destination", () => {
    let drafts = updateComposerDraft(
      new Map(),
      {
        scope: project,
        doc,
        attachments: [],
        posture,
      },
      { createId: () => "draft-source", now: 1 }
    );
    const session: ComposerDraftScope = {
      kind: "session",
      sessionId: "session-1",
      projectPath: "/work/a",
    };
    const moved = promoteComposerDraft(drafts, project, session, 2);
    expect(moved.outcome).toBe("moved");
    expect(moved.drafts.has(composerDraftScopeKey(project))).toBe(false);
    expect(moved.drafts.get(composerDraftScopeKey(session))?.id).toBe(
      "draft-source"
    );

    drafts = updateComposerDraft(
      moved.drafts,
      {
        scope: otherProject,
        doc: [{ type: "text", text: "Destination" }],
        attachments: [],
        posture,
      },
      { createId: () => "draft-destination", now: 3 }
    );
    const conflict = promoteComposerDraft(drafts, session, otherProject, 4);
    expect(conflict.outcome).toBe("conflict");
    expect(conflict.drafts.get(composerDraftScopeKey(otherProject))?.id).toBe(
      "draft-destination"
    );
    expect(conflict.drafts.get(composerDraftScopeKey(session))?.id).toBe(
      "draft-source"
    );
  });

  test("round-trips minimal attachment references and execution posture", () => {
    const storage = new MemoryStorage();
    const drafts = updateComposerDraft(
      new Map(),
      {
        scope: project,
        doc: [
          ...doc,
          {
            type: "skill",
            skill_id: "review",
            params: { focus: "correctness" },
          },
        ],
        attachments: [
          { id: "image-1", kind: "attachment", name: "Diagram.png" },
        ],
        posture,
      },
      { createId: () => "draft-1", now: 10 }
    );

    expect(saveComposerDrafts(drafts, storage)).toBe(true);
    const raw = storage.getItem(COMPOSER_DRAFT_STORAGE_KEY) ?? "";
    expect(raw).not.toContain("preview_data_url");
    const loaded = loadComposerDrafts(storage);
    expect(loaded.warning).toBeNull();
    expect(loaded.drafts.get(composerDraftScopeKey(project))).toEqual(
      drafts.get(composerDraftScopeKey(project))
    );
  });

  test("rejects corrupt or unavailable storage without throwing", () => {
    const corrupt = new MemoryStorage();
    corrupt.setItem(
      COMPOSER_DRAFT_STORAGE_KEY,
      JSON.stringify({ version: 1, drafts: [{}] })
    );
    expect(loadComposerDrafts(corrupt)).toEqual({
      drafts: new Map(),
      warning: "corrupt",
    });
    corrupt.setItem(COMPOSER_DRAFT_STORAGE_KEY, "{not-json");
    expect(loadComposerDrafts(corrupt).warning).toBe("corrupt");

    const unavailable: StorageLike = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(loadComposerDrafts(unavailable).warning).toBe("unavailable");
    expect(saveComposerDrafts(new Map(), unavailable)).toBe(false);
  });

  test("refuses an oversized snapshot without replacing the last good copy", () => {
    const storage = new MemoryStorage();
    storage.setItem(COMPOSER_DRAFT_STORAGE_KEY, "last-good-copy");
    const drafts = updateComposerDraft(
      new Map(),
      {
        scope: project,
        doc: [{ type: "text", text: "界".repeat(2_000_000) }],
        attachments: [],
        posture,
      },
      { createId: () => "draft-too-large", now: 1 }
    );

    expect(saveComposerDrafts(drafts, storage)).toBe(false);
    expect(storage.getItem(COMPOSER_DRAFT_STORAGE_KEY)).toBe("last-good-copy");
  });
});

test("legacy plan posture is discarded without losing the draft", () => {
  const storage = new MemoryStorage();
  const drafts = updateComposerDraft(
    new Map(),
    {
      scope: project,
      doc,
      attachments: [],
      posture: { ...posture, planMode: true } as ComposerDraftPosture,
    },
    { createId: () => "legacy-draft", now: 1 }
  );
  saveComposerDrafts(drafts, storage);
  const loaded = loadComposerDrafts(storage).drafts.get(
    composerDraftScopeKey(project)
  );
  expect(loaded?.doc).toEqual(doc);
  expect(loaded?.posture.provider).toBe("codex");
  expect(loaded?.posture).not.toHaveProperty("planMode");
});
