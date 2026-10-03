// Runs the Typst compiler off the main thread, fetching packages on demand.

import init, { Compiler, type CompileOutput } from "./wasm/cetz_worker";
import { loadPackage } from "./packages";
import type { Diagnostic, WorkerMessage, WorkerRequest } from "./compiler";

const compiler = init().then(() => new Compiler());
const inflight = new Map<string, Promise<void>>();
let latest = 0;

function post(message: WorkerMessage) {
  self.postMessage(message);
}

function addPackage(c: Compiler, spec: string): Promise<void> {
  let p = inflight.get(spec);
  if (!p) {
    p = loadPackage(spec).then((bytes) => {
      if (!c.has_package(spec)) c.add_package(spec, bytes);
    });
    p.catch(() => inflight.delete(spec));
    inflight.set(spec, p);
  }
  return p;
}

/** Copies a wasm-owned diagnostic into a plain object and frees it. */
function toPlain(d: CompileOutput["diagnostics"][number]): Diagnostic {
  const plain = {
    error: d.error,
    message: d.message,
    file: d.file,
    line: d.line,
    column: d.column,
  };
  d.free();
  return plain;
}

let pending: WorkerRequest | undefined;

// Requests queue up while a compile runs (e.g. during a drag). Defer to a
// fresh task so all queued messages land first, then compile only the newest.
self.onmessage = (e: MessageEvent<WorkerRequest>) => {
  latest = e.data.id;
  const first = pending === undefined;
  pending = e.data;
  if (first) setTimeout(() => {
    const request = pending!;
    pending = undefined;
    run(request).catch((err) => {
      // Always answer, or the editor waits on this compile forever.
      console.error("compile failed", err);
      post({ id: request.id, kind: "done", diagnostics: [{ error: true, message: `internal compiler error: ${err}` }], ms: 0 });
    });
  });
};

async function run({ id, source }: WorkerRequest) {
  const c = await compiler;

  let ms = 0;
  const compile = () => {
    c.set_main(source);
    const start = performance.now();
    const out = c.compile();
    ms = performance.now() - start;
    return out;
  };

  let out = compile();
  const attempted = new Set<string>();
  for (;;) {
    const missing = out.missing_packages.filter((spec) => !attempted.has(spec));
    if (missing.length === 0) break;
    out.free();
    missing.forEach((spec) => attempted.add(spec));
    post({ id, kind: "fetching", packages: missing });
    try {
      await Promise.all(missing.map((spec) => addPackage(c, spec)));
    } catch (err) {
      post({ id, kind: "done", diagnostics: [{ error: true, message: String(err) }], ms: 0 });
      return;
    }
    // A newer request may have arrived while fetching; it takes over.
    if (id !== latest) return;
    out = compile();
  }

  const result = {
    svg: out.svg,
    diagnostics: out.diagnostics.map(toPlain),
    probes: out.probes ? JSON.parse(out.probes) : undefined,
    ms,
  };
  out.free();
  post({ id, kind: "done", ...result });
}
