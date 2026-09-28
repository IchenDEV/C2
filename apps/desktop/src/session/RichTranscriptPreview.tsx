import { useState } from "react";

import { Button } from "@/components/ui/button";

import { EnvironmentPopover } from "../environment/EnvironmentPopover";
import { TrajectoryView } from "./TrajectoryView";
import { TranscriptPane } from "./TranscriptPane";
import type { Turn } from "./turns";

const chart = `\`\`\`chart
{"type":"bar","title":"Renderer verification","xLabel":"Check","yLabel":"Passed assertions","labels":["Transcript order","Markdown","Chart","Visualize"],"series":[{"name":"Desktop","values":[12,8,6,7]},{"name":"Narrow window","values":[10,8,6,7]}]}
\`\`\``;

const previewStartedAt = Date.now() - 31_000;

const previewTurn: Turn = {
  id: 1,
  transcriptStartSeq: 1,
  accepted: true,
  streamBoundaryKnown: true,
  prompt:
    "检查新的对话渲染：文本、工具调用、图表和交互式可视化应当在同一条流里按顺序出现。",
  text:
    `我先核对转录事件和渲染入口，确认现有流式边界。\n\n` +
    `入口已经确认，下面验证图表在真实内容宽度下的排版。\n\n${chart}\n\n` +
    "完整验证仍在进行，当前结果如下。\n\n" +
    'visualize{"path":"/__codetwo__/rich-transcript-preview.html","mode":"wide","title":"Release confidence"}',
  textDeltas: [],
  observedTextDeltas: 3,
  observedThoughtDeltas: 0,
  pendingTextDeltaSkips: 0,
  pendingThoughtDeltaSkips: 0,
  thoughts: [
    "I should inspect the shared renderer before changing the presentation.",
  ],
  tools: [
    {
      id: "inspect-transcript",
      title: "Inspect transcript pipeline",
      status: "completed",
      kind: "read",
      startedAt: previewStartedAt + 4000,
      endedAt: previewStartedAt + 11_000,
      outputs: [
        {
          type: "text",
          text: "Located the shared turn reducer, transcript projection, and desktop renderer.",
        },
      ],
    },
    {
      id: "renderer-tests",
      title: "Run renderer verification",
      status: "in_progress",
      kind: "test",
      startedAt: previewStartedAt + 21_000,
      outputs: [],
    },
    {
      id: "agent-accessibility",
      title: "spawn_agent",
      status: "in_progress",
      kind: "agent",
      agentInput: {
        agent_type: "explorer",
        task_name: "accessibility_review",
        message: "Check the transcript controls and status announcements.",
      },
      startedAt: previewStartedAt + 8000,
      outputs: [],
    },
    {
      id: "agent-layout",
      title: "spawn_agent",
      status: "completed",
      kind: "agent",
      agentInput: {
        agent_type: "worker",
        task_name: "narrow_layout",
        message: "Verify the transcript at a narrow desktop width.",
      },
      startedAt: previewStartedAt + 2000,
      endedAt: previewStartedAt + 18_000,
      outputs: [],
    },
    {
      id: "agent-tests",
      title: "spawn_agent",
      status: "failed",
      kind: "agent",
      agentInput: {
        agent_type: "worker",
        task_name: "renderer_tests",
        message: "Run the renderer regression suite.",
      },
      startedAt: previewStartedAt + 3000,
      endedAt: previewStartedAt + 14_000,
      outputs: [],
    },
  ],
  content: [
    {
      kind: "text",
      text: "我先核对转录事件和渲染入口，确认现有流式边界。\n\n",
      createdAt: previewStartedAt + 1000,
    },
    {
      kind: "tool",
      toolId: "inspect-transcript",
      createdAt: previewStartedAt + 11_000,
    },
    {
      kind: "text",
      text: `入口已经确认，下面验证图表在真实内容宽度下的排版。\n\n${chart}\n\n`,
      createdAt: previewStartedAt + 13_000,
    },
    {
      kind: "tool",
      toolId: "renderer-tests",
      createdAt: previewStartedAt + 21_000,
    },
    {
      kind: "tool",
      toolId: "agent-accessibility",
      createdAt: previewStartedAt + 22_000,
    },
    {
      kind: "tool",
      toolId: "agent-layout",
      createdAt: previewStartedAt + 23_000,
    },
    {
      kind: "tool",
      toolId: "agent-tests",
      createdAt: previewStartedAt + 24_000,
    },
    {
      kind: "text",
      text:
        "完整验证仍在进行，当前结果如下。\n\n" +
        'visualize{"path":"/__codetwo__/rich-transcript-preview.html","mode":"wide","title":"Release confidence"}',
      createdAt: previewStartedAt + 24_000,
    },
  ],
  startedAt: previewStartedAt,
  endedAt: previewStartedAt + 25_000,
};
const previewTurns = [previewTurn] as const;

export function RichTranscriptPreview() {
  const [trajectory, setTrajectory] = useState(false);
  return (
    <div className="bg-background text-foreground flex h-screen min-h-0 flex-col">
      <header className="bg-fill-quiet flex shrink-0 items-center gap-2 px-5 py-3">
        <p className="text-body font-medium">Rich conversation</p>
        <span className="text-callout text-muted-foreground ms-auto">
          Streaming
        </span>
        <EnvironmentPopover
          project="codeTwo"
          projectPath="/tmp/codeTwo"
          projects={[]}
          git={{
            is_repo: true,
            branch: "codex/tasks",
            ahead: 0,
            behind: 0,
            files: [],
          }}
          diffStat={{ added: 42, deleted: 8 }}
          onRefresh={() => {
            /* empty */
          }}
          onSelectProject={() => {
            /* empty */
          }}
          onAddProject={() => {
            /* empty */
          }}
          onOpenSourceControl={() => {
            /* empty */
          }}
          onOpenSettings={() => {
            /* empty */
          }}
        />
        <Button
          type="button"
          size="compact"
          variant="secondary"
          onClick={() => setTrajectory((current) => !current)}
        >
          {trajectory ? "Conversation" : "Trajectory"}
        </Button>
      </header>
      <main className="flex min-h-0 flex-1">
        {trajectory ? (
          <TrajectoryView
            turns={previewTurns}
            usage={{ input_tokens: 12_480, output_tokens: 3206 }}
            hasEarlier={false}
            loadingEarlier={false}
            onLoadEarlier={() => {
              /* empty */
            }}
          />
        ) : (
          <TranscriptPane
            sessionId="rich-transcript-preview"
            variant="main"
            turns={previewTurns}
            loading={false}
            hasEarlier={false}
            loadingEarlier={false}
            onLoadEarlier={() => {
              /* empty */
            }}
            onForkTurn={() => {
              /* empty */
            }}
            onAddSelection={() => {
              /* empty */
            }}
            onExplainSelection={() => {
              /* empty */
            }}
            onAskSelectionInSideChat={() => {
              /* empty */
            }}
          />
        )}
      </main>
      <footer className="shrink-0 px-5 pb-4">
        <div className="rounded-module bg-card text-body text-muted-foreground mx-auto max-w-3xl border px-4 py-3 shadow-(--ds-elevation-surface)">
          Ask a follow-up…
        </div>
      </footer>
    </div>
  );
}
