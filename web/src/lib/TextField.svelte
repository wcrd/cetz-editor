<script lang="ts">
  // A text argument: its markup, plus size, color, bold and italic. Editing
  // the text only touches what's between the brackets; styling rewrites the
  // `text(...)` / `strong` / `emph` around it.
  import ColorInput from "./ColorInput.svelte";
  import PropRow from "./PropRow.svelte";
  import { applyChange, isBold, isItalic, lengthText, textFill, textProp, textSize, withBody, withStyle, type TextChange, type TextStyle } from "./props";
  import { TEXT_MORE } from "./schema";

  let {
    text,
    style,
    commit,
    onkeydown,
    textarea = $bindable(),
  }: {
    /** The argument's source, e.g. `text(5pt)[Floor]`. */
    text: string;
    style: TextStyle;
    commit: (text: string) => void;
    onkeydown: (e: KeyboardEvent) => void;
    /** For focusing the text from outside. */
    textarea?: HTMLInputElement | HTMLTextAreaElement;
  } = $props();

  function setBody(body: string) {
    if (body !== style.body) commit(withBody(text, style, body));
  }

  function set(change: TextChange) {
    const next = withStyle(style, change);
    if (next !== text) commit(next);
  }

  let open = $state(false);
  const setCount = $derived(TEXT_MORE.filter((o) => textProp(style, o.key)).length);
  const bold = $derived(isBold(style));
  const italic = $derived(isItalic(style));
</script>

<div class="text">
  <textarea bind:this={textarea} rows="2" value={style.body} onchange={(e) => setBody(e.currentTarget.value)} {onkeydown} spellcheck="false"></textarea>
  <div class="style">
    <input
      class="code size"
      title="Size"
      placeholder="size"
      value={textSize(style)}
      onchange={(e) => set({ size: lengthText(e.currentTarget.value) })}
      {onkeydown}
      spellcheck="false"
    />
    <ColorInput text={textFill(style)?.text ?? ""} code={false} title="Text color" commit={(t) => set({ fill: t })} />
    <button class="toggle bold" class:on={bold} aria-pressed={bold} title="Bold" onclick={() => set({ bold: !bold })}>B</button>
    <button class="toggle italic" class:on={italic} aria-pressed={italic} title="Italic" onclick={() => set({ italic: !italic })}>I</button>
    <button class="more" class:open aria-expanded={open} title="{open ? 'Hide' : 'Show'} font, weight and spacing" onclick={() => (open = !open)}>
      <span class="caret">▸</span> more{#if setCount > 0 && !open}<span class="count"> +{setCount}</span>{/if}
    </button>
  </div>
  {#if open}
    <div class="props">
      {#each TEXT_MORE as opt (opt.key)}
        {@const value = textProp(style, opt.key)?.text ?? ""}
        <PropRow {opt} texts={[value]} commit={(c) => set({ prop: opt.key, text: applyChange(c, value) })} {onkeydown} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .style {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .size {
    width: 64px;
  }
  .style .toggle {
    width: 24px;
    height: 22px;
    padding: 0;
  }
  .toggle.bold {
    font-weight: 700;
  }
  .toggle.italic {
    font-style: italic;
    font-family: Georgia, serif;
  }
  .style .more {
    margin-left: auto;
    border: none;
    background: none;
    padding: 0 2px;
    color: var(--muted);
    font-size: 11.5px;
  }
  .more .caret {
    display: inline-block;
    font-size: 10px;
    transition: transform 0.1s;
  }
  .more.open .caret {
    transform: rotate(90deg);
  }
  .count {
    font-size: 10.5px;
  }
  .props {
    --label-width: 64px;
    margin-top: 4px;
  }
  .toggle.on {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
