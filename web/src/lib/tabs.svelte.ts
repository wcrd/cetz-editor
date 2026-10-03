// The open documents, one editor per tab. They share a compiler worker and
// the toolbar's view settings; everything else (source, undo history, view,
// selection, file link) belongs to its tab.

import { TypstCompiler } from "./compiler";
import { Editor, Prefs } from "./editor.svelte";
import { NEW_DOCUMENT } from "./files";

export class Tabs {
  editors = $state<Editor[]>([]);
  active = $state<Editor>() as Editor;
  prefs = new Prefs();
  compiler = new TypstCompiler();
  /** Registered by the app on each new editor: show the code panel. */
  openCode?: () => void;

  /**
   * Opens a document in a new tab: shown, right after the active one, or
   * (when not `activate`) in the background at the end.
   */
  open(source: string, fileName: string, activate = true): Editor {
    const editor = new Editor(source, this.compiler, this.prefs);
    editor.fileName = fileName;
    editor.openCode = () => this.openCode?.();
    const at = activate ? this.editors.indexOf(this.active) : -1;
    this.editors.splice(at < 0 ? this.editors.length : at + 1, 0, editor);
    if (activate || !this.active) this.active = editor;
    return editor;
  }

  openNew(): Editor {
    const editor = this.open(NEW_DOCUMENT, "untitled.typ");
    editor.savedSource = "";
    return editor;
  }

  /** Whether a tab is a new document nobody has touched, so opening a file can replace it. */
  isPristine(editor: Editor): boolean {
    return !editor.fileLinked && editor.fileName === "untitled.typ" && editor.source === NEW_DOCUMENT;
  }

  /** Closes a tab, asking first if it has unsaved changes. Returns whether it closed. */
  close(editor: Editor): boolean {
    if (editor.dirty && !this.isPristine(editor) && !confirm(`Discard unsaved changes to ${editor.fileName}?`)) return false;
    const at = this.editors.indexOf(editor);
    if (at < 0) return true;
    // Never leave the window empty: closing the last tab starts a new one.
    if (this.editors.length === 1) this.openNew();
    this.editors.splice(this.editors.indexOf(editor), 1);
    if (this.active === editor) this.active = this.editors[Math.min(at, this.editors.length - 1)];
    return true;
  }

  dispose() {
    this.compiler.dispose();
  }
}
