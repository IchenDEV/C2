#!/usr/bin/env node
import { launch } from "../common/entry.mjs";
import { createOpenCodeV1Adapter } from "./adapter.mjs";

await launch({
  backend: "opencode-v1",
  packageName: "@opencode-ai/sdk",
  packageJson: new URL("./package.json", import.meta.url),
  makeAdapter: (sdk, host, ctx) => createOpenCodeV1Adapter({ sdk, host, ...ctx }),
});
