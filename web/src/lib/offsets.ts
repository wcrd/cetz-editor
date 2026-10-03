// Converts between UTF-8 byte offsets (used by the Rust core and the probe)
// and UTF-16 offsets (used by JS strings and CodeMirror).

export class OffsetIndex {
  readonly text: string;
  // utf16 index -> byte offset, only built for non-ASCII text.
  #bytes?: Uint32Array;

  constructor(text: string) {
    this.text = text;
    if (!/^[\x00-\x7f]*$/.test(text)) {
      const bytes = new Uint32Array(text.length + 1);
      let b = 0;
      for (let i = 0; i < text.length; i++) {
        bytes[i] = b;
        const c = text.charCodeAt(i);
        if (c < 0x80) b += 1;
        else if (c < 0x800) b += 2;
        else if (c >= 0xd800 && c < 0xdc00) {
          // High surrogate: the pair is 4 bytes; give the low half the same offset.
          b += 4;
          bytes[++i] = b - 4;
        } else b += 3;
      }
      bytes[text.length] = b;
      this.#bytes = bytes;
    }
  }

  get byteLength(): number {
    return this.#bytes ? this.#bytes[this.text.length] : this.text.length;
  }

  toByte(utf16: number): number {
    return this.#bytes ? this.#bytes[Math.min(utf16, this.text.length)] : utf16;
  }

  toUtf16(byte: number): number {
    const bytes = this.#bytes;
    if (!bytes) return byte;
    let lo = 0;
    let hi = this.text.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (bytes[mid] < byte) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** The text between two byte offsets. */
  slice(start: number, end: number): string {
    return this.text.slice(this.toUtf16(start), this.toUtf16(end));
  }
}
