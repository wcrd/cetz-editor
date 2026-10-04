<script lang="ts">
  let { open = $bindable(false) }: { open?: boolean } = $props();

  let dialog: HTMLDialogElement;
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const cmd = isMac ? "⌘" : "Ctrl";
  const alt = isMac ? "⌥" : "Alt";

  $effect(() => {
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<!-- Clicking the backdrop (the dialog itself, outside its body) closes it. -->
<dialog bind:this={dialog} onclose={() => (open = false)} onclick={(e) => e.target === dialog && dialog.close()} aria-labelledby="help-title">
  <div class="body">
    <header>
      <h2 id="help-title">Quick guide</h2>
      <button class="close" aria-label="Close" onclick={() => dialog.close()}>
        <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" /></svg>
      </button>
    </header>

    <section>
      <h3>Snapping and connecting</h3>
      <p>
        Line and arrow ends snap to shape anchors (<code>east</code>, <code>north-west</code>…), other lines' corners, and named
        points. A snapped end is <em>connected</em>: the code refers to the anchor or point, so the line follows when the shape
        moves. Dragging a selected line's handle onto a target connects it the same way.
      </p>
    </section>

    <section>
      <h3>Connectors</h3>
      <p>
        <kbd>E</kbd> draws an arrow from one shape to another, elbowed or straight; press <kbd>E</kbd> again to switch. Its ends
        name the shapes' anchors and an elbow's corners are worked out from them, so CeTZ re-routes it when a shape moves. The
        inspector's Route switches a selected one and re-picks its sides. Drag the dot on a selected elbow's middle segment to
        move where it crosses over.
      </p>
    </section>

    <section>
      <h3>Text</h3>
      <p>Double-click text to edit it where it is; <kbd>Enter</kbd> saves and <kbd>Esc</kbd> cancels. New text from <kbd>T</kbd> opens ready to type, and so does a node from <kbd>N</kbd>: a rounded rect with its text placed on it by name, so the text moves with it.</p>
    </section>
    <section>
      <h3>Smart guides</h3>
      <p>
        Moving, resizing or drawing lines shapes up with the others: edges and centres within a few pixels snap together, and
        an orange guide shows the match. <kbd>{cmd}</kbd> turns this off with the rest of snapping.
      </p>
    </section>
    <section>
      <h3><kbd>.</kbd> Named point</h3>
      <p>
        Click to drop a point, then type its name. It's written once in the code and referenced by name everywhere it's used,
        so moving it moves every shape built on it. <kbd>⇧P</kbd> shows all points.
      </p>
    </section>

    <section>
      <h3><kbd>J</kbd> Join points</h3>
      <p>
        Click point by point to build a path. Clicking a named point, a line's corner, or a shape's anchor <em>shares</em> it
        rather than copying its coordinates, so shapes built this way stay stuck together: draw a second triangle on two
        corners of the first and they share an edge.
      </p>
      <ul>
        <li>Click the first point to close the shape; <kbd>Enter</kbd> or double-click leaves it open; <kbd>Esc</kbd> cancels.</li>
        <li>Select an open line, press <kbd>J</kbd>, and click one of its ends to keep extending it.</li>
        <li><kbd>K</kbd> works the same way but draws a smooth curve through the points (<code>catmull</code>).</li>
        <li>With a line or curve selected, <kbd>{cmd}</kbd>-click it to add a point, or <kbd>{cmd}</kbd>-click one of its points to remove it.</li>
        <li><kbd>Q</kbd> marks an angle: click the corner, then a point on each side. <kbd>B</kbd> draws a brace between two points.</li>
      </ul>
    </section>

    <section>
      <h3><kbd>G</kbd> Polygon · <kbd>S</kbd> Star · <kbd>U</kbd> Arc</h3>
      <p>
        All three start at the centre. A polygon's drag ends on a corner, and a star's on an outer point, setting its size and rotation. An arc's drag ends where
        it starts (or click the centre, then the start); move to sweep it either way round, and click to finish. Radii snap to
        the grid and angles to 15°. Drag the square on a selected circle, polygon, star or arc to change its radius.
      </p>
    </section>
    <section>
      <h3>Rotating</h3>
      <p>
        Drag the knob above a selected shape; hold <kbd>⇧</kbd> for 15° steps. Polygons, stars and text set their <code>angle</code>; anything else is wrapped in
        a <code>scope</code> that starts with a <code>rotate</code>, which keeps its name usable outside, and later turns edit
        that rotation. Back at 0°, the scope goes away. The knob off the bottom-right corner of a group, scope or your own function scales the same way, into the same scope; shapes resize with their own handles.
      </p>
    </section>
    <section>
      <h3>Modifiers</h3>
      <dl>
        <dt><kbd>{cmd}</kbd></dt>
        <dd>No snapping, while drawing or dragging: off the grid, past anchors and points.</dd>
        <dt><kbd>{alt}</kbd></dt>
        <dd>
          Don't share. With <kbd>J</kbd>, a click lands on the point but writes plain coordinates. When dragging a shared
          point's handle, or moving shapes that use shared points, it detaches them instead of moving the point.
        </dd>
        <dt><kbd>⇧</kbd></dt>
        <dd>Lock lines, joined segments and rotation to 15° steps.</dd>
        <dt><kbd>Space</kbd></dt>
        <dd>Hold and drag to pan.</dd>
      </dl>
    </section>

    <p class="foot">Hover a toolbar button for its shortcut. <kbd>?</kbd> opens this guide.</p>
  </div>
</dialog>

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    font-size: 13px;
    line-height: 1.5;
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.3);
  }
  .body {
    padding: 14px 18px 16px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 13px;
  }
  section {
    margin-top: 14px;
  }
  p,
  ul,
  dl {
    margin: 0;
  }
  ul {
    padding-left: 18px;
    margin-top: 4px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 10px;
  }
  dd {
    margin: 0;
  }
  kbd,
  code {
    font-family: ui-monospace, monospace;
    font-size: 11.5px;
    padding: 0 4px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }
  kbd {
    border: 1px solid var(--border);
  }
  .foot {
    margin-top: 16px;
    color: var(--muted);
  }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .close:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .close svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
</style>
