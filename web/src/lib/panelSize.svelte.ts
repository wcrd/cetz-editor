// A side panel's width, dragged by its edge (see Resizer.svelte) and
// remembered in this browser. Undefined means the layout's default.

export class PanelSize {
  width = $state<number>();
  dragging = $state(false);
  #drag?: { x: number; width: number };

  /** `sign` is 1 when the panel's dragged edge is its right one, -1 for its left. */
  constructor(
    readonly key: string,
    readonly sign: 1 | -1,
  ) {
    try {
      const width = Number(localStorage.getItem(key));
      if (width > 0) this.width = width;
    } catch {
      // Default width.
    }
  }

  /** For a `--…-width` custom property: unset keeps the layout's default. */
  get css(): string | null {
    return this.width === undefined ? null : `${Math.round(this.width)}px`;
  }

  start(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    this.#drag = { x: e.clientX, width: rendered(handle) };
    this.dragging = true;
  }

  move(e: PointerEvent) {
    if (this.#drag) this.width = this.#drag.width + this.sign * (e.clientX - this.#drag.x);
  }

  end(e: PointerEvent) {
    if (!this.#drag) return;
    this.#drag = undefined;
    this.dragging = false;
    // Keep what the layout allowed, not how far past its limit the pointer went.
    this.width = rendered(e.currentTarget as HTMLElement);
    this.#save();
  }

  keydown(e: KeyboardEvent) {
    const step = { ArrowLeft: -16, ArrowRight: 16 }[e.key];
    if (step === undefined) return;
    // Keep the arrows from also nudging the selected shapes.
    e.preventDefault();
    e.stopPropagation();
    const handle = e.currentTarget as HTMLElement;
    this.width = rendered(handle) + this.sign * (e.shiftKey ? step * 4 : step);
    requestAnimationFrame(() => {
      this.width = rendered(handle);
      this.#save();
    });
  }

  reset() {
    this.width = undefined;
    this.#save();
  }

  #save() {
    try {
      if (this.width === undefined) localStorage.removeItem(this.key);
      else localStorage.setItem(this.key, String(Math.round(this.width)));
    } catch {
      // Not remembered; that's fine.
    }
  }
}

/** The panel's width as laid out: the drag handle sits directly inside it. */
function rendered(handle: HTMLElement): number {
  return handle.parentElement!.getBoundingClientRect().width;
}
