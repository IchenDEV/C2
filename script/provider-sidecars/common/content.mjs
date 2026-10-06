// Neutral helpers shared by adapters: ACP-shaped tool content, titles, permission options and
// question schemas. These shapes are the C2 vocabulary, not any one SDK's.

export const MAX_TITLE_CHARS = 200;
export const MAX_TOOL_OUTPUT_CHARS = 64 * 1024;

export function truncate(text, limit = MAX_TITLE_CHARS) {
  const value = String(text ?? "");
  return value.length > limit ? `${value.slice(0, limit - 1)}…` : value;
}

export function tail(text, limit = MAX_TOOL_OUTPUT_CHARS) {
  const value = String(text ?? "");
  return value.length > limit ? value.slice(value.length - limit) : value;
}

/** ACP tool-call content for plain text; empty text yields no content at all. */
export function textContent(text) {
  const value = String(text ?? "");
  return value ? [{ type: "content", content: { type: "text", text: tail(value) } }] : undefined;
}

export const PERMISSION_OPTIONS = Object.freeze({
  allowOnce: { id: "allow", name: "Allow", kind: "allow_once" },
  allowAlways: { id: "allow_always", name: "Always allow", kind: "allow_always" },
  rejectOnce: { id: "deny", name: "Deny", kind: "reject_once" },
});

/** Build a question schema: one string (or string-array) property per question. */
export function questionSchema(questions) {
  const properties = {};
  const required = [];
  for (const q of questions) {
    if (!q?.id) continue;
    const choices = (q.options ?? [])
      .filter((o) => o?.label)
      .map((o) => ({ const: o.label, title: o.label, description: o.description ?? "" }));
    const base = { title: q.header || q.id, description: q.question ?? "" };
    if (q.multiSelect) {
      properties[q.id] = { ...base, type: "array", items: { anyOf: choices } };
    } else {
      properties[q.id] = { ...base, type: "string", ...(choices.length ? { oneOf: choices } : {}) };
    }
    required.push(q.id);
  }
  return { type: "object", properties, required };
}

/** Normalize host content blocks ({type:text|image}) into plain parts for SDK mappers. */
export function splitContent(content) {
  const parts = [];
  for (const block of Array.isArray(content) ? content : []) {
    if (block?.type === "text" && typeof block.text === "string") parts.push({ type: "text", text: block.text });
    else if (block?.type === "image" && typeof block.data === "string") {
      parts.push({ type: "image", data: block.data, mimeType: block.mimeType || "image/png" });
    }
  }
  return parts;
}

export function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Async queue used as a streaming-input prompt. Closing ends the SDK's input. */
export function createInputQueue() {
  const items = [];
  const waiters = [];
  let closed = false;
  return {
    push(item) {
      if (closed) return false;
      const waiter = waiters.shift();
      if (waiter) waiter({ value: item, done: false });
      else items.push(item);
      return true;
    },
    close() {
      closed = true;
      while (waiters.length) waiters.shift()({ value: undefined, done: true });
    },
    [Symbol.asyncIterator]() {
      return {
        next: () => {
          if (items.length) return Promise.resolve({ value: items.shift(), done: false });
          if (closed) return Promise.resolve({ value: undefined, done: true });
          return new Promise((resolve) => waiters.push(resolve));
        },
        return: () => {
          closed = true;
          return Promise.resolve({ value: undefined, done: true });
        },
      };
    },
  };
}
