<script lang="ts">
  import fixture from "../../fixtures/zone_diagram.typ?raw";
  import { call_end, summarize } from "./lib/core";
  import { TypstCompiler, type CompilerStatus } from "./lib/compiler";
  import type { Probe } from "./lib/probe";
  import ProbeOverlay from "./lib/ProbeOverlay.svelte";

  let source = $state(fixture);
  let status = $state<CompilerStatus>({ kind: "loading" });
  // Keep showing the last good render while recompiling or on errors.
  let svg = $state<string>();
  let probes = $state<Probe[]>([]);
  let showGeometry = $state(true);
  let hovered = $state<number>();
  let textarea: HTMLTextAreaElement;

  let viewBox = $derived(svg && /viewBox="([^"]+)"/.exec(svg)?.[1]);

  let summary = $derived.by(() => {
    const s = summarize(source);
    const result = { nodes: s.nodes, errors: s.errors, lossless: s.lossless };
    s.free();
    return result;
  });

  const compiler = new TypstCompiler((next) => {
    status = next;
    if (next.kind === "done" && next.svg) {
      svg = next.svg;
      probes = next.probes ?? [];
    }
  });

  $effect(() => {
    const text = source;
    const timer = setTimeout(() => {
      if (status.kind === "done") status = { kind: "compiling" };
      compiler.compile(text);
    }, 150);
    return () => clearTimeout(timer);
  });

  $effect(() => () => compiler.dispose());

  // Probe ids are UTF-8 byte offsets; the textarea counts UTF-16 units.
  function utf16Index(text: string, byteOffset: number): number {
    return new TextDecoder().decode(new TextEncoder().encode(text).slice(0, byteOffset)).length;
  }

  /** Selects the draw call that produced a shape in the source pane. */
  function selectCall(id: number) {
    const end = call_end(source, id) ?? id;
    textarea.focus();
    textarea.setSelectionRange(utf16Index(source, id), utf16Index(source, end));
    // Scroll the selection into view.
    const line = source.slice(0, utf16Index(source, id)).split("\n").length - 1;
    const lineHeight = parseFloat(getComputedStyle(textarea).lineHeight);
    textarea.scrollTop = Math.max(0, line * lineHeight - textarea.clientHeight / 3);
  }

  let hoveredLabel = $derived.by(() => {
    if (hovered === undefined) return undefined;
    const probe = probes.find((p) => p.id === hovered);
    const prefix = source.slice(0, utf16Index(source, hovered));
    const line = prefix.split("\n").length;
    const call = source.slice(utf16Index(source, hovered)).match(/^[\w.-]+/)?.[0];
    return `${call}(…) at line ${line}${probe?.name ? ` · name: "${probe.name}"` : ""}`;
  });

  function plural(n: number, word: string) {
    return `${n} ${word}${n === 1 ? "" : "s"}`;
  }

  let compileLabel = $derived.by(() => {
    switch (status.kind) {
      case "loading":
        return "loading compiler…";
      case "compiling":
        return "compiling…";
      case "fetching":
        return `fetching ${status.packages.join(", ")}…`;
      case "done": {
        const errors = status.diagnostics.filter((d) => d.error).length;
        return errors ? plural(errors, "compile error") : `compiled in ${status.ms.toFixed(0)} ms`;
      }
    }
  });

  let diagnostics = $derived(status.kind === "done" ? status.diagnostics : []);
</script>

<main>
  <header>
    <h1>CeTZ Editor</h1>
    <p class="status">
      {summary.nodes} syntax nodes ·
      {summary.errors.length === 0 ? "no syntax errors" : plural(summary.errors.length, "syntax error")} ·
      {summary.lossless ? "lossless round-trip" : "round-trip mismatch"} ·
      <span class="compile" class:failed={diagnostics.some((d) => d.error)}>{compileLabel}</span>
    </p>
    <label class="toggle"><input type="checkbox" bind:checked={showGeometry} /> Show CeTZ geometry</label>
  </header>
  <div class="panes">
    <textarea bind:this={textarea} bind:value={source} spellcheck="false"></textarea>
    <section class="preview" class:stale={diagnostics.some((d) => d.error)}>
      {#if hoveredLabel}<span class="hovered">{hoveredLabel}</span>{/if}
      {#if svg}
        <div class="canvas">
          {@html svg}
          {#if showGeometry && viewBox}
            <ProbeOverlay
              {probes}
              {viewBox}
              {hovered}
              onhover={(id) => (hovered = id)}
              onselect={selectCall}
            />
          {/if}
        </div>
      {/if}
    </section>
  </div>
  {#if diagnostics.length > 0}
    <ul class="diagnostics">
      {#each diagnostics as d}
        <li class:error={d.error}>
          {#if d.line !== undefined}<span class="loc"
              >{d.file ?? "main.typ"}:{d.line + 1}:{(d.column ?? 0) + 1}</span
            >{/if}
          {d.message}
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, sans-serif;
    background: #fafafa;
    color: #222;
  }
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 16px;
    box-sizing: border-box;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 16px;
  }
  h1 {
    font-size: 1.1rem;
    margin: 0;
    white-space: nowrap;
  }
  .status {
    margin: 0;
    color: #666;
    font-size: 0.9rem;
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .compile.failed {
    color: #b00020;
  }
  .toggle {
    font-size: 0.9rem;
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .hovered {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 1;
    padding: 2px 6px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.75);
    color: white;
    font: 12px ui-monospace, monospace;
    pointer-events: none;
  }
  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 12px;
  }
  textarea,
  .preview {
    border: 1px solid #ddd;
    border-radius: 6px;
    background: white;
  }
  textarea {
    font: 13px/1.5 ui-monospace, monospace;
    padding: 12px;
    resize: none;
  }
  .preview {
    position: relative;
    overflow: auto;
    padding: 16px;
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .preview.stale {
    opacity: 0.5;
  }
  .canvas {
    position: relative;
    max-width: 100%;
  }
  .canvas > :global(svg:first-child) {
    display: block;
    max-width: 100%;
    height: auto;
  }
  .diagnostics {
    margin: 0;
    padding-left: 20px;
    font-size: 0.9rem;
    max-height: 20vh;
    overflow: auto;
  }
  .diagnostics .error {
    color: #b00020;
  }
  .loc {
    font-family: ui-monospace, monospace;
    margin-right: 8px;
  }
</style>
