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

/** Compiles sources in a worker; only the latest request's result is reported. */
export class TypstCompiler {
  #worker = new Worker(new URL("./compiler.worker.ts", import.meta.url), { type: "module" });
  #latest = 0;

  constructor(onStatus: (status: CompilerStatus) => void) {
    this.#worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
      if (e.data.id !== this.#latest) return;
      const { id: _, ...status } = e.data;
      onStatus(status);
    };
  }

  compile(source: string): void {
    const request: WorkerRequest = { id: ++this.#latest, source };
    this.#worker.postMessage(request);
  }

  dispose(): void {
    this.#worker.terminate();
  }
}
