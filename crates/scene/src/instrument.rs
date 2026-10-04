//! Produces an instrumented copy of a source file in which every draw call in
//! a CeTZ canvas body or a drawing function is wrapped in `__cetz_probe(<offset>, <call>)`. The probe
//! (see `crates/compile/src/probe.typ`) records the geometry CeTZ computes for
//! that call, keyed by the call's byte offset in the original source.
//!
//! Rendering is unchanged: the probe only adds invisible metadata.

use typst_syntax::LinkedNode;

use crate::walk;

/// Path of the probe library in the compiler's virtual file system.
pub const PROBE_PATH: &str = "/__cetz-editor/probe.typ";

const PREAMBLE: &str = "#import \"/__cetz-editor/probe.typ\": __cetz_probe;";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrumented {
    pub text: String,
    /// `(original offset, inserted length)`, sorted by offset. Used to map
    /// positions in `text` back to the original source.
    pub insertions: Vec<(usize, usize)>,
    /// Original byte offsets of the wrapped calls (the probe ids).
    pub calls: Vec<usize>,
}

impl Instrumented {
    /// Maps a byte offset in the instrumented text to the original source.
    /// Offsets inside inserted text map to the insertion point.
    pub fn to_original(&self, offset: usize) -> usize {
        let mut shift = 0;
        for &(at, len) in &self.insertions {
            let start = at + shift;
            if offset < start {
                break;
            }
            if offset < start + len {
                return at;
            }
            shift += len;
        }
        offset - shift
    }
}

pub fn instrument(source: &str) -> Instrumented {
    let root = typst_syntax::parse(source);
    let mut spans = Vec::new();
    walk::for_each_canvas(&LinkedNode::new(&root), &mut |_, body| {
        walk::for_each_call(body, walk::Context::default(), &mut |call, _| spans.push(call.range()));
    });
    for function in walk::drawing_functions(&LinkedNode::new(&root)) {
        walk::for_each_function_call(&function, &mut |call, _| spans.push(call.range()));
    }
    spans.sort_unstable_by_key(|r| r.start);
    spans.dedup();

    let mut edits: Vec<(usize, String)> = vec![(0, PREAMBLE.to_string())];
    for span in &spans {
        edits.push((span.start, format!("__cetz_probe({}, ", span.start)));
        edits.push((span.end, ")".to_string()));
    }
    // Stable sort keeps a closing `)` before an opening wrap at the same offset.
    edits.sort_by_key(|(at, _)| *at);

    let mut text = String::with_capacity(source.len() + edits.len() * 24);
    let mut insertions = Vec::with_capacity(edits.len());
    let mut last = 0;
    for (at, insert) in edits {
        text.push_str(&source[last..at]);
        text.push_str(&insert);
        insertions.push((at, insert.len()));
        last = at;
    }
    text.push_str(&source[last..]);
    let calls = spans.iter().map(|r| r.start).collect();
    Instrumented { text, insertions, calls }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  let pts = (A: (0, 0))
  for (k, p) in pts { anchor(k, p) }
  line((0, 0), (1, 1), stroke: red)
  group(name: "g", { circle((0, 0)) })
  if true { rect((0, 0), (1, 1)) } else { content((0, 0), [x]) }
})
#line(length: 1cm)
"#;

    #[test]
    fn wraps_statement_calls_in_canvas_bodies() {
        let out = instrument(SRC);
        let wrapped: Vec<&str> = out
            .calls
            .iter()
            .map(|&at| SRC[at..].split('(').next().unwrap())
            .collect();
        assert_eq!(wrapped, ["anchor", "line", "group", "circle", "rect", "content"]);
        assert!(out.text.starts_with(PREAMBLE));
        let line = SRC.find("line((0").unwrap();
        assert!(out.text.contains(&format!("__cetz_probe({line}, line((0, 0), (1, 1), stroke: red))")));
        // The `let` binding and the `#line` outside the canvas are untouched.
        assert!(out.text.contains("let pts = (A: (0, 0))"));
        assert!(out.text.contains("\n#line(length: 1cm)"));
    }

    #[test]
    fn nested_wraps_close_correctly() {
        let out = instrument(SRC);
        let g = SRC.find("group(").unwrap();
        let c = SRC.find("circle(").unwrap();
        assert!(out.text.contains(&format!(
            "__cetz_probe({g}, group(name: \"g\", {{ __cetz_probe({c}, circle((0, 0))) }}))"
        )));
    }

    #[test]
    fn maps_offsets_back_to_the_original() {
        let out = instrument(SRC);
        // Each wrapped call's text sits right after its `__cetz_probe(<at>, `.
        for &at in &out.calls {
            let wrap = format!("__cetz_probe({at}, ");
            let pos = out.text.find(&wrap).unwrap() + wrap.len();
            assert_eq!(out.to_original(pos), at);
        }
        // Text inside an insertion maps to the insertion point.
        assert_eq!(out.to_original(5), 0);
        // The end of the file maps to the end of the original.
        assert_eq!(out.to_original(out.text.len()), SRC.len());
    }

    #[test]
    fn source_without_canvas_only_gets_preamble() {
        let out = instrument("= Hello\n#line()");
        assert!(out.calls.is_empty());
        assert_eq!(out.text, format!("{PREAMBLE}= Hello\n#line()"));
    }
}
