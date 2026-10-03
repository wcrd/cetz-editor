<script lang="ts">
  import { isVec, pathData, toPage, type Probe } from "./probe";

  interface Props {
    probes: Probe[];
    /** The Typst SVG's viewBox, so the overlay shares its coordinate system. */
    viewBox: string;
    hovered?: number;
    onhover: (id: number | undefined) => void;
    onselect: (id: number) => void;
  }

  let { probes, viewBox, hovered, onhover, onselect }: Props = $props();

  const palette = ["#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#0aa", "#f032e6", "#9a6324"];
  // Color by call so all instances of one call (e.g. in a loop) match.
  let colors = $derived(new Map([...new Set(probes.map((p) => p.id))].map((id, i) => [id, palette[i % palette.length]])));
</script>

<svg class="overlay" {viewBox} xmlns="http://www.w3.org/2000/svg">
  {#each probes as probe, i (i)}
    {@const color = colors.get(probe.id)}
    <g
      class="probe"
      class:hovered={hovered === probe.id}
      style:--color={color}
      role="button"
      tabindex="-1"
      onpointerenter={() => onhover(probe.id)}
      onpointerleave={() => onhover(undefined)}
      onclick={() => onselect(probe.id)}
      onkeydown={() => {}}
    >
      {#each probe.drawables as d}
        <path class="hit" d={pathData(probe, d)} />
        <path class="shape" d={pathData(probe, d)} />
      {/each}
      {#each Object.values(probe.anchors).filter(isVec) as anchor}
        {@const [x, y] = toPage(probe, anchor)}
        <circle cx={x} cy={y} r="1" />
      {/each}
    </g>
  {/each}
</svg>

<style>
  .overlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .probe {
    cursor: pointer;
    outline: none;
  }
  .hit {
    fill: transparent;
    stroke: transparent;
    stroke-width: 6;
  }
  .shape {
    fill: none;
    stroke: var(--color);
    stroke-width: 0.6;
    stroke-dasharray: 2 1;
    pointer-events: none;
  }
  circle {
    fill: var(--color);
    pointer-events: none;
  }
  .hovered .shape {
    stroke-width: 1.5;
    stroke-dasharray: none;
  }
  .hovered .hit {
    fill: color-mix(in srgb, var(--color) 15%, transparent);
  }
</style>
