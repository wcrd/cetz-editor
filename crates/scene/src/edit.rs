//! Edit commands from the editor, applied as minimal text patches so every
//! byte the user didn't touch stays exactly as written.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::scene::{self, Call, Scene, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Edit {
    /// Translate every literal coordinate of the calls (and of calls nested
    /// in them, like a group's children) by `(dx, dy)` canvas units.
    Move { calls: Vec<usize>, dx: f64, dy: f64 },
    /// Set one positional coordinate argument. Literal coordinates keep their
    /// formatting; anything else (an anchor name, an expression) is replaced.
    SetCoord { call: usize, arg: usize, x: f64, y: f64 },
    /// Replace an argument's value with raw Typst source text.
    SetArgText { call: usize, arg: usize, text: String },
    /// Set (`Some`) or remove (`None`) a named argument.
    SetNamed { call: usize, key: String, text: Option<String> },
    Delete { calls: Vec<usize> },
    /// Append a statement to the end of a canvas body (default: the first).
    Insert { canvas: Option<usize>, text: String },
    /// Point a coordinate argument at another call's anchor
    /// (`"name.anchor"`), naming that call first if it has no name.
    Connect { call: usize, arg: usize, target: usize, anchor: String },
    /// Copy the calls right after themselves, offset by `(dx, dy)`, without
    /// their `name:` so names stay unique.
    Duplicate { calls: Vec<usize>, dx: f64, dy: f64 },
}

