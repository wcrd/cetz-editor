<script lang="ts">
  // The source pane. CodeMirror owns undo history for every change —
  // typing and canvas edits alike — so Cmd+Z works the same everywhere.
  import { onMount } from "svelte";
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    drawSelection,
    Decoration,
    ViewPlugin,
    type DecorationSet,
    type ViewUpdate,
  } from "@codemirror/view";
  import { Annotation, ChangeSet, EditorState, StateEffect, StateField, type Extension } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap, indentWithTab, undo, redo } from "@codemirror/commands";
  import { bracketMatching, indentOnInput } from "@codemirror/language";
  import { highlight } from "./core";
  import { OffsetIndex } from "./offsets";
  import type { Editor } from "./editor.svelte";
  import type { Patch, Range } from "./scene";

  let { editor }: { editor: Editor } = $props();
  let host: HTMLDivElement;
  let view: EditorView;

  /** Marks transactions that replace the whole document from outside. */
  const external = Annotation.define<boolean>();

  // Highlights for the selected and hovered calls, in UTF-16 offsets.
  const setMarks = StateEffect.define<{ selected: Range[]; hovered?: Range }>();
  const marks = StateField.define({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      for (const e of tr.effects) {
        if (e.is(setMarks)) {
          const ranges = [
            ...e.value.selected.map((r) => selectedMark.range(r.start, r.end)),
            ...(e.value.hovered ? [hoveredMark.range(e.value.hovered.start, e.value.hovered.end)] : []),
          ].filter((r) => r.from < r.to);
          deco = Decoration.set(ranges, true);
        }
      }
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  const selectedMark = Decoration.mark({ class: "cm-call-selected" });
  const hoveredMark = Decoration.mark({ class: "cm-call-hovered" });

  // Syntax colors from Typst's own highlighter, re-run on every edit.
  const syntaxMarks = new Map<string, Decoration>();
  function syntaxDecorations(text: string): DecorationSet {
    const index = new OffsetIndex(text);
    const spans: [number, number, string][] = JSON.parse(highlight(text));
    const ranges = spans
      .filter(([start, end]) => start < end)
      .map(([start, end, cls]) => {
        let mark = syntaxMarks.get(cls);
        if (!mark) syntaxMarks.set(cls, (mark = Decoration.mark({ class: cls })));
        return mark.range(index.toUtf16(start), index.toUtf16(end));
      });
    return Decoration.set(ranges, true);
  }
  const syntax = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = syntaxDecorations(view.state.doc.toString());
      }
      update(update: ViewUpdate) {
        if (update.docChanged) this.decorations = syntaxDecorations(update.state.doc.toString());
      }
    },
    { decorations: (v) => v.decorations },
  );

  function createState(text: string): EditorState {
    return EditorState.create({ doc: text, extensions: extensions() });
  }

  function extensions(): Extension[] {
    return [
      lineNumbers(),
      history(),
      drawSelection(),
      highlightActiveLine(),
      bracketMatching(),
      indentOnInput(),
      syntax,
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      marks,
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          // Convert the change set to byte-offset patches against the old text.
          const oldIndex = new OffsetIndex(update.startState.doc.toString());
          const patches: Patch[] = [];
          update.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
            patches.push({ start: oldIndex.toByte(fromA), end: oldIndex.toByte(toA), text: inserted.toString() });
          });
          editor.sourceChanged(update.state.doc.toString(), patches);
        }
        if (update.selectionSet && update.view.hasFocus && !update.transactions.some((t) => t.annotation(external))) {
          selectAtCursor(update.state.selection.main.head);
        }
      }),
    ];
  }

  /** Selects the innermost draw call containing the cursor. */
  function selectAtCursor(utf16: number) {
    const byte = editor.index.toByte(utf16);
    const containing = editor.calls.filter((c) => c.range.start <= byte && byte <= c.range.end);
    const innermost = containing.sort((a, b) => b.range.start - a.range.start)[0];
    if (innermost && !editor.selected.includes(innermost.id)) {
      editor.scope = innermost.parent ?? undefined;
      editor.selection = [innermost.id];
    }
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: createState(editor.source),
    });
    syncNormalized();
    editor.code = {
      applyPatches(steps) {
        // Compose the steps into one change set so they undo together.
        let changes: ChangeSet | undefined;
        for (const { patches, index } of steps) {
          const step = ChangeSet.of(
            patches.map((p) => ({ from: index.toUtf16(p.start), to: index.toUtf16(p.end), insert: p.text })),
            index.text.length,
          );
          changes = changes ? changes.compose(step) : step;
        }
        if (changes) view.dispatch({ changes, annotations: external.of(true) });
      },
      replaceAll(text: string, resetHistory: boolean) {
        if (resetHistory) {
          view.setState(createState(text));
          const doc = view.state.doc.toString();
          editor.sourceChanged(doc, [{ start: 0, end: editor.index.byteLength, text: doc }]);
        } else {
          view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, annotations: external.of(true) });
        }
      },
      undo: () => undo(view),
      redo: () => redo(view),
      refresh: () => view.requestMeasure(),
      select(range: Range) {
        const anchor = editor.index.toUtf16(range.start);
        const head = editor.index.toUtf16(range.end);
        view.dispatch({ selection: { anchor, head }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
        view.focus();
      },
      reveal(range: Range) {
        const from = editor.index.toUtf16(range.start);
        view.dispatch({ effects: EditorView.scrollIntoView(from, { y: "center" }) });
      },
    };
    return () => {
      editor.code = undefined;
      view.destroy();
    };
  });

  /** CodeMirror turns `\r\n` into `\n`; adopt its text so offsets agree. */
  function syncNormalized() {
    const doc = view.state.doc.toString();
    if (doc !== editor.source) editor.sourceChanged(doc, [{ start: 0, end: editor.index.byteLength, text: doc }]);
  }

  // Mirror selection/hover into highlights, and scroll to a new selection.
  let lastRevealed: number | undefined;
  $effect(() => {
    if (!view) return;
    const toRange = (id: number) => {
      const call = editor.callById.get(id);
      return call && { start: editor.index.toUtf16(call.range.start), end: editor.index.toUtf16(call.range.end) };
    };
    const selected = editor.selected.map(toRange).filter((r) => r !== undefined);
    const hovered = editor.hovered === undefined ? undefined : toRange(editor.hovered);
    view.dispatch({ effects: setMarks.of({ selected, hovered }) });

    const first = editor.selected[0];
    if (first !== undefined && first !== lastRevealed && !view.hasFocus) {
      const call = editor.callById.get(first);
      if (call) editor.code?.reveal(call.range);
    }
    lastRevealed = first;
  });
