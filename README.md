# CeTZ Editor

A canvas-style visual editor for [CeTZ](https://github.com/cetz-package/cetz)
diagrams that edits your hand-written Typst source in place. Everything runs
in the browser: the Typst compiler is compiled to WebAssembly, and there's no
backend.

- **Drag, draw and restyle on a canvas.** Every change is a minimal edit to
  the source, and every byte you didn't touch stays exactly as you wrote it.
- **Exact geometry from CeTZ itself.** Selection outlines, handles and anchors
  come from CeTZ's own computed shapes (see
  [docs/spikes/01-cetz-geometry.md](docs/spikes/01-cetz-geometry.md)), so
  they're right for rotations, scaling, groups, Bézier curves and math labels.
- **Code and canvas stay in sync.** Clicking a shape selects its call in the
  code, and putting the cursor in a call selects its shape. Undo covers edits
  from both.

## Getting started

Requires Rust (with the `wasm32-unknown-unknown` target), `wasm-bindgen-cli`
0.2.129, Node and pnpm. `typst` and LaTeX are only needed for `just render`.

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

```bash
just setup
```

```bash
just dev
```

| Recipe | |
|---|---|
| `just dev` | Build the WASM modules and start the dev server |
| `just build` | Production build into `web/dist` |
| `just test` | Rust tests and a Svelte type-check |
| `just render` | Render `fixtures/` to `generated/` with the Typst and LaTeX CLIs |

The first compile downloads CeTZ from packages.typst.org. After that it's
cached in the browser (see [todos/bundle-packages-vs-cdn.md](todos/bundle-packages-vs-cdn.md)).

## Using it

| | |
|---|---|
| **V** / **L** / **A** / **R** / **C** / **T** | Select, line, arrow, rectangle, circle, text tools |
| **.** | Named point: click to place one, then type its name |
| **J** | Join points: click points in turn; click the first again to close the shape, Enter or double-click to finish an open path, Esc to cancel |
| Click, Shift-click, drag on empty space | Select, add to selection, marquee select |
| Drag a shape | Move it (snaps its first point to the grid) |
| Drag a handle | Move that coordinate. Drop it on another shape's anchor to write `"name.anchor"` (the target is named if needed) |
| Double-click a group | Enter it to select its children (Esc to leave) |
| Arrow keys (Shift: 1 unit) | Nudge by one grid step |
| ⌘D / Delete | Duplicate / delete |
| ⌘Z / ⇧⌘Z | Undo / redo, for canvas and code edits alike |
| ⌘O / ⌘S / ⇧⌘S | Open / save / save as. Saving writes back to the opened file in Chromium; other browsers download it |
| Scroll / ⌘-scroll or pinch / Space-drag | Pan / zoom / pan |
| ⌘0, ⌘+, ⌘− | Fit, zoom in, zoom out |
| P | Show every shared point's marker |
| G | Toggle the grid |
| I | Infinite canvas: hide the page edge, extend the grid everywhere, and keep the drawing still as an auto-sized page grows |
| ⌘\ | Show or hide the code panel (left). The inspector stays on the right |

**Shared points.** Points defined once and used by name are linked to their
definition: `let A = (0, 0)`, entries of `let pts = (A: ..., B: ...)`, and
CeTZ anchors (`anchor("A", (0, 0))`, or a `for (k, p) in pts { anchor(k, p) }`
loop). Dragging a shared corner, or a shape that uses one, edits the definition,
so every shape using it follows. Hold ⌥ while dragging to detach just that use
into its own coordinate (dropping it on another anchor reconnects it). The
inspector shows each corner's point (`→ A (pts.A)`) with a Detach button, and
a Share button turns a literal coordinate into a shared `anchor(...)`. Hover a
point to highlight the shapes using it, and click its marker to select them.

**Drawing with named points.** While a drawing tool is active every named
point shows, and the start or end of a line, rect, circle or label snaps to
one and writes its name (`line("A", "G")`, `rect("E", "C")`) instead of
numbers. Handles snap to them the same way. The point tool (**.**) adds a
point to a dictionary an anchor loop names (`pts = (…, I: (3, 5.75))`), or
inserts `anchor("P1", (x, y))` before the first shape, and opens its name for
editing. Double-click a point's name in the outline to rename it everywhere
it's used by that name. The join tool (**J**) builds a path from points:
`line("A", "B", "G", close: true)`, snapping to named points and to other
shapes' anchors (an unnamed shape gets a name in the same undo step).

**Snapping modifiers.** Hold **⇧** to lock line, arrow and join segments to
15° steps (lengths still snap to the grid along horizontal and vertical ones).
Hold **⌘** (Ctrl elsewhere) to place without any snapping: no grid, points or
anchors. It works for every drawing tool, handle and move.

**Outline.** With nothing selected, the right panel shows the document:
**Variables** (every `let`; point variables and dictionaries of points are
editable here, other values show a summary and jump to the code) and
**Shapes** (every draw call). Calls in loops show how many shapes they drew
(`line ×4`) and expand into those repetitions, read-only and labelled by the
points they connect (`A → E`); hover or click one to highlight just it.

The inspector edits coordinates, text, the name and any named argument.
Common values get fields: text size, color, bold and italic for
`text(5pt)[...]`, `strong[...]` and the like; color, thickness and dash for
`stroke`; start and end marks; swatches for colors such as `luma(90%)` or
`orange.lighten(85%)`. Anything else, or any field after clicking `</>`, is
edited as a Typst expression. Your last session is restored on reload, and you can
drop a `.typ` file on the window to open it. Hover the file name to see whether
Save writes back to the opened file. Browsers only reveal a picked file's name,
not its folder, so the full path can't be shown.

## How it works

```
crates/scene/    Parses canvas bodies into draw calls with byte ranges; applies
                 edits (move, set coordinate/argument, connect, insert,
                 duplicate, delete) as minimal text patches; instruments
                 sources for the probe.
crates/compile/  In-memory Typst world: embedded fonts, packages added by the
                 host, SVG output, and probe queries. probe.typ wraps each
                 CeTZ element to record its drawables, anchors and transform.
crates/wasm/     Small bindings to `scene` used on the main thread.
crates/worker/   Bindings to `compile`, run in a Web Worker.
web/             Svelte 5 app: canvas, inspector, CodeMirror code pane.
```

The source text is the document. The scene model and the probe geometry are
both derived from it, keyed by each call's byte offset. An edit becomes text
patches, CodeMirror applies them as one undoable transaction, and everything
holding offsets is remapped through the patches until the next compile. While
you drag, the editor previews edits in the worker and shifts the overlay
natively, so it stays responsive when compiles lag.

See [docs/plan.md](docs/plan.md) for the design and milestones.

## Limitations

- Targets **Typst 0.15.1 and CeTZ 0.5.2**. The probe depends on CeTZ's
  internal element structures.
- Only literal coordinates like `(1, 2)` get handles and move. Computed
  coordinates, relative coordinates (`(rel: ...)`) and lengths with units show
  in the preview, but you edit them in the code.
- Shapes created in loops share one call. Moving one moves every iteration.
- Elements passed straight into wrappers like `on-layer(1, content(...))` lose
  their name and anchors, and children of `hide({...})` can't be selected.
- New shapes are appended to the end of the active canvas.
- The compiler module is about 38 MB (16 MB gzipped), mostly embedded fonts.
