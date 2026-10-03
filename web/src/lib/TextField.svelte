<script lang="ts">
  // A text argument: its markup, plus size, color, bold and italic. Editing
  // the text only touches what's between the brackets; styling rewrites the
  // `text(...)` / `strong` / `emph` around it.
  import ColorInput from "./ColorInput.svelte";
  import { isBold, isItalic, lengthText, textFill, textSize, withBody, withStyle, type TextChange, type TextStyle } from "./props";

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
  </div>
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
  .toggle {
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
  .toggle.on {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
