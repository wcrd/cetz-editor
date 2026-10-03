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

export type WorkerRequest = { id: number; source: string };

export type WorkerMessage =
  | { id: number; kind: "fetching"; packages: string[] }
  | ({ id: number; kind: "done" } & CompileResult);

export type CompilerStatus =
  | { kind: "loading" }
  | { kind: "compiling" }
  | { kind: "fetching"; packages: string[] }
  | ({ kind: "done" } & CompileResult);

/**
 * Compiles sources in a worker. Requests that arrive while the worker is busy
 * are coalesced; results are reported in order, never older than one already
 * reported, tagged with the request id `compile` returned.
 */
export class TypstCompiler {
  #worker = new Worker(new URL("./compiler.worker.ts", import.meta.url), { type: "module" });
  #latest = 0;
  #reported = 0;

  constructor(onStatus: (status: CompilerStatus, request: number) => void) {
    this.#worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
      const { id, ...status } = e.data;
      if (id < this.#reported) return;
      this.#reported = id;
      onStatus(status, id);
    };
  }

  compile(source: string): number {
    const request: WorkerRequest = { id: ++this.#latest, source };
    this.#worker.postMessage(request);
    return request.id;
  }

  dispose(): void {
    this.#worker.terminate();
  }
}
