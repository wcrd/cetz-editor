//! Edit commands from the editor, applied as minimal text patches so every
//! byte the user didn't touch stays exactly as written.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use typst_syntax::{LinkedNode, SyntaxKind};

use crate::scene::{self, Call, Scene, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Edit {
    /// Translate every literal coordinate of the calls (and of calls nested
    /// in them, like a group's children) by `(dx, dy)` canvas units. Shared
    /// points they use move too (once each), unless `detach` is set: then
    /// those uses become literal coordinates in just these calls.
    Move {
        calls: Vec<usize>,
        dx: f64,
        dy: f64,
        #[serde(default)]
        detach: bool,
    },
    /// Move a shared point's definition; every use follows.
    SetPoint { point: usize, x: f64, y: f64 },
    /// Move several shared points' definitions by `(dx, dy)` canvas units.
    MovePoints { points: Vec<usize>, dx: f64, dy: f64 },
    /// Delete shared points' definitions (an `anchor(..)` call, a `let`, or a
    /// dictionary entry), and any `anchor("A", pts.A)` that only names one.
    /// Arguments that used them get the point's position as a literal
    /// coordinate, so the drawing doesn't change. Fails rather than leave a
    /// reference the editor can't rewrite (say, a name inside a loop's data).
    DeletePoints { points: Vec<usize> },
    /// Create a named point at `(x, y)`. It joins a points dictionary that an
    /// anchor loop names (`pts = (..., I: (x, y))`) when the canvas has one,
    /// else becomes `anchor("P1", (x, y))` before the first draw call.
    /// `created` holds the new point's id.
    AddPoint { canvas: Option<usize>, x: f64, y: f64, name: Option<String> },
    /// Rename a point's definition and every reference to it by that name.
    RenamePoint { point: usize, name: String },
    /// Turn a literal coordinate argument into a shared point: insert
    /// `anchor("name", (x, y))` before the call and use `"name"` instead.
    ExtractPoint { call: usize, arg: usize, name: Option<String> },
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
        Edit::Move { calls, dx, dy, detach } => {
            let moved = with_descendants(&scene, &outermost(&scene, calls)?);
            move_calls(&scene, &moved, *dx, *dy, *detach, &mut patches);
        }
        Edit::SetPoint { point, x, y } => {
            let p = scene.point(*point).ok_or_else(|| format!("no shared point at offset {point}"))?;
            patches.push(patch(p.x_range.clone(), num(*x)));
            patches.push(patch(p.y_range.clone(), num(*y)));
        }
        Edit::MovePoints { points, dx, dy } => {
            let mut ids = points.clone();
            ids.sort_unstable();
            ids.dedup();
            for id in ids {
                let p = scene.point(id).ok_or_else(|| format!("no shared point at offset {id}"))?;
                patches.push(patch(p.x_range.clone(), num(p.x + dx)));
                patches.push(patch(p.y_range.clone(), num(p.y + dy)));
            }
        }
        Edit::DeletePoints { points } => {
            delete_points(source, &scene, points, &mut patches)?;
            let result = finish(source, patches, created)?;
            check_unreferenced(source, &scene, points, &result)?;
            return Ok(result);
        }
        Edit::AddPoint { canvas, x, y, name } => {
            let canvas = match canvas {
                Some(id) => scene.canvases.iter().find(|c| c.id == *id),
                None => scene.canvases.first(),
            }
            .ok_or("no canvas to add a point to")?;
            let value = format!("({}, {})", num(*x), num(*y));
            if let Some((dict, keys)) = anchored_dict(source, &scene, canvas) {
                let name = match name {
                    Some(n) => valid_new_name(&scene, n)?,
                    None => next_key(&scene, &keys),
                };
                let prefix = format!(", {name}: ");
                created.push((patches.len(), prefix.len()));
                patches.push(patch(dict..dict, format!("{prefix}{value}")));
            } else {
                let name = match name {
                    Some(n) => valid_new_name(&scene, n)?,
                    None => unique_point_name(&scene),
                };
                let definition = format!("anchor({name:?}, ");
                match canvas.calls.iter().find(|c| c.parent.is_none()) {
                    Some(first) => {
                        let line_start = source[..first.range.start].rfind('\n').map_or(0, |i| i + 1);
                        let indent = &source[line_start..first.range.start];
                        if indent.trim().is_empty() {
                            created.push((patches.len(), indent.len() + definition.len()));
                            patches.push(patch(line_start..line_start, format!("{indent}{definition}{value})\n")));
                        } else {
                            created.push((patches.len(), definition.len()));
                            patches.push(patch(first.range.start..first.range.start, format!("{definition}{value}); ")));
                        }
                    }
                    None => {
                        let (at, prefix, suffix) = insertion_point(source, canvas);
                        created.push((patches.len(), prefix.len() + definition.len()));
                        patches.push(patch(at..at, format!("{prefix}{definition}{value}){suffix}")));
                    }
                }
            }
        }
        Edit::RenamePoint { point, name } => {
            let p = scene.point(*point).ok_or_else(|| format!("no shared point at offset {point}"))?;
            let range = p.name_range.clone().ok_or("this point has no name of its own to rename")?;
            let old = if p.name_quoted { p.anchors.first().cloned().unwrap_or_default() } else { source[range.clone()].to_string() };
            if *name == old {
                return finish(source, patches, created);
            }
            let name = valid_new_name(&scene, name)?;
            patches.push(patch(range.clone(), if p.name_quoted { format!("{name:?}") } else { name.clone() }));
            // Uses of the old name: `"old"` (an anchor it defines), `old`, or `var.old`.
            let renamed_anchor = p.name_quoted || p.anchors.iter().any(|a| *a == old);
            for arg in scene.canvases.iter().flat_map(|c| &c.calls).flat_map(|c| &c.args).filter(|a| a.point == Some(*point)) {
                // The definition itself: its value, and (for `anchor("C", ..)`) its name.
                if arg.value_range == p.range || arg.value_range == range {
                    continue;
                }
                let text = arg.text.as_str();
                let replacement = match &arg.value {
                    Value::Str { value } if renamed_anchor && *value == old => Some(format!("{name:?}")),
                    Value::Expr if text == old => Some(name.clone()),
                    Value::Expr if text.rsplit_once('.').is_some_and(|(_, field)| field == old) => {
                        text.rsplit_once('.').map(|(base, _)| format!("{base}.{name}"))
                    }
                    _ => None,
                };
                if let Some(r) = replacement {
                    patches.push(patch(arg.value_range.clone(), r));
                }
            }
        }
        Edit::ExtractPoint { call, arg, name } => {
            let call = find_call(&scene, *call)?;
            let arg = call.args.get(*arg).ok_or("no such argument")?;
            if !matches!(arg.value, Value::Coord { .. }) || arg.point.is_some() {
                return Err("only a literal coordinate can become a shared point".into());
            }
            let name = match name {
                Some(n) if !n.is_empty() && !n.contains('.') => n.clone(),
                Some(_) => return Err("a point name can't be empty or contain '.'".into()),
                None => unique_point_name(&scene),
            };
            let definition = format!("anchor({:?}, {})", name, arg.text);
            let line_start = source[..call.range.start].rfind('\n').map_or(0, |i| i + 1);
            if source[line_start..call.range.start].trim().is_empty() {
                let indent = &source[line_start..call.range.start];
                patches.push(patch(line_start..line_start, format!("{indent}{definition}\n")));
            } else {
                patches.push(patch(call.range.start..call.range.start, format!("{definition}; ")));
            }
            patches.push(patch(arg.value_range.clone(), format!("{name:?}")));
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

/// Moves the calls' own literal coordinates and the shared points they use.
fn move_calls(scene: &Scene, calls: &[&Call], dx: f64, dy: f64, detach: bool, patches: &mut Vec<Patch>) {
    let mut points = std::collections::BTreeSet::new();
    for call in calls.iter().filter(|c| !TRANSFORMS.contains(&base_name(&c.callee))) {
        for arg in call.args.iter().filter(|a| a.key.is_none()) {
            match (&arg.value, arg.point) {
                // A literal that is itself a shared point: move it once.
                (Value::Coord { .. }, Some(p)) if !detach => {
                    points.insert(p);
                }
                (Value::Coord { x, y, x_range, y_range }, _) => {
                    if dx != 0.0 {
                        patches.push(patch(x_range.clone(), num(x + dx)));
                    }
                    if dy != 0.0 {
                        patches.push(patch(y_range.clone(), num(y + dy)));
                    }
                }
                (_, Some(p)) if detach => {
                    if let Some(p) = scene.point(p) {
                        patches.push(patch(arg.value_range.clone(), format!("({}, {})", num(p.x + dx), num(p.y + dy))));
                    }
                }
                (_, Some(p)) => {
                    points.insert(p);
                }
                _ => {}
            }
        }
    }
    for p in points.into_iter().filter_map(|id| scene.point(id)) {
        if dx != 0.0 {
            patches.push(patch(p.x_range.clone(), num(p.x + dx)));
        }
        if dy != 0.0 {
            patches.push(patch(p.y_range.clone(), num(p.y + dy)));
        }
    }
}

/// Removes the points' definitions and inlines their uses (see `Edit::DeletePoints`).
fn delete_points(source: &str, scene: &Scene, ids: &[usize], patches: &mut Vec<Patch>) -> Result<(), String> {
    let doomed: Vec<&crate::Point> =
        ids.iter().map(|&id| scene.point(id).ok_or_else(|| format!("no shared point at offset {id}"))).collect::<Result<_, _>>()?;
    let is_doomed = |id: Option<usize>| id.is_some_and(|id| doomed.iter().any(|p| p.id == id));
    let calls: Vec<&Call> = scene.canvases.iter().flat_map(|c| &c.calls).collect();
    let mut removed: Vec<Range<usize>> = Vec::new();

    // `anchor("A", ..)` calls that define or name a doomed point go entirely.
    for call in calls.iter().filter(|c| base_name(&c.callee) == "anchor") {
        if call.args.get(1).is_some_and(|a| is_doomed(a.point) || doomed.iter().any(|p| p.range == a.value_range)) {
            removed.push(statement_range(source, &call.range));
        }
    }

    // Other definitions: a `let`, or entries of a dictionary.
    let root = typst_syntax::parse(source);
    let root = LinkedNode::new(&root);
    let mut dicts: Vec<(LinkedNode, Vec<Range<usize>>)> = Vec::new();
    for p in &doomed {
        if removed.iter().any(|r| r.start <= p.range.start && p.range.end <= r.end) {
            continue;
        }
        let parent = find_node(&root, &p.range, SyntaxKind::Array).and_then(|n| n.parent().cloned());
        match parent.as_ref().map(|n| n.kind()) {
            Some(SyntaxKind::LetBinding) => {
                let mut range = parent.unwrap().range();
                if source[..range.start].ends_with('#') {
                    range.start -= 1;
                }
                removed.push(statement_range(source, &range));
            }
            Some(SyntaxKind::Named) => {
                let entry = parent.unwrap();
                let dict = entry.parent().cloned().ok_or("dictionary entry outside a dictionary")?;
                match dicts.iter_mut().find(|(d, _)| d.range() == dict.range()) {
                    Some((_, entries)) => entries.push(entry.range()),
                    None => dicts.push((dict, vec![entry.range()])),
                }
            }
            Some(SyntaxKind::Array) => {
                return Err(format!("can't delete {}: it's an item of an array, and the items after it would shift", p.path));
            }
            _ => return Err(format!("can't find where {} is defined", p.path)),
        }
    }
    for (dict, doomed_entries) in &dicts {
        let entries: Vec<Range<usize>> = dict.children().filter(|c| c.kind() == SyntaxKind::Named).map(|c| c.range()).collect();
        let gone = |r: &Range<usize>| doomed_entries.contains(r);
        if entries.iter().all(gone) {
            removed.push(dict.range().start..dict.range().start);
            patches.push(patch(dict.range(), "(:)".into()));
            continue;
        }
        // Each run of removed entries takes the separator before it (or, at
        // the start, the one after it) so the commas stay right.
        let mut i = 0;
        while i < entries.len() {
            if !gone(&entries[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < entries.len() && gone(&entries[i]) {
                i += 1;
            }
            let range = if start > 0 { entries[start - 1].end..entries[i - 1].end } else { entries[start].start..entries[i].start };
            removed.push(range);
        }
    }
    for range in &removed {
        if !range.is_empty() {
            patches.push(patch(range.clone(), String::new()));
        }
    }

    // Uses left behind keep the position as a literal.
    let inside_removed = |r: &Range<usize>| removed.iter().any(|d| d.start <= r.start && r.end <= d.end && !d.is_empty())
        || dicts.iter().any(|(d, _)| d.range().start <= r.start && r.end <= d.range().end);
    for arg in calls.iter().flat_map(|c| &c.args) {
        let Some(p) = doomed.iter().find(|p| Some(p.id) == arg.point) else { continue };
        if arg.value_range == p.range || inside_removed(&arg.value_range) {
            continue;
        }
        patches.push(patch(arg.value_range.clone(), format!("({}, {})", num(p.x), num(p.y))));
    }
    Ok(())
}

/// Fails if the edited source still refers to a deleted point by name.
fn check_unreferenced(source: &str, scene: &Scene, ids: &[usize], result: &EditResult) -> Result<(), String> {
    let doomed: Vec<&crate::Point> = ids.iter().filter_map(|&id| scene.point(id)).collect();
    let root = typst_syntax::parse(&result.source);
    let mut found = None;
    find_reference(&LinkedNode::new(&root), &doomed, &mut found);
    match found {
        Some((name, offset)) => {
            // Report the line in the source as it stands, since the edit won't apply.
            let mut shift = 0isize;
            for p in &result.patches {
                if (p.start as isize + shift) + p.text.len() as isize > offset as isize {
                    break;
                }
                shift += p.text.len() as isize - (p.end - p.start) as isize;
            }
            let original = (offset as isize - shift) as usize;
            let line = source[..original].matches('\n').count() + 1;
            Err(format!("can't delete {name}: line {line} still refers to it in a way the editor can't rewrite"))
        }
        None => Ok(()),
    }
}

fn find_reference<'a>(node: &LinkedNode, doomed: &[&'a crate::Point], found: &mut Option<(&'a str, usize)>) {
    if found.is_some() {
        return;
    }
    let text = node.get().full_text();
    for p in doomed {
        let hit = match node.kind() {
            // An anchor name, alone or with one of its anchors: "A", "A.east".
            SyntaxKind::Str => node.get().cast::<typst_syntax::ast::Str>().is_some_and(|s| {
                let s = s.get();
                p.anchors.iter().any(|a| s == a.as_str() || s.strip_prefix(a.as_str()).is_some_and(|rest| rest.starts_with('.')))
            }),
            SyntaxKind::FieldAccess => p.path.contains('.') && text == p.path.as_str(),
            // A `let` name in use: not a key, a binding, or the field in `x.A`.
            SyntaxKind::Ident => !p.path.contains('.') && p.anchors.is_empty() && text == p.path.as_str() && !names_something(node),
            _ => false,
        };
        if hit {
            *found = Some((p.path.as_str(), node.offset()));
            return;
        }
    }
    for child in node.children() {
        find_reference(&child, doomed, found);
    }
}

/// Whether an identifier is a name being given (a key or a `let` binding) or
/// a field after a dot, rather than a variable being read.
fn names_something(ident: &LinkedNode) -> bool {
    let Some(parent) = ident.parent() else { return false };
    let first = parent.children().find(|c| c.kind() == SyntaxKind::Ident).is_some_and(|c| c.offset() == ident.offset());
    match parent.kind() {
        SyntaxKind::Named | SyntaxKind::LetBinding => first,
        SyntaxKind::FieldAccess => !first,
        _ => false,
    }
}

/// The end of the last entry of a points dictionary in this canvas that an
/// anchor loop turns into anchors, and the dictionary's keys.
fn anchored_dict(source: &str, scene: &Scene, canvas: &scene::Canvas) -> Option<(usize, Vec<String>)> {
    let var = scene.variables.iter().rev().find(|v| {
        v.kind == scene::VariableKind::Points
            && (v.canvas == Some(canvas.id) || v.canvas.is_none())
            && scene.points.iter().any(|p| p.path.starts_with(&format!("{}.", v.name)) && !p.anchors.is_empty())
    })?;
    let root = typst_syntax::parse(source);
    let dict = find_node(&LinkedNode::new(&root), &var.value_range, SyntaxKind::Dict)?;
    let entries: Vec<_> = dict.children().filter(|c| c.kind() == SyntaxKind::Named).collect();
    let keys = entries.iter().filter_map(|e| e.children().next()).map(|k| k.get().leaf_text().to_string()).collect();
    Some((entries.last()?.range().end, keys))
}

fn find_node<'a>(node: &LinkedNode<'a>, range: &Range<usize>, kind: SyntaxKind) -> Option<LinkedNode<'a>> {
    if node.range() == *range && node.kind() == kind {
        return Some(node.clone());
    }
    node.children().filter(|c| c.range().start <= range.start && range.end <= c.range().end).find_map(|c| find_node(&c, range, kind))
}

/// The next free key in the dictionary's style: `I` after `A`..`H`, else `P1`...
fn next_key(scene: &Scene, keys: &[String]) -> String {
    let letters = !keys.is_empty() && keys.iter().all(|k| k.len() == 1 && k.chars().all(|c| c.is_ascii_uppercase()));
    if letters {
        if let Some(free) = ('A'..='Z').map(String::from).find(|k| !keys.contains(k) && !name_taken(scene, k)) {
            return free;
        }
    }
    unique_point_name(scene)
}

fn name_taken(scene: &Scene, name: &str) -> bool {
    scene.points.iter().any(|p| p.anchors.iter().any(|a| a == name) || p.path == name)
        || scene.canvases.iter().flat_map(|c| &c.calls).any(|c| c.name.as_deref() == Some(name))
        || scene.variables.iter().any(|v| v.name == name)
}

/// Checks a name for a new or renamed point: an identifier nothing else uses.
fn valid_new_name(scene: &Scene, name: &str) -> Result<String, String> {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if !ok {
        return Err(format!("\"{name}\" isn't a valid name: use letters, digits, - and _"));
    }
    if name_taken(scene, name) {
        return Err(format!("\"{name}\" is already used"));
    }
    Ok(name.to_string())
}

/// `P1`, `P2`, ... whichever isn't already an anchor or element name.
fn unique_point_name(scene: &Scene) -> String {
    (1..).map(|i| format!("P{i}")).find(|n| !name_taken(scene, n)).unwrap()
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
/// The call's text with its coordinates shifted, shared points it uses
/// detached into literals (so the copy is independent), and its `name:`
/// removed.
fn duplicate_text(source: &str, scene: &Scene, call: &Call, dx: f64, dy: f64) -> Result<String, String> {
    let mut patches = Vec::new();
    move_calls(scene, &with_descendants(scene, &[call]), dx, dy, true, &mut patches);
    if call.name.is_some() {
        set_named(call, "name", None, &mut patches)?;
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
        let out = run(Edit::Move { calls: vec![id("line")], dx: 1.0, dy: 0.25, detach: false });
        assert!(out.source.contains(r#"line((1, 0.25), (2.5,-1.75), "a.east", stroke: red, name: "l")"#), "{}", out.source);
        assert_eq!(out.patches.len(), 4);
    }

    #[test]
    fn move_group_moves_children_but_not_transforms() {
        let out = run(Edit::Move { calls: vec![id("group"), id("rect")], dx: 2.0, dy: 0.0, detach: false });
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
            let count = |s: &Scene| s.canvases.iter().map(|c| c.calls.len()).sum::<usize>();
            let total = count(&scene);
            for call in scene.canvases.iter().flat_map(|c| &c.calls) {
                let edits = [
                    Edit::Move { calls: vec![call.id], dx: 0.5, dy: -0.5, detach: false },
                    Edit::Move { calls: vec![call.id], dx: 0.5, dy: -0.5, detach: true },
                    Edit::SetNamed { call: call.id, key: "stroke".into(), text: Some("blue".into()) },
                    Edit::Duplicate { calls: vec![call.id], dx: 1.0, dy: 0.0 },
                    Edit::Delete { calls: vec![call.id] },
                ];
                for edit in edits {
                    let out = apply(&src, &edit).unwrap();
                    let summary = crate::summarize(&out.source);
                    assert!(summary.errors.is_empty(), "{edit:?} on {}: {:?}\n{}", call.callee, summary.errors, out.source);
                    let after = count(&crate::parse(&out.source));
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

    const SHARED: &str = r#"#canvas({
  import draw: *
  let pts = (A: (0, 0), B: (2, 0))
  for (k, p) in pts { anchor(k, p) }
  line("A", "B")
  line("B", (3, 1))
  rect((0, 1), (1, 2))
})"#;

    fn shared_id(src: &str, needle: &str) -> usize {
        src.find(needle).unwrap()
    }

    #[test]
    fn move_moves_shared_points_once() {
        let calls = vec![shared_id(SHARED, r#"line("A""#), shared_id(SHARED, r#"line("B""#)];
        let out = apply(SHARED, &Edit::Move { calls, dx: 1.0, dy: 0.0, detach: false }).unwrap();
        assert!(out.source.contains("let pts = (A: (1, 0), B: (3, 0))"), "{}", out.source);
        assert!(out.source.contains(r#"line("A", "B")"#));
        assert!(out.source.contains(r#"line("B", (4, 1))"#));
    }

    #[test]
    fn move_with_detach_leaves_shared_points() {
        let call = shared_id(SHARED, r#"line("B""#);
        let out = apply(SHARED, &Edit::Move { calls: vec![call], dx: 1.0, dy: 1.0, detach: true }).unwrap();
        assert!(out.source.contains("let pts = (A: (0, 0), B: (2, 0))"));
        assert!(out.source.contains("line((3, 1), (4, 2))"), "{}", out.source);
    }

    #[test]
    fn set_point_edits_the_definition() {
        let scene = crate::parse(SHARED);
        let b = scene.points.iter().find(|p| p.path == "pts.B").unwrap().id;
        let out = apply(SHARED, &Edit::SetPoint { point: b, x: 2.5, y: -1.0 }).unwrap();
        assert!(out.source.contains("B: (2.5, -1)"));
    }

    #[test]
    fn extract_point_inserts_an_anchor() {
        let call = shared_id(SHARED, "rect(");
        let out = apply(SHARED, &Edit::ExtractPoint { call, arg: 1, name: None }).unwrap();
        assert!(out.source.contains("  anchor(\"P1\", (1, 2))\n  rect((0, 1), \"P1\")"), "{}", out.source);
        let scene = crate::parse(&out.source);
        let rect = scene.canvases[0].calls.iter().find(|c| c.callee == "rect").unwrap();
        assert_eq!(scene.point(rect.args[1].point.unwrap()).unwrap().path, "P1");
        // Already shared or not a literal: refused.
        assert!(apply(SHARED, &Edit::ExtractPoint { call: shared_id(SHARED, r#"line("A""#), arg: 0, name: None }).is_err());
    }

    #[test]
    fn add_point_joins_an_anchored_dictionary() {
        let out = apply(SHARED, &Edit::AddPoint { canvas: None, x: 7.0, y: 2.0, name: None }).unwrap();
        assert!(out.source.contains("let pts = (A: (0, 0), B: (2, 0), C: (7, 2))"), "{}", out.source);
        let scene = crate::parse(&out.source);
        let p = scene.point(out.created[0]).unwrap();
        assert_eq!((p.path.as_str(), p.anchors.clone()), ("pts.C", vec!["C".to_string()]));
    }

    #[test]
    fn add_point_inserts_an_anchor_otherwise() {
        let src = "#canvas({\n  import draw: *\n  rect((0, 0), (1, 1))\n})";
        let out = apply(src, &Edit::AddPoint { canvas: None, x: 1.5, y: 0.0, name: None }).unwrap();
        assert_eq!(out.source, "#canvas({\n  import draw: *\n  anchor(\"P1\", (1.5, 0))\n  rect((0, 0), (1, 1))\n})");
        assert_eq!(crate::parse(&out.source).point(out.created[0]).unwrap().path, "P1");
        let out = apply(src, &Edit::AddPoint { canvas: None, x: 0.0, y: 0.0, name: Some("Top".into()) }).unwrap();
        assert!(out.source.contains("anchor(\"Top\", (0, 0))"));
        assert!(apply(src, &Edit::AddPoint { canvas: None, x: 0.0, y: 0.0, name: Some("1x".into()) }).is_err());
    }

    #[test]
    fn rename_point_updates_references() {
        let scene = crate::parse(SHARED);
        let b = scene.points.iter().find(|p| p.path == "pts.B").unwrap().id;
        let out = apply(SHARED, &Edit::RenamePoint { point: b, name: "Right".into() }).unwrap();
        assert!(out.source.contains("let pts = (A: (0, 0), Right: (2, 0))"), "{}", out.source);
        assert!(out.source.contains(r#"line("A", "Right")"#));
        assert!(out.source.contains(r#"line("Right", (3, 1))"#));

        let src = "#let O = (0, 0)\n#canvas({\n  anchor(\"C\", (5, 5))\n  line(O, \"C\")\n})";
        let scene = crate::parse(src);
        let o = scene.points.iter().find(|p| p.path == "O").unwrap().id;
        let c = scene.points.iter().find(|p| p.path == "C").unwrap().id;
        let out = apply(src, &Edit::RenamePoint { point: o, name: "origin".into() }).unwrap();
        assert!(out.source.contains("#let origin = (0, 0)") && out.source.contains(r#"line(origin, "C")"#), "{}", out.source);
        let out = apply(src, &Edit::RenamePoint { point: c, name: "top".into() }).unwrap();
        assert!(out.source.contains(r#"anchor("top", (5, 5))"#) && out.source.contains(r#"line(O, "top")"#), "{}", out.source);
        assert!(apply(src, &Edit::RenamePoint { point: c, name: "O".into() }).is_err());
    }

    #[test]
    fn duplicate_detaches_shared_points() {
        let call = shared_id(SHARED, r#"line("A""#);
        let out = apply(SHARED, &Edit::Duplicate { calls: vec![call], dx: 0.5, dy: 0.0 }).unwrap();
        assert!(out.source.contains("line(\"A\", \"B\")\n  line((0.5, 0), (2.5, 0))"), "{}", out.source);
    }

    fn point_id(src: &str, path: &str) -> usize {
        crate::parse(src).points.iter().find(|p| p.path == path).unwrap().id
    }

    #[test]
    fn delete_points_inlines_anchor_uses() {
        let src = "#canvas({\n  import draw: *\n  anchor(\"P1\", (1, 2))\n  anchor(\"P2\", (3, 4))\n  line(\"P1\", \"P2\")\n})\n";
        let out = apply(src, &Edit::DeletePoints { points: vec![point_id(src, "P1")] }).unwrap();
        assert_eq!(out.source, "#canvas({\n  import draw: *\n  anchor(\"P2\", (3, 4))\n  line((1, 2), \"P2\")\n})\n");
        // Both at once: no anchors left, both uses literal.
        let both = vec![point_id(src, "P1"), point_id(src, "P2")];
        let out = apply(src, &Edit::DeletePoints { points: both }).unwrap();
        assert_eq!(out.source, "#canvas({\n  import draw: *\n  line((1, 2), (3, 4))\n})\n");
    }

    #[test]
    fn delete_points_removes_dictionary_entries() {
        let src = "#canvas({\n  import draw: *\n  let pts = (A: (0, 0), B: (1, 0), C: (1, 1))\n  for (k, p) in pts { anchor(k, p) }\n  line(\"A\", pts.B, \"C\")\n})\n";
        let del = |paths: &[&str]| apply(src, &Edit::DeletePoints { points: paths.iter().map(|p| point_id(src, p)).collect() }).unwrap().source;
        assert!(del(&["pts.B"]).contains("let pts = (A: (0, 0), C: (1, 1))\n"), "{}", del(&["pts.B"]));
        assert!(del(&["pts.B"]).contains("line(\"A\", (1, 0), \"C\")"));
        assert!(del(&["pts.A", "pts.B"]).contains("let pts = (C: (1, 1))"), "{}", del(&["pts.A", "pts.B"]));
        assert!(del(&["pts.A", "pts.B", "pts.C"]).contains("let pts = (:)\n"));
        assert!(del(&["pts.A", "pts.B", "pts.C"]).contains("line((0, 0), (1, 0), (1, 1))"));
    }

    #[test]
    fn delete_points_removes_lets() {
        let src = "#let O = (0, 0)\n#canvas({\n  import draw: *\n  circle(O)\n})\n";
        let out = apply(src, &Edit::DeletePoints { points: vec![point_id(src, "O")] }).unwrap();
        assert_eq!(out.source, "#canvas({\n  import draw: *\n  circle((0, 0))\n})\n");
    }

    #[test]
    fn delete_points_refuses_to_leave_dangling_names() {
        // The loop's data names "A"; the editor can't rewrite that.
        let src = "#canvas({\n  import draw: *\n  anchor(\"A\", (0, 0))\n  anchor(\"B\", (1, 0))\n  for (a, b) in ((\"A\", \"B\"),) { line(a, b) }\n})\n";
        let err = apply(src, &Edit::DeletePoints { points: vec![point_id(src, "A")] }).unwrap_err();
        assert!(err.contains("line 5"), "{err}");
        // Array items would renumber the rest.
        let src = "#let ps = ((0, 0), (1, 1))\n#canvas({ import draw: *; line(..ps) })\n";
        assert!(apply(src, &Edit::DeletePoints { points: vec![point_id(src, "ps[0]")] }).is_err());
    }

    #[test]
    fn move_points_shifts_each_once() {
        let src = "#canvas({\n  import draw: *\n  anchor(\"P1\", (1, 2))\n  anchor(\"P2\", (3, 4))\n})\n";
        let (p1, p2) = (point_id(src, "P1"), point_id(src, "P2"));
        let out = apply(src, &Edit::MovePoints { points: vec![p1, p2, p1], dx: 0.5, dy: -1.0 }).unwrap();
        assert!(out.source.contains("anchor(\"P1\", (1.5, 1))") && out.source.contains("anchor(\"P2\", (3.5, 3))"), "{}", out.source);
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
