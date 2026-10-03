// Opening and saving .typ files. Uses the File System Access API where the
// browser has it (save writes back to the opened file); otherwise falls back
// to a file input and a download.

import type { Editor } from "./editor.svelte";
import type { Tabs } from "./tabs.svelte";

// Minimal typings; not every TS DOM lib includes these yet.
export interface FileHandle {
  name: string;
  getFile(): Promise<File>;
  createWritable(): Promise<{ write(data: string): Promise<void>; close(): Promise<void> }>;
  isSameEntry?(other: FileHandle): Promise<boolean>;
  queryPermission?(o: { mode: "readwrite" }): Promise<PermissionState>;
  requestPermission?(o: { mode: "readwrite" }): Promise<PermissionState>;
}
type PickerOptions = {
  suggestedName?: string;
  multiple?: boolean;
  types?: { description: string; accept: Record<string, string[]> }[];
};
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

const isAbort = (err: unknown) => err instanceof DOMException && err.name === "AbortError";

/**
 * Shows a file in a tab: the tab it's already open in, else a new one (taking
 * over the active tab if that's an untouched new document).
 */
async function show(tabs: Tabs, text: string, name: string, handle?: FileHandle): Promise<Editor> {
  if (handle) {
    for (const editor of tabs.editors) {
      if (editor.handle && (await editor.handle.isSameEntry?.(handle))) {
        tabs.active = editor;
        return editor;
      }
    }
  }
  const replaced = tabs.isPristine(tabs.active) ? tabs.active : undefined;
  const editor = tabs.open(text, name);
  editor.load(text, name);
  editor.handle = handle;
  editor.fileLinked = !!handle;
  if (replaced) tabs.close(replaced);
  return editor;
}

export function newDocument(tabs: Tabs) {
  tabs.openNew();
}

export async function openFile(tabs: Tabs) {
  if (fsWindow.showOpenFilePicker) {
    try {
      const picked = await fsWindow.showOpenFilePicker({ types: TYPES, multiple: true });
      for (const handle of picked) {
        const file = await handle.getFile();
        await show(tabs, await file.text(), file.name, handle);
      }
    } catch (err) {
      if (!isAbort(err)) tabs.active.flash(`Couldn't open the file: ${err}`);
    }
    return;
  }
  const input = Object.assign(document.createElement("input"), { type: "file", accept: ".typ,text/plain", multiple: true });
  input.onchange = () => void openDropped(tabs, [...(input.files ?? [])]);
  input.click();
}

/** Opens files dropped on the window (or picked without a handle), each in a tab. */
export async function openDropped(tabs: Tabs, files: File[]) {
  for (const file of files) await show(tabs, await file.text(), file.name);
}

export async function save(editor: Editor, saveAs = false) {
  const source = editor.source;
  // Write the file back with the line endings it was opened with.
  const text = editor.lineEnding === "\n" ? source : source.replace(/\n/g, editor.lineEnding);
  try {
    let handle = editor.handle;
    if (!handle || saveAs) {
      if (!fsWindow.showSaveFilePicker) return download(editor, text, source);
      handle = editor.handle = await fsWindow.showSaveFilePicker({ suggestedName: editor.fileName, types: TYPES });
    }
    const writable = await writableFor(handle);
    await writable.write(text);
    await writable.close();
    editor.fileName = handle.name;
    editor.savedSource = source;
    editor.fileLinked = true;
    editor.flash(`Saved ${handle.name}`);
  } catch (err) {
    if (!isAbort(err)) editor.flash(`Couldn't save: ${err}`);
  }
}

/**
 * Opens a file for writing. A handle restored from an earlier visit needs the
 * user's permission again; saving is a click or keypress, so the browser can ask.
 */
async function writableFor(handle: FileHandle) {
  const mode = { mode: "readwrite" } as const;
  if (handle.queryPermission && (await handle.queryPermission(mode)) !== "granted") {
    if ((await handle.requestPermission?.(mode)) !== "granted") {
      throw new DOMException(`Not allowed to write to ${handle.name}`, "NotAllowedError");
    }
  }
  return handle.createWritable();
}

