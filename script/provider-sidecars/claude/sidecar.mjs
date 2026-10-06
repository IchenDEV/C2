#!/usr/bin/env node
import { launch } from "../common/entry.mjs";
import { createClaudeAdapter } from "./adapter.mjs";

await launch({
  backend: "claude-agent-sdk",
  packageName: "@anthropic-ai/claude-agent-sdk",
  packageJson: new URL("./package.json", import.meta.url),
  makeAdapter: (sdk, host, ctx) => createClaudeAdapter({ sdk, host, ...ctx }),
});
