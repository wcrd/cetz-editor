// Main-thread handle on the Typst compiler running in a Web Worker.

import type { Probe } from "./probe";

export interface Diagnostic {
  error: boolean;
  message: string;
  /** The file the diagnostic points into; undefined for the main source. */
  file?: string;
  /** Zero-based line and column, when known. */
  line?: number;
  column?: number;
}

export interface CompileResult {
  svg?: string;
  diagnostics: Diagnostic[];
  /** CeTZ geometry per draw call, when the compile succeeded. */
  probes?: Probe[];
  /** Page heights in points; pages are stacked top to bottom in `svg`. */
  pageHeights?: number[];
  /** Wall-clock compile time, excluding package fetches. */
  ms: number;
}

export type ExportFormat = "pdf" | "svg" | "png";

export type CompileRequest = { kind: "compile"; id: number; source: string };
/** Exports have their own ids, and are never coalesced. */
export type ExportRequest = { kind: "export"; id: number; source: string; format: ExportFormat; pixelPerPt: number };
export type WorkerRequest = CompileRequest | ExportRequest;

/** `crashed`: the worker's WebAssembly died, and it must be replaced. */
export type WorkerMessage =
  | { id: number; kind: "fetching"; packages: string[] }
  | ({ id: number; kind: "done"; crashed?: boolean } & CompileResult)
  | { id: number; kind: "exported"; data?: Uint8Array; error?: string; crashed?: boolean };

export type CompilerStatus =
  | { kind: "loading" }
  | { kind: "compiling" }
  | { kind: "fetching"; packages: string[] }
  | ({ kind: "done" } & CompileResult);

/**
 * Compiles sources in a worker, shared by every open document. Requests that
 * arrive while the worker is busy are coalesced (only the newest is
 * compiled); results are reported in order, never older than one already
 * reported, to the callback of the request they answer.
 */
export class TypstCompiler {
  #worker = this.#spawn();
  #latest = 0;
  #reported = 0;
  /** The newest compile request, to resend to a replacement worker. */
  #newest?: CompileRequest;
  #callbacks = new Map<number, (status: CompilerStatus, request: number) => void>();
  #exports = 0;
  #exported = new Map<number, { resolve: (data: Uint8Array) => void; reject: (err: Error) => void }>();

  #spawn(): Worker {
    const worker = new Worker(new URL("./compiler.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
      this.#receive(e.data);
      // After answering, so the request that crashed it isn't sent again.
      if (e.data.kind !== "fetching" && e.data.crashed) this.#restart();
    };
    return worker;
  }

  #receive(message: WorkerMessage) {
    if (message.kind === "exported") {
      const { id, data, error } = message;
      const pending = this.#exported.get(id);
      this.#exported.delete(id);
      if (data) pending?.resolve(data);
      else pending?.reject(new Error(error ?? "export failed"));
      return;
    }
    const { id, ...status } = message;
    if ("crashed" in status) delete status.crashed;
    if (id < this.#reported) return;
    this.#reported = id;
    const onStatus = this.#callbacks.get(id);
    if (status.kind === "done") {
      // Coalesced requests before this one will never be answered.
      for (const key of this.#callbacks.keys()) if (key <= id) this.#callbacks.delete(key);
    }
    onStatus?.(status, id);
  }

  /**
   * Replaces a worker whose WebAssembly died: nothing it was still doing
   * will finish, so exports fail and the newest compile goes to the new one.
   */
  #restart() {
    this.#worker.terminate();
    this.#worker = this.#spawn();
    for (const pending of this.#exported.values()) pending.reject(new Error("the compiler crashed"));
    this.#exported.clear();
    if (this.#newest && this.#newest.id > this.#reported) this.#worker.postMessage(this.#newest);
  }

  compile(source: string, onStatus: (status: CompilerStatus, request: number) => void): number {
    const request: CompileRequest = { kind: "compile", id: ++this.#latest, source };
    this.#callbacks.set(request.id, onStatus);
    this.#newest = request;
    this.#worker.postMessage(request);
    return request.id;
  }

  /**
   * Compiles `source` as written (not instrumented for the editor) into a
   * file: PDF, SVG, or PNG at `pixelPerPt`. Rejects with the first error.
   */
  export(source: string, format: ExportFormat, pixelPerPt = 2): Promise<Uint8Array> {
    const request: ExportRequest = { kind: "export", id: ++this.#exports, source, format, pixelPerPt };
    return new Promise((resolve, reject) => {
      this.#exported.set(request.id, { resolve, reject });
      this.#worker.postMessage(request);
    });
  }

  dispose(): void {
    this.#worker.terminate();
  }
}
