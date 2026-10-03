<script lang="ts">
  import fixture from "../../fixtures/zone_diagram.typ?raw";
  import Canvas from "./lib/Canvas.svelte";
  import CodeEditor from "./lib/CodeEditor.svelte";
  import Inspector from "./lib/Inspector.svelte";
  import Outline from "./lib/Outline.svelte";
  import GridStep from "./lib/GridStep.svelte";
  import Help from "./lib/Help.svelte";
  import type { Editor, Tool } from "./lib/editor.svelte";
  import type { ExportFormat } from "./lib/compiler";
  import { exportFile, loadSession, newDocument, openDropped, openFile, restoreSession, save, saveSession } from "./lib/files";
  import { PanelSize } from "./lib/panelSize.svelte";
  import Resizer from "./lib/Resizer.svelte";
  import { Tabs } from "./lib/tabs.svelte";

  // Restore the last session's tabs in this browser; otherwise start on the sample.
  const tabs = new Tabs();
  const session = loadSession();
  if (session) restoreSession(tabs, session);
  else tabs.open(fixture, "zone_diagram.typ");
  $effect(() => () => tabs.dispose());

  /** The document in the active tab. */
  const editor = $derived(tabs.active);

  $effect(() => {
    void tabs.active;
    for (const e of tabs.editors) {
      void e.source;
      void e.fileName;
      void e.savedSource;
      void e.handle;
    }
    const timer = setTimeout(() => saveSession(tabs), 400);
    return () => clearTimeout(timer);
  });

  function onbeforeunload(e: BeforeUnloadEvent) {
    saveSession(tabs);
    if (tabs.editors.some((t) => t.dirty)) e.preventDefault();
  }

  // A hidden code pane can't measure; re-measure the one a tab switch reveals.
  $effect(() => {
    const code = editor.code;
    if (code) requestAnimationFrame(() => code.refresh());
  });

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
  tabs.openCode = () => toggleCode(true);

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

  // Whether the inspector panel is open; remembered like the code panel.
  const SIDE_OPEN_KEY = "cetz-editor:side-open";
  let sideOpen = $state(readSideOpen());
  function readSideOpen(): boolean {
    try {
      return localStorage.getItem(SIDE_OPEN_KEY) !== "false";
    } catch {
      return true;
    }
  }
  function toggleSide(open = !sideOpen) {
    sideOpen = open;
    try {
      localStorage.setItem(SIDE_OPEN_KEY, String(open));
    } catch {
      // Not remembered; that's fine.
    }
  }
  const sideLabel = $derived(editor.selected.length > 0 ? "Inspector" : "Outline");

  // Panel widths, dragged by their edges; remembered in this browser.
  const codeSize = new PanelSize("cetz-editor:code-width", 1);
  const sideSize = new PanelSize("cetz-editor:side-width", -1);

  // Infinite canvas view; remembered in this browser like the code panel.
  const INFINITE_KEY = "cetz-editor:infinite";
  try {
    tabs.prefs.infinite = localStorage.getItem(INFINITE_KEY) === "true";
  } catch {
    // Default view.
  }
  function toggleInfinite() {
    editor.infinite = !editor.infinite;
    try {
      localStorage.setItem(INFINITE_KEY, String(editor.infinite));
    } catch {
      // Not remembered; that's fine.
    }
  }

  // Grid steps kept in the file, unless turned off in this browser.
  const GRID_IN_FILE_KEY = "cetz-editor:grid-in-file";
  try {
    tabs.prefs.gridInFile = localStorage.getItem(GRID_IN_FILE_KEY) !== "false";
  } catch {
    // Default.
  }
  function toggleGridInFile() {
    tabs.prefs.gridInFile = !tabs.prefs.gridInFile;
    editor.flash(tabs.prefs.gridInFile ? "Grid step: kept in the file, above each canvas" : "Grid step: kept in this browser");
    try {
      localStorage.setItem(GRID_IN_FILE_KEY, String(tabs.prefs.gridInFile));
    } catch {
      // Not remembered; that's fine.
    }
  }

  // Rulers, on unless turned off in this browser.
  const RULERS_KEY = "cetz-editor:rulers";
  try {
    tabs.prefs.showRulers = localStorage.getItem(RULERS_KEY) !== "false";
  } catch {
    // Default view.
  }
  function toggleRulers() {
    editor.showRulers = !editor.showRulers;
    try {
      localStorage.setItem(RULERS_KEY, String(editor.showRulers));
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
    const files = [...(e.dataTransfer?.files ?? [])];
    if (files.length === 0) return;
    e.preventDefault();
    void openDropped(tabs, files);
  }
  // Handy for poking at state from the console during development.
  if (import.meta.env.DEV) {
    Object.assign(window, { tabs });
    Object.defineProperty(window, "editor", { get: () => tabs.active, configurable: true });
  }

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
    { id: "polygon", label: "Polygon", key: "N", icon: "M8 5h8l4 7-4 7H8l-4-7z" },
    { id: "arc", label: "Arc", key: "U", icon: "M5 19A14 14 0 0 1 19 5" },
    { id: "text", label: "Text", key: "T", icon: "M5 6V4h14v2M12 4v16M9 20h6" },
    { id: "point", label: "Named point", key: ".", icon: "M12 3v4M12 17v4M3 12h4M17 12h4M12 9.5a2.5 2.5 0 1 0 0.01 0z" },
    { id: "join", label: "Join points", key: "J", icon: "M5 18L9 6l10 4-4 9zM5 18h.01M9 6h.01M19 10h.01M15 19h.01" },
    { id: "curve", label: "Curve through points", key: "K", icon: "M4 18C6 6 11 6 12 12s6 6 8-6M4 18h.01M12 12h.01M20 6h.01" },
    { id: "brace", label: "Brace", key: "B", icon: "M4 16c0-3 2-3 4-3h2c1 0 2-1 2-3 0 2 1 3 2 3h2c2 0 4 0 4 3" },
    { id: "angle", label: "Angle mark", key: "Q", icon: "M4 19h16M4 19L15 6M10 19a6 6 0 0 0-1.6-4.1" },
  ];

  // Typst's points are 1/72 in, so a PNG at N ppi has N / 72 pixels per point.
  const exports: { label: string; format: ExportFormat; ppi?: number }[] = [
    { label: "PDF", format: "pdf" },
    { label: "SVG", format: "svg" },
    { label: "PNG, 144 ppi", format: "png", ppi: 144 },
    { label: "PNG, 300 ppi", format: "png", ppi: 300 },
  ];
  let exportMenu = $state(false);
  let helpOpen = $state(false);
  let exportGroup = $state<HTMLElement>();

  function onpointerdown(e: PointerEvent) {
    if (exportMenu && !exportGroup?.contains(e.target as Node)) exportMenu = false;
  }

  function isEditingText(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    return !!el && (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || !!el.closest?.(".cm-editor"));
  }

  // ⌘C / ⌘X / ⌘V on the canvas copy shapes as CeTZ source; in the code
  // pane or a field they're left to the browser.
  function oncopy(e: ClipboardEvent, cut = false) {
    if (isEditingText(e.target) || helpOpen) return;
    const text = editor.copySelection(cut);
    if (text === undefined || !e.clipboardData) return;
    e.clipboardData.setData("text/plain", text);
    e.preventDefault();
  }
  function onpaste(e: ClipboardEvent) {
    if (isEditingText(e.target) || helpOpen) return;
    const text = e.clipboardData?.getData("text/plain");
    if (!text?.trim()) return;
    e.preventDefault();
    editor.paste(text);
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    const typing = isEditingText(e.target);
    // The help dialog handles its own Escape.
    if (helpOpen) return;

    if (exportMenu && e.key === "Escape") {
      e.preventDefault();
      exportMenu = false;
      return;
    }

    if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save(editor, e.shiftKey);
      return;
    }
    // Checked by key position: Option changes what ⌥⌘\ types.
    if (mod && e.altKey && e.code === "Backslash") {
      e.preventDefault();
      toggleSide();
      return;
    }
    if (mod && e.key === "\\") {
      e.preventDefault();
      toggleCode();
      return;
    }
    if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      void openFile(tabs);
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
    if (mod && e.key.toLowerCase() === "g") {
      e.preventDefault();
      if (e.shiftKey) editor.ungroupSelection();
      else editor.groupSelection();
      return;
    }
    // ⌘] / ⌘[ forward / backward; with Shift, to the front / back. By key
    // position: Shift turns ] into }.
    if (mod && (e.code === "BracketRight" || e.code === "BracketLeft")) {
      e.preventDefault();
      const forward = e.code === "BracketRight";
      editor.arrangeSelection(e.shiftKey ? (forward ? "front" : "back") : forward ? "forward" : "backward");
      return;
    }
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      editor.selection = editor.calls.filter((c) => editor.isSelectable(c)).map((c) => c.id);
      return;
    }
    if (mod) return;

    if (e.key === "Delete" || e.key === "Backspace") {
      editor.deleteSelection();
      e.preventDefault();
      return;
    }
    if (e.key === "Escape") {
      if (editor.draft) editor.endDrag();
      else if (editor.tool !== "select") editor.tool = "select";
      else if (editor.selected.length || editor.selectedPoints.length) {
        editor.selection = [];
        editor.pointSelection = [];
      } else editor.scope = undefined;
      return;
    }
    const arrows: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
    if (arrows[e.key] && (editor.selected.length || editor.selectedPoints.length)) {
      e.preventDefault();
      const step = e.shiftKey ? 1 : editor.gridStep;
      const [dx, dy] = arrows[e.key];
      if (editor.selected.length) editor.edit({ kind: "move", calls: editor.selected, dx: dx * step, dy: dy * step });
      else editor.edit({ kind: "move-points", points: editor.selectedPoints, dx: dx * step, dy: dy * step });
      return;
    }
    if (e.key === "?") {
      helpOpen = true;
      return;
    }
    // Before the tools: plain R is the rectangle.
    if (e.key === "R" && e.shiftKey) {
      toggleRulers();
      return;
    }
    const tool = tools.find((t) => t.key.toLowerCase() === e.key.toLowerCase());
    if (tool) {
      editor.tool = tool.id;
      return;
    }
    if (e.key === "g") editor.showGrid = !editor.showGrid;
    if (e.key === "p") editor.showPoints = !editor.showPoints;
    if (e.key === "i") toggleInfinite();
  }

  // Browsers only reveal a picked file's name, never its folder or path.
  function fileTooltip(editor: Editor): string {
    return [
      editor.fileName,
      editor.fileLinked
        ? "Linked to the file you opened: Save (⌘S) overwrites it."
        : "Not linked to a file on disk: Save asks where to save (or downloads).",
      `Line endings: ${editor.lineEnding === "\r\n" ? "CRLF" : "LF"}`,
      editor.dirty ? "Unsaved changes" : "No unsaved changes",
    ].join("\n");
  }

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
        return errors ? `${errors} error${errors === 1 ? "" : "s"}` : `compile: ${s.ms.toFixed(0)} ms`;
      }
    }
  });
