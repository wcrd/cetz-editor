<script lang="ts">
  // A panel's draggable edge. Double-click resets; arrow keys resize when focused.
  import type { PanelSize } from "./panelSize.svelte";

  let { size, label }: { size: PanelSize; label: string } = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="resizer"
  class:left={size.sign < 0}
  class:dragging={size.dragging}
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  title="Drag to resize · double-click to reset"
  tabindex="0"
  onpointerdown={(e) => size.start(e)}
  onpointermove={(e) => size.move(e)}
  onpointerup={(e) => size.end(e)}
  onpointercancel={(e) => size.end(e)}
  ondblclick={() => size.reset()}
  onkeydown={(e) => size.keydown(e)}
></div>

<style>
  /* Straddles the panel's border so it's easy to grab. */
  .resizer {
    position: absolute;
    top: 0;
    bottom: 0;
    right: -4px;
    width: 7px;
    z-index: 5;
    cursor: col-resize;
    touch-action: none;
  }
  /* The inspector clips its overflow, so its handle stays just inside. */
  .resizer.left {
    right: auto;
    left: -1px;
  }
  .resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 1px;
    transition: background 0.1s;
  }
  .resizer.left::after {
    left: 0;
  }
  .resizer:hover::after,
  .resizer:focus-visible::after,
  .resizer.dragging::after {
    left: 2px;
    width: 3px;
    background: var(--accent);
  }
  .resizer.left:hover::after,
  .resizer.left:focus-visible::after,
  .resizer.left.dragging::after {
    left: 0;
  }
  .resizer:focus-visible {
    outline: none;
  }
  @media (max-width: 760px) {
    .resizer {
      display: none;
    }
  }
</style>
