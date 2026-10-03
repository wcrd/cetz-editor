// Opening and saving .typ files. Uses the File System Access API where the
// browser has it (save writes back to the opened file); otherwise falls back
// to a file input and a download.

import type { Editor } from "./editor.svelte";

// Minimal typings; not every TS DOM lib includes these yet.
interface FileHandle {
  name: string;
  getFile(): Promise<File>;
  createWritable(): Promise<{ write(data: string): Promise<void>; close(): Promise<void> }>;
}
type PickerOptions = { suggestedName?: string; types?: { description: string; accept: Record<string, string[]> }[] };
type FsWindow = Window & {
  showOpenFilePicker?: (o?: PickerOptions) => Promise<FileHandle[]>;
  showSaveFilePicker?: (o?: PickerOptions) => Promise<FileHandle>;
};

const TYPES = [{ description: "Typst source", accept: { "text/plain": [".typ"] } }];
const fsWindow = window as FsWindow;

export const NEW_DOCUMENT = `#import "@preview/cetz:0.5.2": canvas, draw
#set page(width: auto, height: auto, margin: 8pt)

#canvas({
  import draw: *
  rect((0, 0), (4, 2), name: "box")
  content("box.center", [Hello])
})
`;

let handle: FileHandle | undefined;

function confirmDiscard(editor: Editor): boolean {
  return !editor.dirty || confirm(`Discard unsaved changes to ${editor.fileName}?`);
}

const isAbort = (err: unknown) => err instanceof DOMException && err.name === "AbortError";

export function newDocument(editor: Editor) {
  if (!confirmDiscard(editor)) return;
  handle = undefined;
  editor.load(NEW_DOCUMENT, "untitled.typ");
  editor.savedSource = "";
}

export async function openFile(editor: Editor) {
  if (!confirmDiscard(editor)) return;
  if (fsWindow.showOpenFilePicker) {
    try {
      const [picked] = await fsWindow.showOpenFilePicker({ types: TYPES });
      const file = await picked.getFile();
      editor.load(await file.text(), file.name);
      handle = picked;
    } catch (err) {
      if (!isAbort(err)) editor.flash(`Couldn't open the file: ${err}`);
    }
    return;
  }
  const input = Object.assign(document.createElement("input"), { type: "file", accept: ".typ,text/plain" });
  input.onchange = async () => {
    const file = input.files?.[0];
    if (file) await openDropped(editor, file, false);
  };
  input.click();
}

/** Opens a file dropped on the window (or picked without a handle). */
export async function openDropped(editor: Editor, file: File, confirm = true) {
  if (confirm && !confirmDiscard(editor)) return;
  handle = undefined;
  editor.load(await file.text(), file.name);
}

export async function save(editor: Editor, saveAs = false) {
  const source = editor.source;
  // Write the file back with the line endings it was opened with.
  const text = editor.lineEnding === "\n" ? source : source.replace(/\n/g, editor.lineEnding);
  try {
    if (!handle || saveAs) {
      if (!fsWindow.showSaveFilePicker) return download(editor, text, source);
      handle = await fsWindow.showSaveFilePicker({ suggestedName: editor.fileName, types: TYPES });
    }
    const writable = await handle.createWritable();
    await writable.write(text);
    await writable.close();
    editor.fileName = handle.name;
    editor.savedSource = source;
    editor.flash(`Saved ${handle.name}`);
  } catch (err) {
    if (!isAbort(err)) editor.flash(`Couldn't save: ${err}`);
  }
}

function download(editor: Editor, text: string, source: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: editor.fileName });
  a.click();
  URL.revokeObjectURL(url);
  editor.savedSource = source;
}

// The last session is kept in this browser so a reload doesn't lose work.
// Best effort: storage can be unavailable or full.
const SESSION_KEY = "cetz-editor:session";

export function loadSession():
  | { source: string; fileName: string; savedSource: string; lineEnding?: "\n" | "\r\n" }
  | undefined {
  try {
    const raw = localStorage.getItem(SESSION_KEY);
    const s = raw && JSON.parse(raw);
    return s && typeof s.source === "string" ? s : undefined;
  } catch {
    return undefined;
  }
}

export function saveSession(editor: Editor) {
  try {
    localStorage.setItem(
      SESSION_KEY,
      JSON.stringify({
        source: editor.source,
        fileName: editor.fileName,
        savedSource: editor.savedSource,
        lineEnding: editor.lineEnding,
      }),
    );
  } catch {
    // Ignore: the session just won't be restored.
  }
}