</script>

<div class="code" bind:this={host}></div>

<style>
  .code {
    height: 100%;
    overflow: hidden;
  }
  .code :global(.cm-editor) {
    height: 100%;
    font-size: 12.5px;
  }
  .code :global(.cm-scroller) {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    line-height: 1.55;
  }
  .code :global(.cm-gutters) {
    background: var(--panel);
    color: var(--muted);
    border-right: 1px solid var(--border);
  }
  .code :global(.cm-activeLine),
  .code :global(.cm-activeLineGutter) {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }
  .code :global(.cm-call-selected) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-radius: 2px;
  }
  .code :global(.cm-call-hovered) {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }
  /* Typst's highlight classes (typ-*), in a GitHub-like palette. */
  .code :global(.typ-comment) {
    color: var(--muted);
    font-style: italic;
  }
  .code :global(.typ-key),
  .code :global(.typ-marker),
  .code :global(.typ-term),
  .code :global(.typ-math-delim),
  .code :global(.typ-math-op) {
    color: light-dark(#cf222e, #ff7b72);
  }
  .code :global(.typ-func) {
    color: light-dark(#6639ba, #d2a8ff);
  }
  .code :global(.typ-pol) {
    color: light-dark(#953800, #ffa657);
  }
  .code :global(.typ-num),
  .code :global(.typ-escape) {
    color: light-dark(#0550ae, #79c0ff);
  }
  .code :global(.typ-str),
  .code :global(.typ-raw) {
    color: light-dark(#0a3069, #a5d6ff);
  }
  .code :global(.typ-label),
  .code :global(.typ-ref) {
    color: light-dark(#116329, #7ee787);
  }
  .code :global(.typ-op),
  .code :global(.typ-punct) {
    color: color-mix(in srgb, var(--text) 70%, transparent);
  }
  .code :global(.typ-heading),
  .code :global(.typ-strong) {
    font-weight: 600;
  }
  .code :global(.typ-emph) {
    font-style: italic;
  }
  .code :global(.typ-link) {
    text-decoration: underline;
  }
  .code :global(.cm-focused) {
    outline: none;
  }
</style>