function download(editor: Editor, text: string, source: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: editor.fileName });
  a.click();
  URL.revokeObjectURL(url);
  editor.savedSource = source;
}

// The open tabs are kept in this browser so a reload doesn't lose work.
// Best effort: storage can be unavailable or full.
const SESSION_KEY = "cetz-editor:tabs";
/** Where sessions were kept before tabs: a single document. */
const OLD_SESSION_KEY = "cetz-editor:session";

interface SavedTab {
  source: string;
  fileName: string;
  savedSource: string;
  lineEnding?: "\n" | "\r\n";
}

export function loadSession(): { tabs: SavedTab[]; active: number } | undefined {
  const isTab = (t: SavedTab | undefined) => !!t && typeof t.source === "string";
  try {
    const raw = localStorage.getItem(SESSION_KEY);
    const s = raw && JSON.parse(raw);
    if (s && Array.isArray(s.tabs) && s.tabs.length > 0 && s.tabs.every(isTab)) return s;
    const old = JSON.parse(localStorage.getItem(OLD_SESSION_KEY) ?? "null");
    return isTab(old) ? { tabs: [old], active: 0 } : undefined;
  } catch {
    return undefined;
  }
}

export function restoreSession(tabs: Tabs, session: { tabs: SavedTab[]; active: number }) {
  const editors = session.tabs.map((saved) => {
    const editor = tabs.open(saved.source, saved.fileName, false);
    editor.savedSource = saved.savedSource;
    editor.lineEnding = saved.lineEnding ?? "\n";
    return editor;
  });
  tabs.active = tabs.editors[session.active] ?? tabs.editors[0];
  // Relink the tabs to their files on disk. The handles are saved alongside
  // the tabs; a name check guards against the two stores getting out of step.
  relinked = loadHandles().then((handles) => {
    editors.forEach((editor, i) => {
      const handle = handles?.[i];
      if (!handle || handle.name !== editor.fileName || editor.handle) return;
      editor.handle = handle;
      editor.fileLinked = true;
    });
  });
}

export function saveSession(tabs: Tabs) {
  try {
    const saved: SavedTab[] = tabs.editors.map((editor) => ({
      source: editor.source,
      fileName: editor.fileName,
      savedSource: editor.savedSource,
      lineEnding: editor.lineEnding,
    }));
    localStorage.setItem(SESSION_KEY, JSON.stringify({ tabs: saved, active: tabs.editors.indexOf(tabs.active) }));
    localStorage.removeItem(OLD_SESSION_KEY);
  } catch {
    // Ignore: the session just won't be restored.
  }
  // Wait for the restore, or this could overwrite the handles it's reading.
  void relinked.then(() => saveHandles(tabs.editors.map((editor) => editor.handle)));
}

// File handles can't go in localStorage, but IndexedDB can store them: one
// list, in tab order, beside the session above. (Their own database: the
// package cache owns the "cetz-editor" one and its version.)
const HANDLES_KEY = "tab-handles";
let relinked: Promise<void> = Promise.resolve();

function handleStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("cetz-editor-files", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("handles");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const db = open.result;
      const tx = db.transaction("handles", mode);
      const request = run(tx.objectStore("handles"));
      tx.oncomplete = () => {
        db.close();
        resolve(request.result);
      };
      tx.onerror = tx.onabort = () => {
        db.close();
        reject(tx.error);
      };
    };
  });
}

async function loadHandles(): Promise<(FileHandle | undefined)[] | undefined> {
  try {
    return await handleStore("readonly", (store) => store.get(HANDLES_KEY));
  } catch {
    return undefined;
  }
}

async function saveHandles(handles: (FileHandle | undefined)[]) {
  try {
    await handleStore("readwrite", (store) => store.put(handles, HANDLES_KEY));
  } catch {
    // Ignore: the tabs just won't be relinked after a reload.
  }
}
