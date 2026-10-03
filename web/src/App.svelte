<script lang="ts">
  import fixture from "../../fixtures/zone_diagram.typ?raw";
  import { summarize } from "./lib/core";

  let source = $state(fixture);
  let summary = $derived.by(() => {
    const s = summarize(source);
    const result = { nodes: s.nodes, errors: s.errors, lossless: s.lossless };
    s.free();
    return result;
  });
</script>

<main>
  <header>
    <h1>CeTZ Editor</h1>
    <p class="status">
      {summary.nodes} syntax nodes ·
      {summary.errors.length === 0
        ? "no errors"
        : `${summary.errors.length} error${summary.errors.length === 1 ? "" : "s"}`} ·
      {summary.lossless ? "lossless round-trip" : "round-trip mismatch"}
    </p>
  </header>
  <textarea bind:value={source} spellcheck="false"></textarea>
  {#if summary.errors.length > 0}
    <ul class="errors">
      {#each summary.errors as error}<li>{error}</li>{/each}
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
  textarea {
    flex: 1;
    font: 13px/1.5 ui-monospace, monospace;
    padding: 12px;
    border: 1px solid #ddd;
    border-radius: 6px;
    resize: none;
  }
  .errors {
    margin: 0;
    color: #b00020;
    font-size: 0.9rem;
  }
</style>