</script>

<Help bind:open={helpOpen} />

<svelte:window {onkeydown} {onpointerdown} oncopy={(e) => oncopy(e)} oncut={(e) => oncopy(e, true)} {onpaste} {onbeforeunload} {ondragover} {ondrop} ondragleave={() => (dropping = false)} />

<div
  class="app"
  class:resizing={codeSize.dragging || sideSize.dragging}
  class:side-closed={!sideOpen}
  style:--code-width={codeSize.css}
  style:--side-width={sideSize.css}
>
  <header class="toolbar" class:code-open={codeOpen}>
    <div class="section code-section">
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
        <button title="New tab" aria-label="New file" onclick={() => newDocument(tabs)}>
          <svg viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6zM14 3v4h4M12 11v6M9 14h6" /></svg>
        </button>
        <button title="Open (⌘O)" aria-label="Open file" onclick={() => openFile(tabs)}>
          <svg viewBox="0 0 24 24"><path d="M3 7V5h7l2 2h9v12H3zM3 9h18" /></svg>
        </button>
        <button title="Save (⌘S)" aria-label="Save file" onclick={() => save(editor)}>
          <svg viewBox="0 0 24 24"><path d="M5 3h11l3 3v15H5zM8 3v5h7V3M8 21v-7h8v7" /></svg>
        </button>
        <div class="export" bind:this={exportGroup}>
          <button
            class:active={exportMenu}
            title="Export as PDF, SVG or PNG"
            aria-label="Export"
            aria-haspopup="menu"
            aria-expanded={exportMenu}
            onclick={() => (exportMenu = !exportMenu)}
          >
            <svg viewBox="0 0 24 24"><path d="M12 15V4M7.5 8.5L12 4l4.5 4.5M5 14v6h14v-6" /></svg>
          </button>
          {#if exportMenu}
            <div class="menu" role="menu">
              {#each exports as x (x.label)}
                <button
                  role="menuitem"
                  onclick={() => {
                    exportMenu = false;
                    void exportFile(editor, x.format, x.ppi && x.ppi / 72);
                  }}
                >
                  {x.label}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <div class="status" class:failed={editor.hasErrors} title={compileLabel}>{compileLabel}</div>
    </div>

    <div class="section canvas-section">
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
            <kbd aria-hidden="true">{t.key}</kbd>
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
        <button
          class:active={editor.infinite}
          title="Infinite canvas (I)"
          aria-label="Infinite canvas"
          aria-pressed={editor.infinite}
          onclick={toggleInfinite}
        >
          <svg viewBox="0 0 24 24"><path d="M12 12c-2-2.7-3.6-4-5.5-4a4 4 0 0 0 0 8c1.9 0 3.5-1.3 5.5-4s3.6-4 5.5-4a4 4 0 0 1 0 8c-1.9 0-3.5-1.3-5.5-4z" /></svg>
        </button>
        <button
          class:active={editor.showRulers}
          title="Rulers (⇧R)"
          aria-label="Rulers"
          aria-pressed={editor.showRulers}
          onclick={toggleRulers}
        >
          <svg viewBox="0 0 24 24"><path d="M3 8h18v8H3zM7 8v3M11 8v4M15 8v3M19 8v4" /></svg>
        </button>
        <label title="Show shared points (P)"><input type="checkbox" bind:checked={editor.showPoints} /> Points</label>
        <label title="Show grid (G)"><input type="checkbox" bind:checked={editor.showGrid} /> Grid</label>
        <label title="Snap to grid"><input type="checkbox" bind:checked={editor.snap} /> Snap</label>
        <GridStep {editor} />
        <button
          class:active={tabs.prefs.gridInFile}
          title={tabs.prefs.gridInFile
            ? "Grid step is kept in the file, as a // cetz-editor: grid comment above each canvas. Click to keep it in this browser instead"
            : "Grid step is kept in this browser. Click to keep it in the file, above each canvas"}
          aria-label="Keep grid step in the file"
          aria-pressed={tabs.prefs.gridInFile}
          onclick={toggleGridInFile}
        >
          <svg viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6M12 10v8" /></svg>
        </button>
      </div>

      <div class="group zoom">
        <button title="Zoom out (⌘−)" aria-label="Zoom out" onclick={() => editor.viewport?.zoomBy(0.8)}>−</button>
        <button class="pct" title="Fit (⌘0)" onclick={() => editor.viewport?.fit()}>{Math.round(editor.zoom * 100)}%</button>
        <button title="Zoom in (⌘+)" aria-label="Zoom in" onclick={() => editor.viewport?.zoomBy(1.25)}>+</button>
      </div>

      <div class="group">
        <button
          class:active={sideOpen}
          title="{sideOpen ? 'Hide' : 'Show'} {sideLabel.toLowerCase()} (⌥⌘\)"
          aria-label="Toggle inspector panel"
          aria-pressed={sideOpen}
          onclick={() => toggleSide()}
        >
          <svg viewBox="0 0 24 24"><path d="M4 5h16v14H4zM14 5v14M16 9h1.5M16 12h1.5" /></svg>
        </button>
        <button title="Quick guide (?)" aria-label="Quick guide" onclick={() => (helpOpen = true)}>
          <svg viewBox="0 0 24 24"><path d="M12 3a9 9 0 1 0 0.01 0zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6v.6M12 17h.01" /></svg>
        </button>
      </div>
    </div>
  </header>

  <div class="tabs" role="tablist" aria-label="Open files">
    <div class="tabs-label" aria-hidden="true">Tabs</div>
    {#each tabs.editors as t (t)}
      <div class="tab" class:active={t === editor} class:dirty={t.dirty && !tabs.isPristine(t)}>
        <button
          class="name"
          role="tab"
          aria-selected={t === editor}
          title={fileTooltip(t)}
          onclick={() => (tabs.active = t)}
          onauxclick={(e) => e.button === 1 && tabs.close(t)}
        >
          {t.fileName}
        </button>
        <button class="close" title="Close" aria-label="Close {t.fileName}" onclick={() => tabs.close(t)}>
          <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" /></svg>
          <span class="dot" title="Unsaved changes"></span>
        </button>
      </div>
    {/each}
    <button class="add" title="New tab" aria-label="New tab" onclick={() => newDocument(tabs)}>
      <svg viewBox="0 0 24 24"><path d="M12 6v12M6 12h12" /></svg>
    </button>
  </div>

  <main class:code-open={codeOpen}>
    <aside class="code-panel" aria-label="Code">
      {#if !codeOpen}
        <button class="rail" title="Show code (⌘\)" onclick={() => toggleCode(true)}>
          <svg viewBox="0 0 24 24"><path d="M9 6l6 6-6 6" /></svg>
          <span>Code</span>
        </button>
      {/if}
      <!-- Stays mounted while collapsed (and per tab): CodeMirror owns the undo history. -->
      <div class="code-body" class:collapsed={!codeOpen}>
        <div class="panel-header">
          <span>Code</span>
          <button title="Hide code (⌘\)" aria-label="Hide code panel" onclick={() => toggleCode(false)}>
            <svg viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6" /></svg>
          </button>
        </div>
        {#each tabs.editors as t (t)}
          <div class="code" class:hidden={t !== editor}><CodeEditor editor={t} /></div>
        {/each}
      </div>
      {#if codeOpen}<Resizer size={codeSize} label="Resize code panel" />{/if}
    </aside>
    <section class="stage">
      {#key editor}<Canvas {editor} />{/key}
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
    <aside class="side" aria-label={sideLabel}>
      {#if sideOpen}
        <Resizer size={sideSize} label="Resize {sideLabel.toLowerCase()}" />
        <div class="panel-header">
          <span>{sideLabel}</span>
          <button title="Hide {sideLabel.toLowerCase()} (⌥⌘\)" aria-label="Hide inspector panel" onclick={() => toggleSide(false)}>
            <svg viewBox="0 0 24 24"><path d="M9 6l6 6-6 6" /></svg>
          </button>
        </div>
        <div class="side-body">
          {#key editor}
            {#if editor.selected.length > 0}
              <Inspector {editor} />
            {:else}
              <Outline {editor} />
            {/if}
          {/key}
        </div>
      {:else}
        <button class="rail" title="Show {sideLabel.toLowerCase()} (⌥⌘\)" onclick={() => toggleSide(true)}>
          <svg viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6" /></svg>
          <span>{sideLabel}</span>
        </button>
      {/if}
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
    --point: #9b3fd6;
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
      --point: #c58bff;
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
    --point: #c58bff;
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
    /* Columns: the panels keep their widths and the canvas takes the rest,
       down to a minimum; past that the panels give way toward theirs. The
       code panel's minimum fits its toolbar section: the buttons plus
       "Loading compiler…". */
    --code-col: minmax(320px, var(--code-width, 30%));
    --stage-col: minmax(260px, 1fr);
    --side-col: minmax(260px, var(--side-width, 340px));
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .app.side-closed {
    --side-col: 28px;
  }
  /* The header shares the body's code column, so the file controls sit over
     the code and the drawing tools start at the canvas's left edge. */
  .toolbar {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    font-size: 13px;
    min-height: 34px;
  }
  .toolbar.code-open {
    grid-template-columns: var(--code-col) var(--stage-col) var(--side-col);
  }
  /* Too narrow to line up with the code column and still fit the canvas
     controls; let the file controls take only what they need. */
  @media (max-width: 1060px) {
    .toolbar.code-open {
      grid-template-columns: auto minmax(0, 1fr);
    }
    .toolbar.code-open .code-section {
      border-right: none;
    }
  }
  .section {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
    padding: 6px 12px;
  }
  .toolbar.code-open .code-section {
    border-right: 1px solid var(--border);
  }
  .canvas-section {
    grid-column: 2 / -1;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .toggles {
    margin-left: auto;
  }
  .group {
    display: flex;
    gap: 2px;
    align-items: center;
  }
  .toolbar button {
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
  .toolbar button:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .toolbar button.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  .toolbar button.tool {
    position: relative;
    width: 34px;
  }
  .tool kbd {
    position: absolute;
    right: 3px;
    bottom: 1px;
    font-family: inherit;
    font-size: 9px;
    font-weight: 500;
    line-height: 1;
    color: var(--muted);
    pointer-events: none;
  }
  .tool.active kbd {
    color: inherit;
  }
  .export {
    position: relative;
  }
  .export .menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    min-width: 140px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18);
  }
  .export .menu button {
    text-align: left;
    border: none;
    border-radius: 5px;
    padding: 0 10px;
    white-space: nowrap;
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
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .status.failed {
    color: #d33;
  }

  .tabs {
    display: flex;
    align-items: stretch;
    height: 32px;
    flex: none;
    padding-left: 6px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    font-size: 12.5px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 200px;
    min-width: 0;
    flex: none;
    border-right: 1px solid var(--border);
    color: var(--muted);
  }
  /* Labelled like the panel headers ("Code"), lined up with their text. */
  .tabs-label {
    display: flex;
    align-items: center;
    flex: none;
    padding: 0 12px 0 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .tabs-label + .tab {
    border-left: 1px solid var(--border);
  }
  .tab.active {
    background: var(--bg);
    color: var(--text);
    /* Merge into the content below. */
    margin-bottom: -1px;
  }
  .tab.active::before {
    content: "";
    position: absolute;
    inset: 0 0 auto;
    height: 2px;
    background: var(--accent);
  }
  .tabs button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: pointer;
  }
  .tab .name {
    min-width: 0;
    height: 100%;
    padding: 0 4px 0 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab .close,
  .tabs .add {
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0;
    margin-right: 6px;
    border-radius: 4px;
    color: var(--muted);
  }
  .tabs .add {
    width: 28px;
    height: 28px;
    margin: auto 4px;
  }
  .tab .close:hover,
  .tabs .add:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 9%, transparent);
  }
  .tabs svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    display: block;
    margin: auto;
  }
  /* The close button stays hidden until hover, except on the active tab;
     unsaved tabs show a dot in its place. */
  .tab:not(.active):not(:hover) .close svg {
    visibility: hidden;
  }
  .tab .dot {
    display: none;
    width: 8px;
    height: 8px;
    margin: auto;
    border-radius: 50%;
    background: var(--accent);
  }
  .tab.dirty:not(:hover) .close svg {
    display: none;
  }
  .tab.dirty:not(:hover) .dot {
    display: block;
  }

  main {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 28px var(--stage-col) var(--side-col);
    grid-template-areas: "code stage side";
  }
  main.code-open {
    grid-template-columns: var(--code-col) var(--stage-col) var(--side-col);
  }
  .stage {
    grid-area: stage;
    position: relative;
    min-width: 0;
  }
  .side {
    position: relative;
    display: flex;
    flex-direction: column;
    grid-area: side;
    border-left: 1px solid var(--border);
    background: var(--panel);
    min-height: 0;
    overflow: hidden;
  }
  .code-panel {
    position: relative;
    grid-area: code;
    min-width: 0;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--panel);
    display: flex;
    flex-direction: column;
  }
  .app.resizing {
    cursor: col-resize;
    user-select: none;
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
  .code.hidden {
    display: none;
  }
  .side-body {
    flex: 1;
    min-height: 0;
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
    main:not(.code-open) .code-panel,
    .side-closed .side {
      display: none;
    }
    .side-closed main {
      grid-template-rows: minmax(0, 1fr);
      grid-template-areas: "stage";
    }
    .side-closed main.code-open {
      grid-template-rows: minmax(0, 1fr) 35%;
      grid-template-areas: "stage" "code";
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
    .status {
      display: none;
    }
    .toolbar,
    .toolbar.code-open {
      display: flex;
      overflow-x: auto;
      gap: 8px;
      padding: 6px 12px;
    }
    .section {
      display: contents;
    }
    .group {
      flex: none;
    }
  }
</style>
