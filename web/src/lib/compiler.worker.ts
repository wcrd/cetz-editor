// Runs the Typst compiler off the main thread, fetching packages on demand.

import init, { Compiler, type CompileOutput } from "./wasm/cetz_worker";
import { loadPackage } from "./packages";
import type { CompileRequest, Diagnostic, ExportRequest, WorkerMessage, WorkerRequest } from "./compiler";
import type { Probe } from "./probe";

/** The uncompressed size of the compiler's WebAssembly, set by the build. */
declare const __WORKER_WASM_BYTES__: number;

/**
 * Fetches the compiler, reporting progress: it's tens of megabytes, so the
 * first visit can take a while. Streams it into compilation as it arrives.
 */
async function load(): Promise<Response> {
  const res = await fetch(new URL("./wasm/cetz_worker_bg.wasm", import.meta.url));
  // The body arrives decompressed, so Content-Length (compressed) won't do.
  const size = Number(res.headers.get("content-length")) || 0;
  const total = __WORKER_WASM_BYTES__ || size;
  if (!res.ok || !res.body || !total) return res;
  let loaded = 0;
  let reported = 0;
  const counted = res.body.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, controller) {
        loaded += chunk.byteLength;
        // Every half percent is plenty, and keeps a cached load from flooding the page.
        if (loaded - reported >= total / 200 || loaded >= total) {
          reported = loaded;
          post({ kind: "loading", progress: Math.min(loaded / total, 1), size: size || total });
        }
        controller.enqueue(chunk);
      },
    }),
  );
  return new Response(counted, { headers: res.headers });
}

const compiler = init({ module_or_path: load() }).then(() => new Compiler());
const inflight = new Map<string, Promise<void>>();
let latest = 0;

function post(message: WorkerMessage, transfer: Transferable[] = []) {
  self.postMessage(message, { transfer });
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

/**
 * Whether the WebAssembly side died: a Rust panic (a trap) or a stack
 * overflow. Its state is then unusable, and every later call fails, so the
 * main thread replaces this worker.
 */
function isCrash(err: unknown): boolean {
  return err instanceof WebAssembly.RuntimeError || err instanceof RangeError;
}

let pending: CompileRequest | undefined;

// Requests queue up while a compile runs (e.g. during a drag). Defer to a
// fresh task so all queued messages land first, then compile only the newest.
self.onmessage = (e: MessageEvent<WorkerRequest>) => {
  if (e.data.kind === "export") {
    const request = e.data;
    runExport(request).catch((err) => post({ id: request.id, kind: "exported", error: String(err), crashed: isCrash(err) }));
    return;
  }
  latest = e.data.id;
  const first = pending === undefined;
  pending = e.data;
  if (first) setTimeout(() => {
    const request = pending!;
    pending = undefined;
    run(request).catch((err) => {
      // Always answer, or the editor waits on this compile forever.
      console.error("compile failed", err);
      const crashed = isCrash(err);
      const message = crashed ? `The compiler crashed and was restarted (${err}). This diagram may need more memory than the browser allows.` : `internal compiler error: ${err}`;
      post({ id: request.id, kind: "done", diagnostics: [{ error: true, message }], ms: 0, crashed });
    });
  });
};

async function run({ id, source }: CompileRequest) {
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

  // Probe positions are per page; shift them onto the stacked SVG.
  const offsets = [0];
  for (const h of out.page_heights) offsets.push(offsets[offsets.length - 1] + h);
  const probes: Probe[] | undefined = out.probes ? JSON.parse(out.probes) : undefined;
  for (const p of probes ?? []) p.origin.y += offsets[p.origin.page - 1] ?? 0;
  const pageHeights = [...out.page_heights];
  const result = { svg: out.svg, diagnostics: out.diagnostics.map(toPlain), probes, pageHeights, ms };
  out.free();
  post({ id, kind: "done", ...result });
}

/** "message (line 3)", for the first error, or undefined if there are none. */
function describeError(diagnostics: Diagnostic[]): string | undefined {
  const d = diagnostics.find((d) => d.error);
  if (!d) return undefined;
  const where = d.line === undefined ? "" : ` (${d.file ? `${d.file}, ` : ""}line ${d.line + 1})`;
  return d.message + where;
}

async function runExport({ id, source, format, pixelPerPt }: ExportRequest) {
  const c = await compiler;
  const attempted = new Set<string>();
  for (;;) {
    const out = c.export(source, format, pixelPerPt);
    const missing = out.missing_packages.filter((spec) => !attempted.has(spec));
    if (missing.length > 0) {
      out.free();
      missing.forEach((spec) => attempted.add(spec));
      await Promise.all(missing.map((spec) => addPackage(c, spec)));
      continue;
    }
    const data = out.data;
    const error = describeError(out.diagnostics.map(toPlain));
    out.free();
    if (data) post({ id, kind: "exported", data }, [data.buffer]);
    else post({ id, kind: "exported", error: error ?? "export failed" });
    return;
  }
}