#[derive(Debug, Clone, Serialize)]
pub struct EditResult {
    pub source: String,
    /// The changes made, in original-source offsets, sorted.
    pub patches: Vec<Patch>,
    /// Ids (offsets in the new source) of calls the edit created.
    pub created: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Patch {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Calls whose geometry is a transform, not a shape: moving a selection must
/// not shift them.
const TRANSFORMS: &[&str] = &["translate", "rotate", "scale", "set-origin", "set-transform", "set-viewport"];

pub fn apply(source: &str, edit: &Edit) -> Result<EditResult, String> {
    let scene = scene::parse(source);
    let mut patches = Vec::new();
    // (patch index, offset within its text) of each created call.
    let mut created = Vec::new();

    match edit {
        Edit::Move { calls, dx, dy } => {
            for call in with_descendants(&scene, &outermost(&scene, calls)?) {
                if !TRANSFORMS.contains(&base_name(&call.callee)) {
                    move_coords(call, *dx, *dy, &mut patches);
                }
            }
        }
        Edit::SetCoord { call, arg, x, y } => {
            let arg = find_arg(&scene, *call, *arg)?;
            match &arg.value {
                Value::Coord { x_range, y_range, .. } => {
                    patches.push(patch(x_range.clone(), num(*x)));
                    patches.push(patch(y_range.clone(), num(*y)));
                }
                _ => patches.push(patch(arg.value_range.clone(), format!("({}, {})", num(*x), num(*y)))),
            }
        }
        Edit::SetArgText { call, arg, text } => {
            let arg = find_arg(&scene, *call, *arg)?;
            patches.push(patch(arg.value_range.clone(), text.clone()));
        }
        Edit::SetNamed { call, key, text } => {
            let call = find_call(&scene, *call)?;
            set_named(call, key, text.as_deref(), &mut patches)?;
        }
        Edit::Delete { calls } => {
            for call in outermost(&scene, calls)? {
                patches.push(patch(statement_range(source, &call.range), String::new()));
            }
        }
        Edit::Insert { canvas, text } => {
            let canvas = match canvas {
                Some(id) => scene.canvases.iter().find(|c| c.id == *id),
                None => scene.canvases.first(),
            }
            .ok_or("no canvas to insert into")?;
            let (at, prefix, suffix) = insertion_point(source, canvas);
            created.push((patches.len(), prefix.len()));
            patches.push(patch(at..at, format!("{prefix}{text}{suffix}")));
        }
        Edit::Connect { call, arg, target, anchor } => {
            if call == target {
                return Err("can't connect a call to itself".into());
            }
            let arg = find_arg(&scene, *call, *arg)?;
            let target = find_call(&scene, *target)?;
            let name = match &target.name {
                Some(name) => name.clone(),
                None => {
                    let name = unique_name(&scene, base_name(&target.callee));
                    set_named(target, "name", Some(&format!("{name:?}")), &mut patches)?;
                    name
                }
            };
            patches.push(patch(arg.value_range.clone(), format!("{:?}", format!("{name}.{anchor}"))));
        }
        Edit::Duplicate { calls, dx, dy } => {
            for call in outermost(&scene, calls)? {
                let copy = duplicate_text(source, &scene, call, *dx, *dy)?;
                let line = statement_range(source, &call.range);
                let (sep, at) = if line.start < call.range.start || line.end > call.range.end {
                    // The call has its own line: copy it onto the next one.
                    (format!("\n{}", indent_at(source, call.range.start)), call.range.end)
                } else {
                    ("; ".to_string(), call.range.end)
                };
                created.push((patches.len(), sep.len()));
                patches.push(patch(at..at, format!("{sep}{copy}")));
            }
        }
    }

    finish(source, patches, created)
}

fn finish(source: &str, mut patches: Vec<Patch>, created: Vec<(usize, usize)>) -> Result<EditResult, String> {
    // Sort while remembering where each created marker's patch went.
    let mut order: Vec<usize> = (0..patches.len()).collect();
    order.sort_by_key(|&i| (patches[i].start, patches[i].end));
    let sorted: Vec<Patch> = order.iter().map(|&i| patches[i].clone()).collect();
    for pair in sorted.windows(2) {
        if pair[1].start < pair[0].end {
            return Err("edit produced overlapping changes".into());
        }
    }

    let mut out = String::with_capacity(source.len());
    let mut new_start = vec![0; patches.len()];
    let mut last = 0;
    for &i in &order {
        let p = &patches[i];
        out.push_str(&source[last..p.start]);
        new_start[i] = out.len();
        out.push_str(&p.text);
        last = p.end;
    }
    out.push_str(&source[last..]);
    let created = created.into_iter().map(|(i, within)| new_start[i] + within).collect();
    patches = sorted;
    Ok(EditResult { source: out, patches, created })
}

fn patch(range: Range<usize>, text: String) -> Patch {
    Patch { start: range.start, end: range.end, text }
}

fn find_call(scene: &Scene, id: usize) -> Result<&Call, String> {
    scene.call(id).ok_or_else(|| format!("no draw call at offset {id}"))
}

fn find_arg(scene: &Scene, call: usize, arg: usize) -> Result<&scene::Arg, String> {
    find_call(scene, call)?.args.get(arg).ok_or_else(|| format!("call at {call} has no argument {arg}"))
}

/// The selected calls, minus any nested inside another selected call.
fn outermost<'a>(scene: &'a Scene, ids: &[usize]) -> Result<Vec<&'a Call>, String> {
    let calls: Vec<&Call> = ids.iter().map(|&id| find_call(scene, id)).collect::<Result<_, _>>()?;
    let mut out: Vec<&Call> = calls
        .iter()
        .filter(|c| !calls.iter().any(|o| o.id != c.id && o.range.start <= c.range.start && c.range.end <= o.range.end))
        .copied()
        .collect();
    out.sort_by_key(|c| c.id);
    out.dedup_by_key(|c| c.id);
    Ok(out)
}

