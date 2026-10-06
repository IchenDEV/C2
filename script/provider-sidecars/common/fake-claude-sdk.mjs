// Test-only stand-in for `@anthropic-ai/claude-agent-sdk`, loaded through CODETWO_SIDECAR_SDK_MODULE
// so the real sidecar process and the Rust `SidecarRuntime` can be exercised end to end without a
// model, credentials or network. It proves the process/IPC contract, not Claude behavior.

export function query({ prompt, options }) {
  const out = [];
  const waiters = [];
  const emit = (message) => {
    const waiter = waiters.shift();
    if (waiter) waiter({ value: message, done: false });
    else out.push(message);
  };
  let closed = false;
  let turnAborted = false;

  (async () => {
    for await (const message of prompt) {
      const text = message.message.content.map((b) => b.text ?? "").join("");
      turnAborted = false;
      if (text.includes("crash")) process.exit(1);
      emit({ type: "stream_event", event: { type: "message_start", message: { id: "m" } } });
      emit({ type: "stream_event", event: { type: "content_block_delta", delta: { type: "text_delta", text: "Hello from fake" } } });
      if (text.includes("tool")) {
        const decision = await options.canUseTool("Bash", { command: "echo hi" }, { toolUseID: "t1", suggestions: [] });
        emit({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "t1", is_error: decision.behavior !== "allow", content: [{ type: "text", text: decision.behavior }] }] } });
      }
      if (text.includes("hang")) {
        // Wait for an interrupt instead of finishing.
        await new Promise((resolve) => {
          const timer = setInterval(() => {
            if (turnAborted) {
              clearInterval(timer);
              resolve();
            }
          }, 5);
        });
        emit({ type: "result", subtype: "error_during_execution", terminal_reason: "aborted_streaming", is_error: true, errors: [] });
        continue;
      }
      emit({ type: "result", subtype: "success", is_error: false, result: "ok", stop_reason: "end_turn", terminal_reason: "completed", usage: { input_tokens: 3, output_tokens: 2 }, modelUsage: { fake: { contextWindow: 1000 } }, total_cost_usd: 0 });
    }
  })();

  return {
    [Symbol.asyncIterator]() {
      return this;
    },
    next() {
      if (out.length) return Promise.resolve({ value: out.shift(), done: false });
      if (closed) return Promise.resolve({ value: undefined, done: true });
      return new Promise((resolve) => waiters.push(resolve));
    },
    async interrupt() {
      turnAborted = true;
    },
    async supportedModels() {
      return [{ value: "fake", displayName: "Fake", description: "fixture" }];
    },
    async setModel() {},
    async applyFlagSettings() {},
    close() {
      closed = true;
      while (waiters.length) waiters.shift()({ value: undefined, done: true });
    },
  };
}
