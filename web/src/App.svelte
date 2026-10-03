<script lang="ts">
  import fixture from "../../fixtures/zone_diagram.typ?raw";
  import Canvas from "./lib/Canvas.svelte";
  import CodeEditor from "./lib/CodeEditor.svelte";
  import Inspector from "./lib/Inspector.svelte";
  import { Editor, type Tool } from "./lib/editor.svelte";
  import { loadSession, newDocument, openDropped, openFile, save, saveSession } from "./lib/files";

  // Restore the last session in this browser; otherwise start on the sample.
  const session = loadSession();
  const editor = new Editor(session?.source ?? fixture);
  if (session) {
    editor.fileName = session.fileName;
    editor.savedSource = session.savedSource;
    editor.lineEnding = session.lineEnding ?? "\n";
  } else {
    editor.fileName = "zone_diagram.typ";
  }
  $effect(() => () => editor.dispose());

  $effect(() => {
    void editor.source;
    void editor.fileName;
    void editor.savedSource;
    const timer = setTimeout(() => saveSession(editor), 400);
    return () => clearTimeout(timer);
  });

  function onbeforeunload(e: BeforeUnloadEvent) {
    saveSession(editor);
    if (editor.dirty) e.preventDefault();
  }

  // Whether the code panel is open; remembered in this browser.
  const CODE_OPEN_KEY = "cetz-editor:code-open";
  let codeOpen = $state(readCodeOpen());
  function readCodeOpen(): boolean {
    try {
      return localStorage.getItem(CODE_OPEN_KEY) !== "false";
    } catch {
      return true;
    }
  }
  function toggleCode(open = !codeOpen) {
    codeOpen = open;
    // CodeMirror can't measure while hidden; re-measure once it's visible.
    if (open) requestAnimationFrame(() => editor.code?.refresh());
    try {
      localStorage.setItem(CODE_OPEN_KEY, String(open));
    } catch {
      // Not remembered; that's fine.
    }
  }

  let dropping = $state(false);
  function ondragover(e: DragEvent) {
    if (e.dataTransfer?.types.includes("Files")) {
      e.preventDefault();
      dropping = true;
    }
  }
  function ondrop(e: DragEvent) {
    dropping = false;
    const file = e.dataTransfer?.files[0];
    if (!file) return;
    e.preventDefault();
    void openDropped(editor, file);
  }
  // Handy for poking at state from the console during development.
  if (import.meta.env.DEV) (window as unknown as { editor: Editor }).editor = editor;

  // Compile on every change (and every load, even of identical text):
  // immediately while dragging, debounced while typing.
  $effect(() => {
    void editor.source;
    void editor.loads;
    if (editor.draft) {
      editor.compile();
      return;
    }
    const timer = setTimeout(() => editor.compile(), 120);
    return () => clearTimeout(timer);
  });

  const tools: { id: Tool; label: string; key: string; icon: string }[] = [
    { id: "select", label: "Select", key: "V", icon: "M5 3l13 8-6 1.5L9 19z" },
    { id: "line", label: "Line", key: "L", icon: "M5 19L19 5" },
    { id: "arrow", label: "Arrow", key: "A", icon: "M5 19L19 5M11 5h8v8" },
    { id: "rect", label: "Rectangle", key: "R", icon: "M4 6h16v12H4z" },
    { id: "circle", label: "Circle", key: "C", icon: "M12 4a8 8 0 1 0 0.01 0z" },
    { id: "text", label: "Text", key: "T", icon: "M5 6V4h14v2M12 4v16M9 20h6" },
  ];

  function isEditingText(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    return !!el && (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || !!el.closest?.(".cm-editor"));
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    const typing = isEditingText(e.target);

    if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save(editor, e.shiftKey);
      return;
    }
    if (mod && e.key === "\\") {
      e.preventDefault();
      toggleCode();
      return;
    }
    if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      void openFile(editor);
      return;
    }
    if (mod && e.key.toLowerCase() === "z" && !typing) {
      e.preventDefault();
      if (e.shiftKey) editor.code?.redo();
      else editor.code?.undo();
      return;
    }
    if (mod && e.key.toLowerCase() === "y" && !typing) {
      e.preventDefault();
      editor.code?.redo();
      return;
    }
    if (mod && (e.key === "=" || e.key === "+")) {
      e.preventDefault();
      editor.viewport?.zoomBy(1.25);
      return;
    }
    if (mod && e.key === "-") {
      e.preventDefault();
      editor.viewport?.zoomBy(0.8);
      return;
    }
    if (mod && e.key === "0") {
      e.preventDefault();
      editor.viewport?.fit();
      return;
    }
    if (typing) return;

    if (mod && e.key.toLowerCase() === "d") {
      e.preventDefault();
      if (editor.selected.length) editor.edit({ kind: "duplicate", calls: editor.selected, dx: 0.5, dy: -0.5 });
      return;
    }
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      editor.selection = editor.calls.filter((c) => editor.isSelectable(c)).map((c) => c.id);
      return;
    }
    if (mod) return;

    if (e.key === "Delete" || e.key === "Backspace") {
      if (editor.selected.length) editor.edit({ kind: "delete", calls: editor.selected });
      e.preventDefault();
      return;
    }
    if (e.key === "Escape") {
      if (editor.draft) editor.endDrag();
      else if (editor.tool !== "select") editor.tool = "select";
      else if (editor.selected.length) editor.selection = [];
      else editor.scope = undefined;
      return;
    }
    const arrows: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
    if (arrows[e.key] && editor.selected.length) {
      e.preventDefault();
      const step = e.shiftKey ? 1 : editor.gridStep;
      const [dx, dy] = arrows[e.key];
      editor.edit({ kind: "move", calls: editor.selected, dx: dx * step, dy: dy * step });
      return;
    }
    const tool = tools.find((t) => t.key.toLowerCase() === e.key.toLowerCase());
    if (tool) {
      editor.tool = tool.id;
      return;
    }
    if (e.key === "g") editor.showGrid = !editor.showGrid;
  }

  // Browsers only reveal a picked file's name, never its folder or path.
  const fileTooltip = $derived(
    [
      editor.fileName,
      editor.fileLinked
        ? "Linked to the file you opened: Save (⌘S) overwrites it."
        : "Not linked to a file on disk: Save asks where to save (or downloads).",
      `Line endings: ${editor.lineEnding === "\r\n" ? "CRLF" : "LF"}`,
      editor.dirty ? "Unsaved changes" : "No unsaved changes",
    ].join("\n"),
  );

  const compileLabel = $derived.by(() => {
    const s = editor.status;
    switch (s.kind) {
      case "loading":
        return "Loading compiler…";
      case "compiling":
        return "Compiling…";
      case "fetching":
        return `Fetching ${s.packages.join(", ")}…`;
      case "done": {
        const errors = s.diagnostics.filter((d) => d.error).length;
        return errors ? `${errors} error${errors === 1 ? "" : "s"}` : `${s.ms.toFixed(0)} ms`;
      }
    }
  });