/// The calls plus every call nested inside them.
fn with_descendants<'a>(scene: &'a Scene, calls: &[&'a Call]) -> Vec<&'a Call> {
    scene
        .canvases
        .iter()
        .flat_map(|c| &c.calls)
        .filter(|k| calls.iter().any(|c| c.range.start <= k.range.start && k.range.end <= c.range.end))
        .collect()
}

fn base_name(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

fn move_coords(call: &Call, dx: f64, dy: f64, patches: &mut Vec<Patch>) {
    for (_, arg) in call.coords() {
        if let Value::Coord { x, y, x_range, y_range } = &arg.value {
            if dx != 0.0 {
                patches.push(patch(x_range.clone(), num(x + dx)));
            }
            if dy != 0.0 {
                patches.push(patch(y_range.clone(), num(y + dy)));
            }
        }
    }
}

fn set_named(call: &Call, key: &str, text: Option<&str>, patches: &mut Vec<Patch>) -> Result<(), String> {
    let in_parens: Vec<&scene::Arg> =
        call.args.iter().filter(|a| call.args_close.is_some_and(|close| a.range.end <= close)).collect();
    match (call.named(key), text) {
        (Some((_, arg)), Some(text)) => patches.push(patch(arg.value_range.clone(), text.to_string())),
        (Some((_, arg)), None) => {
            let i = in_parens.iter().position(|a| a.range == arg.range).ok_or("argument is outside the parentheses")?;
            let range = if i > 0 {
                in_parens[i - 1].range.end..arg.range.end
            } else if let Some(next) = in_parens.get(1) {
                arg.range.start..next.range.start
            } else {
                arg.range.clone()
            };
            patches.push(patch(range, String::new()));
        }
        (None, Some(text)) => {
            let close = call.args_close.ok_or("call has no argument list")?;
            match in_parens.last() {
                Some(last) => patches.push(patch(last.range.end..last.range.end, format!(", {key}: {text}"))),
                None => patches.push(patch(close..close, format!("{key}: {text}"))),
            }
        }
        (None, None) => {}
    }
    Ok(())
}

/// The range to delete for a statement: its whole line when it stands alone
/// on it, otherwise the call plus a following `;`.
fn statement_range(source: &str, call: &Range<usize>) -> Range<usize> {
    let line_start = source[..call.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[call.end..].find('\n').map_or(source.len(), |i| call.end + i);
    let before = &source[line_start..call.start];
    let after = &source[call.end..line_end];
    if before.trim().is_empty() && after.trim().trim_start_matches(';').trim().is_empty() {
        let end = if line_end < source.len() { line_end + 1 } else { line_end };
        return line_start..end;
    }
    let mut end = call.end;
    let rest = &source[end..];
    if let Some(semi) = rest.find(';').filter(|&i| rest[..i].trim().is_empty()) {
        end += semi + 1;
    }
    call.start..end
}

fn indent_at(source: &str, offset: usize) -> &str {
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line = &source[line_start..];
    &line[..line.len() - line.trim_start().len()]
}

/// Where to append a statement to a canvas body: `(offset, prefix, suffix)`.
fn insertion_point(source: &str, canvas: &scene::Canvas) -> (usize, String, String) {
    let close = canvas.body.end - 1;
    let starts_line = |offset: usize| {
        let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
        source[line_start..offset].trim().is_empty()
    };
    // Match the last top-level statement that starts its own line.
    let indent = canvas
        .calls
        .iter()
        .filter(|c| c.parent.is_none())
        .next_back()
        .filter(|c| starts_line(c.range.start))
        .map(|c| indent_at(source, c.range.start).to_string())
        .unwrap_or_else(|| format!("{}  ", indent_at(source, canvas.body.start)));
    let line_start = source[..close].rfind('\n').map_or(0, |i| i + 1);
    if line_start > canvas.body.start && source[line_start..close].trim().is_empty() {
        // `}` on its own line: insert a full line before it.
        (line_start, indent, "\n".into())
    } else {
        let close_indent = indent_at(source, canvas.body.start).to_string();
        (close, format!("\n{indent}"), format!("\n{close_indent}"))
    }
}

/// `base`, `base-2`, `base-3`, ... whichever isn't taken in the scene.
fn unique_name(scene: &Scene, base: &str) -> String {
    let taken = |n: &str| scene.canvases.iter().flat_map(|c| &c.calls).any(|c| c.name.as_deref() == Some(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}-{i}")).find(|n| !taken(n)).unwrap()
}

/// The call's text with its coordinates shifted and its `name:` removed.
fn duplicate_text(source: &str, scene: &Scene, call: &Call, dx: f64, dy: f64) -> Result<String, String> {
    let mut patches = Vec::new();
    for c in with_descendants(scene, &[call]) {
        if !TRANSFORMS.contains(&base_name(&c.callee)) {
            move_coords(c, dx, dy, &mut patches);
        }
        if c.id == call.id && c.name.is_some() {
            set_named(c, "name", None, &mut patches)?;
        }
    }
    let text = &source[call.range.clone()];
    let local: Vec<Patch> = patches
        .into_iter()
        .map(|p| Patch { start: p.start - call.range.start, end: p.end - call.range.start, text: p.text })
        .collect();
    Ok(finish(text, local, vec![])?.source)
}

/// Formats a number for source: at most 4 decimals, no trailing zeros.
pub fn num(value: f64) -> String {
    let rounded = (value * 1e4).round() / 1e4;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  line((0, 0), (1.5,-2), "a.east", stroke: red, name: "l")
  for x in (1, 2) { circle((x, 0)) }
  group(name: "g", {
    rotate(30deg)
    rect((0,0), (1,1))
  })
  content((2, 3), [Hi])
})
"#;

    fn id(callee: &str) -> usize {
        SRC.find(&format!("{callee}(")).unwrap()
    }

    fn run(edit: Edit) -> EditResult {
        apply(SRC, &edit).unwrap()
    }

    #[test]
    fn move_preserves_formatting() {
        let out = run(Edit::Move { calls: vec![id("line")], dx: 1.0, dy: 0.25 });
        assert!(out.source.contains(r#"line((1, 0.25), (2.5,-1.75), "a.east", stroke: red, name: "l")"#), "{}", out.source);
        assert_eq!(out.patches.len(), 4);
    }

    #[test]
    fn move_group_moves_children_but_not_transforms() {
        let out = run(Edit::Move { calls: vec![id("group"), id("rect")], dx: 2.0, dy: 0.0 });
        assert!(out.source.contains("rect((2,0), (3,1))"));
        assert!(out.source.contains("rotate(30deg)"));
    }

    #[test]
    fn set_coord_replaces_anchor_references() {
        let out = run(Edit::SetCoord { call: id("line"), arg: 2, x: 3.0, y: -1.0 });
        assert!(out.source.contains(r#"(1.5,-2), (3, -1), stroke"#));
        let out = run(Edit::SetCoord { call: id("line"), arg: 0, x: 0.5, y: 0.0 });
        assert!(out.source.contains("line((0.5, 0), (1.5,-2)"));
    }

    #[test]
    fn set_named_replaces_inserts_and_removes() {
        let line = id("line");
        let out = run(Edit::SetNamed { call: line, key: "stroke".into(), text: Some("blue".into()) });
        assert!(out.source.contains("stroke: blue, name"));
        let out = run(Edit::SetNamed { call: line, key: "fill".into(), text: Some("red".into()) });
        assert!(out.source.contains(r#"name: "l", fill: red)"#));
        let out = run(Edit::SetNamed { call: line, key: "stroke".into(), text: None });
        assert!(out.source.contains(r#""a.east", name: "l")"#));
        let out = run(Edit::SetNamed { call: id("content"), key: "anchor".into(), text: Some("\"west\"".into()) });
        assert!(out.source.contains(r#"content((2, 3), [Hi], anchor: "west")"#));
        let src = "#canvas({ circle() })";
        let out = apply(src, &Edit::SetNamed { call: 10, key: "radius".into(), text: Some("2".into()) }).unwrap();
        assert_eq!(out.source, "#canvas({ circle(radius: 2) })");
    }

    #[test]
    fn delete_removes_whole_lines_or_inline_calls() {
        let out = run(Edit::Delete { calls: vec![id("content")] });
        assert!(out.source.contains("  })\n})"));
        let out = run(Edit::Delete { calls: vec![id("circle")] });
        assert!(out.source.contains("for x in (1, 2) {  }"));
        let out = run(Edit::Delete { calls: vec![id("group"), id("rect")] });
        assert!(!out.source.contains("group") && !out.source.contains("rect"));
        assert!(out.source.contains("x, 0)) }\n  content"));
    }

    #[test]
    fn insert_appends_with_matching_indent() {
        let out = run(Edit::Insert { canvas: None, text: "circle((5, 5))".into() });
        assert!(out.source.contains("  content((2, 3), [Hi])\n  circle((5, 5))\n})"));
        assert_eq!(&out.source[out.created[0]..out.created[0] + 7], "circle(");

        let src = "#canvas({ line((0,0), (1,1)) })";
        let out = apply(src, &Edit::Insert { canvas: None, text: "rect((0,0), (1,1))".into() }).unwrap();
        assert_eq!(out.source, "#canvas({ line((0,0), (1,1)) \n  rect((0,0), (1,1))\n})");
        assert_eq!(crate::parse(&out.source).canvases[0].calls.len(), 2);
    }

    #[test]
    fn duplicate_copies_after_original_without_name() {
        let out = run(Edit::Duplicate { calls: vec![id("line")], dx: 1.0, dy: 1.0 });
        assert!(out.source.contains(
            "  line((0, 0), (1.5,-2), \"a.east\", stroke: red, name: \"l\")\n  line((1, 1), (2.5,-1), \"a.east\", stroke: red)\n"
        ), "{}", out.source);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 8], "line((1,");

        let out = run(Edit::Duplicate { calls: vec![id("circle")], dx: 0.0, dy: 1.0 });
        assert!(out.source.contains("{ circle((x, 0)); circle((x, 0)) }"));
    }

    /// Every edit on every call of every fixture keeps the file valid.
    #[test]
    fn edits_on_fixtures_keep_source_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "typ") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let scene = crate::parse(&src);
            let total = scene.canvases[0].calls.len();
            for call in scene.canvases.iter().flat_map(|c| &c.calls) {
                let edits = [
                    Edit::Move { calls: vec![call.id], dx: 0.5, dy: -0.5 },
                    Edit::SetNamed { call: call.id, key: "stroke".into(), text: Some("blue".into()) },
                    Edit::Duplicate { calls: vec![call.id], dx: 1.0, dy: 0.0 },
                    Edit::Delete { calls: vec![call.id] },
                ];
                for edit in edits {
                    let out = apply(&src, &edit).unwrap();
                    let summary = crate::summarize(&out.source);
                    assert!(summary.errors.is_empty(), "{edit:?} on {}: {:?}\n{}", call.callee, summary.errors, out.source);
                    let after = crate::parse(&out.source).canvases[0].calls.len();
                    match edit {
                        Edit::Delete { .. } => assert!(after < total),
                        Edit::Duplicate { .. } => assert!(after > total),
                        _ => assert_eq!(after, total),
                    }
                }
            }
        }
    }

    #[test]
    fn connect_names_the_target_when_needed() {
        let out = run(Edit::Connect { call: id("line"), arg: 0, target: id("content"), anchor: "west".into() });
        assert!(out.source.contains(r#"line("content.west", (1.5,-2)"#), "{}", out.source);
        assert!(out.source.contains(r#"content((2, 3), [Hi], name: "content")"#));
        let out = run(Edit::Connect { call: id("content"), arg: 0, target: id("group"), anchor: "north".into() });
        assert!(out.source.contains(r#"content("g.north", [Hi])"#));
        assert!(apply(SRC, &Edit::Connect { call: id("line"), arg: 0, target: id("line"), anchor: "end".into() }).is_err());
    }

    #[test]
    fn rejects_unknown_calls() {
        assert!(apply(SRC, &Edit::Delete { calls: vec![1] }).is_err());
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(num(2.0), "2");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(1.23456), "1.2346");
        assert_eq!(num(0.1 + 0.2), "0.3");
    }
}
