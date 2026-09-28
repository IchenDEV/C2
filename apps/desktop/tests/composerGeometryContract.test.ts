import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const normalizeSource = (source: string) => source.replaceAll(/\r\n?/gu, "\n");
const readSource = (relativePath: string) =>
  normalizeSource(
    readFileSync(new URL(relativePath, import.meta.url), "utf-8")
  );

const composer = readSource("../src/session/Composer.tsx");
const app = readSource("../src/App.tsx");
const bridge = readSource("../src/bridge.ts");
const tokens = readSource("../src/design/tokens.css");
const voiceButton = readSource("../src/voice/VoiceButton.tsx");
const controlChip = readSource("../src/components/ui/control-chip.tsx");
const styles = readSource("../src/styles.css");

describe("composer geometry contract", () => {
  test("normalizes source contracts across platform line endings", () => {
    expect(normalizeSource("first\r\nsecond\rthird")).toBe(
      "first\nsecond\nthird"
    );
  });

  test("paints the compact composer on the same DOM card as its interactive content", () => {
    expect(composer).not.toContain("function ComposerLiquidSurface");
    expect(composer).not.toContain("data-gooey-composer");
    expect(composer).toContain('"rounded-composer bg-raised shadow-raised"');
  });

  test("keeps the circular submit control concentric with the composer corner", () => {
    expect(tokens).toContain(
      "--ds-composer-radius: calc(\n    var(--ds-radius-modal) + var(--ds-space-module-inset)\n  );"
    );
    expect(composer).toContain(
      'data-composer-mode={docMode ? "document" : "compact"}'
    );
    expect(composer).toContain('"composer-mode-transition flex flex-col"');
    expect(composer).not.toContain("reducedMotion");
    expect(composer).toContain(
      '"rounded-composer bg-raised shadow-raised pointer-events-auto mx-auto w-full max-w-3xl p-2"'
    );
    expect(composer.match(/"size-8 shrink-0 rounded-full"/gu)).toHaveLength(2);
    expect(
      composer.match(/className="[^"]*size-7 shrink-0 rounded-full[^"]*"/gu)
    ).toHaveLength(2);
    expect(composer).toMatch(
      /variant="ghost"\s+size="compact"\s+focusStyle="inset"/u
    );
    expect(composer).toContain(
      'import { ControlChip as Chip } from "@/components/ui/control-chip";'
    );
    expect(controlChip).toContain('size="compact"');
    expect(controlChip).toContain('data-slot="control-chip"');
    expect(voiceButton).toContain('className="size-8 shrink-0 rounded-full"');
    expect(composer).toMatch(
      /<Button\s[^>]*variant="secondary"[^>]*onClick=\{onStop\}/u
    );
    expect(composer).not.toContain('"px-3 pb-2.5 pt-1.5"');
    expect(composer).not.toContain(
      'className="size-8 shrink-0 rounded-(--ds-radius-control) transition-transform active:scale-90"'
    );
  });

  test("does not advertise the removed composer resize grip", () => {
    expect(composer).not.toContain("composer-grip");
    expect(composer).not.toContain("useResizeHandle");
    expect(composer).toContain("const applied = Math.min(190, maxHeight);");
    expect(styles).not.toContain(".composer-grip");
    expect(app).not.toContain("codetwo.composerHeight");
    expect(app).not.toContain("composerHByPane");
    expect(composer).toContain("aria-label={");
    expect(composer).toContain('? t("composer.collapseLabel")');
    expect(composer).toContain(': t("composer.expandLabel")');
  });

  test("keeps the interactive Project name typographically continuous with the empty heading", () => {
    expect(app).toContain(
      "[font-size:inherit] [line-height:inherit] [font-weight:inherit] [letter-spacing:inherit]"
    );
    expect(app).toContain('</DropdownMenu>{" "}');
    expect(app).toContain('{t("transcript.greetingEnd")}');
  });
});

describe("composer multitask contract", () => {
  test("starts a parallel task in an isolated worktree instead of queueing it", () => {
    expect(composer).toContain("onMultitask: () => void;");
    expect(composer).toContain("<DropdownMenuItem onClick={onMultitask}>");
    expect(composer).toContain('t("composer.multitask")');
    expect(app).toContain("onMultitask={startParallelTask}");
    expect(app).toMatch(/sessionCreationBaselineSha\(\s*"current"/u);
    expect(app).toContain("parallelTask: true");
    expect(bridge).toContain('call("engine.new_parallel_task"');
    expect(bridge).toContain("task_id: parallelTask.taskId");
    expect(bridge).toContain("goal: parallelTask.goal");
  });
});
