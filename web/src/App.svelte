<script lang="ts">
  import fixture from "../../fixtures/zone_diagram.typ?raw";
  import { summarize } from "./lib/core";
  import { TypstCompiler, type CompilerStatus } from "./lib/compiler";

  let source = $state(fixture);
  let status = $state<CompilerStatus>({ kind: "loading" });
  // Keep showing the last good render while recompiling or on errors.
  let svg = $state<string>();

  let summary = $derived.by(() => {
    const s = summarize(source);
    const result = { nodes: s.nodes, errors: s.errors, lossless: s.lossless };
    s.free();
    return result;
  });

  const compiler = new TypstCompiler((next) => {
    status = next;
    if (next.kind === "done" && next.svg) svg = next.svg;
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
  </header>
  <div class="panes">
    <textarea bind:value={source} spellcheck="false"></textarea>
    <section class="preview" class:stale={diagnostics.some((d) => d.error)}>
      {#if svg}
        {@html svg}
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
  }
  .status {
    margin: 0;
    color: #666;
    font-size: 0.9rem;
  }
  .compile.failed {
    color: #b00020;
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
    overflow: auto;
    padding: 16px;
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .preview.stale {
    opacity: 0.5;
  }
  .preview :global(svg) {
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