</script>

<svelte:window {onkeydown} {onbeforeunload} {ondragover} {ondrop} ondragleave={() => (dropping = false)} />

<div class="app">
  <header class="toolbar">
    <div class="group">
      <button
        class:active={codeOpen}
        title="{codeOpen ? 'Hide' : 'Show'} code (⌘\)"
        aria-label="Toggle code panel"
        aria-pressed={codeOpen}
        onclick={() => toggleCode()}
      >
        <svg viewBox="0 0 24 24"><path d="M4 5h16v14H4zM10 5v14M6.5 9h1.5M6.5 12h1.5" /></svg>
      </button>
    </div>

    <div class="group">
      <button title="New" aria-label="New file" onclick={() => newDocument(editor)}>
        <svg viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6zM14 3v4h4M12 11v6M9 14h6" /></svg>
      </button>
      <button title="Open (⌘O)" aria-label="Open file" onclick={() => openFile(editor)}>
        <svg viewBox="0 0 24 24"><path d="M3 7V5h7l2 2h9v12H3zM3 9h18" /></svg>
      </button>
      <button title="Save (⌘S)" aria-label="Save file" onclick={() => save(editor)}>
        <svg viewBox="0 0 24 24"><path d="M5 3h11l3 3v15H5zM8 3v5h7V3M8 21v-7h8v7" /></svg>
      </button>
    </div>

    <div class="title" title={fileTooltip}>
      <span class="file">{editor.fileName}</span>{#if editor.dirty}<span class="dirty" title="Unsaved changes">●</span>{/if}
    </div>

    <div class="group tools" role="toolbar" aria-label="Tools">
      {#each tools as t}
        <button
          class="tool"
          class:active={editor.tool === t.id}
          title="{t.label} ({t.key})"
          aria-label={t.label}
          aria-pressed={editor.tool === t.id}
          onclick={() => (editor.tool = t.id)}
        >
          <svg viewBox="0 0 24 24"><path d={t.icon} /></svg>
        </button>
      {/each}
    </div>

    <div class="group">
      <button title="Undo (⌘Z)" aria-label="Undo" onclick={() => editor.code?.undo()}>
        <svg viewBox="0 0 24 24"><path d="M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3" /></svg>
      </button>
      <button title="Redo (⇧⌘Z)" aria-label="Redo" onclick={() => editor.code?.redo()}>
        <svg viewBox="0 0 24 24"><path d="M15 14l5-5-5-5M20 9H10a6 6 0 0 0 0 12h3" /></svg>
      </button>
    </div>

    <div class="group toggles">
      <label title="Show grid (G)"><input type="checkbox" bind:checked={editor.showGrid} /> Grid</label>
      <label title="Snap to grid"><input type="checkbox" bind:checked={editor.snap} /> Snap</label>
      <select bind:value={editor.gridStep} title="Grid step (canvas units)">
        {#each [0.1, 0.25, 0.5, 1] as step}<option value={step}>{step}</option>{/each}
      </select>
    </div>

    <div class="group zoom">
      <button title="Zoom out (⌘−)" aria-label="Zoom out" onclick={() => editor.viewport?.zoomBy(0.8)}>−</button>
      <button class="pct" title="Fit (⌘0)" onclick={() => editor.viewport?.fit()}>{Math.round(editor.zoom * 100)}%</button>
      <button title="Zoom in (⌘+)" aria-label="Zoom in" onclick={() => editor.viewport?.zoomBy(1.25)}>+</button>
    </div>

    <div class="status" class:failed={editor.hasErrors}>{compileLabel}</div>
  </header>

  <main class:code-open={codeOpen}>
    <aside class="code-panel" aria-label="Code">
      {#if !codeOpen}
        <button class="rail" title="Show code (⌘\)" onclick={() => toggleCode(true)}>
          <svg viewBox="0 0 24 24"><path d="M9 6l6 6-6 6" /></svg>
          <span>Code</span>
        </button>
      {/if}
      <!-- Stays mounted while collapsed: CodeMirror owns the undo history. -->
      <div class="code-body" class:collapsed={!codeOpen}>
        <div class="panel-header">
          <span>Code</span>
          <button title="Hide code (⌘\)" aria-label="Hide code panel" onclick={() => toggleCode(false)}>
            <svg viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6" /></svg>
          </button>
        </div>
        <div class="code"><CodeEditor {editor} /></div>
      </div>
    </aside>
    <section class="stage">
      <Canvas {editor} />
      {#if editor.diagnostics.length > 0}
        <ul class="diagnostics">
          {#each editor.diagnostics as d}
            <li class:error={d.error}>
              {#if d.line !== undefined}<span class="loc">{d.file ?? editor.fileName}:{d.line + 1}:{(d.column ?? 0) + 1}</span>{/if}
              {d.message}
            </li>
          {/each}
        </ul>
      {/if}
      {#if editor.notice}<div class="notice" role="status">{editor.notice}</div>{/if}
      {#if dropping}<div class="drop">Drop a .typ file to open it</div>{/if}
    </section>
    <aside class="side" aria-label="Inspector">
      <Inspector {editor} />
    </aside>
  </main>
</div>

<style>
  :global(:root) {
    --bg: #ffffff;
    --panel: #f7f7f8;
    --canvas-bg: #ececef;
    --border: #e1e1e6;
    --text: #1d1d22;
    --muted: #6b6b76;
    --accent: #2f6fed;
    --snap: #e8590c;
    --grid: rgba(47, 111, 237, 0.12);
    --input-bg: #ffffff;
    --button-bg: #ffffff;
    color-scheme: light;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) {
      --bg: #1c1c20;
      --panel: #232328;
      --canvas-bg: #121215;
      --border: #34343b;
      --text: #e9e9ee;
      --muted: #9a9aa6;
      --accent: #6d9bff;
      --snap: #ff8a3d;
      --grid: rgba(109, 155, 255, 0.16);
      --input-bg: #18181c;
      --button-bg: #2a2a30;
      color-scheme: dark;
    }
  }
  :global(:root[data-theme="dark"]) {
    --bg: #1c1c20;
    --panel: #232328;
    --canvas-bg: #121215;
    --border: #34343b;
    --text: #e9e9ee;
    --muted: #9a9aa6;
    --accent: #6d9bff;
    --snap: #ff8a3d;
    --grid: rgba(109, 155, 255, 0.16);
    --input-bg: #18181c;
    --button-bg: #2a2a30;
    color-scheme: dark;
  }
  :global(body) {
    margin: 0;
    font-family: system-ui, -apple-system, sans-serif;
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    font-size: 13px;
    min-height: 34px;
  }
  .title {
    min-width: 0;
    max-width: 220px;
    display: flex;
    gap: 6px;
    align-items: baseline;
  }
  .file {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dirty {
    color: var(--accent);
    font-size: 10px;
  }
  .group {
    display: flex;
    gap: 2px;
    align-items: center;
  }
  .toolbar button,
  .toolbar select {
    font: inherit;
    color: inherit;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    height: 28px;
    min-width: 28px;
    padding: 0 6px;
    cursor: pointer;
  }
  .toolbar select {
    border-color: var(--border);
    background: var(--input-bg);
  }
  .toolbar button:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .toolbar button.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  .toolbar svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    display: block;
    margin: auto;
  }
  .toggles label {
    display: flex;
    align-items: center;
    gap: 3px;
    margin-right: 6px;
    color: var(--muted);
  }
  .pct {
    min-width: 52px !important;
    font-variant-numeric: tabular-nums;
  }
  .status {
    margin-left: auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .status.failed {
    color: #d33;
  }

  main {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 340px;
    grid-template-areas: "code stage side";
  }
  main.code-open {
    grid-template-columns: minmax(280px, 30%) minmax(0, 1fr) 340px;
  }
  .stage {
    grid-area: stage;
    position: relative;
    min-width: 0;
  }
  .side {
    grid-area: side;
    border-left: 1px solid var(--border);
    background: var(--panel);
    min-height: 0;
    overflow: hidden;
  }
  .code-panel {
    grid-area: code;
    min-width: 0;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--panel);
    display: flex;
    flex-direction: column;
  }
  .code-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .code-body.collapsed {
    display: none;
  }
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px 4px 12px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
  }
  .code {
    flex: 1;
    min-height: 0;
    background: var(--bg);
  }
  .panel-header button,
  .rail {
    font: inherit;
    color: var(--muted);
    background: none;
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }
  .panel-header button {
    width: 24px;
    height: 24px;
    padding: 0;
  }
  .panel-header button:hover,
  .rail:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .rail {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 10px 0;
    border-radius: 0;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .rail span {
    writing-mode: vertical-rl;
  }
  .panel-header svg,
  .rail svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    display: block;
  }
  .panel-header svg {
    margin: auto;
  }

  .diagnostics {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 12px;
    margin: 0;
    padding: 8px 12px 8px 28px;
    max-height: 30%;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.1);
    font-size: 12.5px;
  }
  .diagnostics .error {
    color: #d33;
  }
  .loc {
    font-family: ui-monospace, monospace;
    margin-right: 8px;
    color: var(--muted);
  }
  .drop {
    position: absolute;
    inset: 12px;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--accent);
    font-weight: 600;
    pointer-events: none;
  }
  .notice {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--text);
    color: var(--bg);
    padding: 6px 12px;
    border-radius: 6px;
    font-size: 12.5px;
  }

  @media (max-width: 760px) {
    main,
    main.code-open {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, 1fr) 35%;
      grid-template-areas: "stage" "side";
    }
    main.code-open {
      grid-template-rows: minmax(0, 1fr) 30% 30%;
      grid-template-areas: "stage" "side" "code";
    }
    main:not(.code-open) .code-panel {
      display: none;
    }
    .side {
      border-left: none;
      border-top: 1px solid var(--border);
    }
    .code-panel {
      border-right: none;
      border-top: 1px solid var(--border);
    }
    .toggles,
    .title,
    .status {
      display: none;
    }
    .toolbar {
      overflow-x: auto;
      gap: 8px;
    }
    .group {
      flex: none;
    }
  }
</style>
