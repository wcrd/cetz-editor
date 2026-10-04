//! Edit commands from the editor, applied as minimal text patches so every
//! byte the user didn't touch stays exactly as written.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use typst_syntax::{LinkedNode, SyntaxKind};

use crate::route::{self, Route};
use crate::scene::{self, Call, Scene, Value};
use crate::walk;

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
    /// Make a coordinate argument use the same point as another call's
    /// argument (`from`, `from_arg`): that one's shared point if it has one,
    /// else its literal becomes a new `anchor("P1", ..)` that both use.
    SharePoint { call: usize, arg: usize, from: usize, from_arg: usize },
    /// Set one positional coordinate argument. Literal coordinates keep their
    /// formatting; anything else (an anchor name, an expression) is replaced.
    SetCoord { call: usize, arg: usize, x: f64, y: f64 },
    /// Replace an argument's value with raw Typst source text.
    SetArgText { call: usize, arg: usize, text: String },
    /// Insert positional arguments (raw Typst source text) before argument
    /// `at`, or after the last argument when `at` is past the end: new
    /// vertices for a path.
    InsertArgs { call: usize, at: usize, texts: Vec<String> },
    /// Remove one positional argument, leaving at least `keep` of them.
    RemoveArg { call: usize, arg: usize, keep: usize },
    /// Set (`Some`) or remove (`None`) a named argument.
    SetNamed { call: usize, key: String, text: Option<String> },
    Delete { calls: Vec<usize> },
    /// Append a statement to the end of a canvas body (default: the first).
    Insert { canvas: Option<usize>, text: String },
    /// Append a call from one of CeTZ's libraries (`text` like
    /// `brace((0, 0), (2, 0))` from `module` `decorations`) to a canvas,
    /// named the way the file reaches that library (`decorations.brace`, or
    /// `cetz.decorations.brace` under a bare `#import "@preview/cetz:.."`),
    /// adding it to the file's CeTZ import list when it isn't there.
    /// `created` holds the call.
    InsertLibrary { canvas: Option<usize>, module: String, text: String },
    /// Several edits as one: each is worked out against the same source and
    /// their changes combined. Identical changes (two moves of one shared
    /// point) count once; conflicting ones fail. `created` is empty.
    Batch { edits: Vec<Edit> },
    /// Append copied statements (source text, as the editor copies them) to
    /// the end of a canvas, their literal coordinates moved by `(dx, dy)`. A
    /// name the canvas already uses gets a fresh one (`box` → `box-2`), and
    /// the pasted code's references to it follow. `created` holds the
    /// pasted calls.
    Paste { canvas: Option<usize>, text: String, dx: f64, dy: f64 },
    /// Point a coordinate argument at another call's anchor
    /// (`"name.anchor"`), naming that call first if it has no name. CeTZ
    /// knows names in drawing order, so a call drawn before its target first
    /// moves to just after it (in front of it), when that's allowed as
    /// `Reorder`; `created` then holds the moved call.
    Connect { call: usize, arg: usize, target: usize, anchor: String },
    /// Draw a connector, an arrow from one call's anchor to another's,
    /// straight or elbowed (see `route`), at the end of a canvas (default:
    /// the first), naming either call first if it has no name. `created`
    /// holds the line.
    /// `detour` steps out from each side first (see `route`); `fixed` pins
    /// it, as `SetFixed` does.
    AddConnector {
        canvas: Option<usize>,
        from: usize,
        from_anchor: String,
        to: usize,
        to_anchor: String,
        route: Route,
        #[serde(default)]
        detour: bool,
        /// How far a detour steps out (default `route::STUB`).
        #[serde(default)]
        stub: Option<f64>,
        #[serde(default)]
        fixed: bool,
    },
    /// Rewrite a connector's route as straight or elbowed, moving its ends
    /// to other anchors of the same shapes when `from_anchor` or `to_anchor`
    /// is given. An elbow crosses over halfway again, unless `keep_bend`.
    Reroute {
        call: usize,
        route: Route,
        from_anchor: Option<String>,
        to_anchor: Option<String>,
        #[serde(default)]
        keep_bend: bool,
        #[serde(default)]
        detour: bool,
        /// How far a detour steps out (default: as it does now, else `route::STUB`).
        #[serde(default)]
        stub: Option<f64>,
    },
    /// Move one end of a connector (its start, or its end with `to_end`) to
    /// another call, naming that call if it has no name, joining the anchors
    /// given for both ends and keeping its route and bend. With `fixed`, the
    /// connector is pinned too. CeTZ knows names in drawing order, so a
    /// connector drawn before its new shape moves to the end of the canvas;
    /// `created` then holds it.
    Reconnect {
        call: usize,
        to_end: bool,
        target: usize,
        from_anchor: String,
        to_anchor: String,
        fixed: bool,
        #[serde(default)]
        detour: bool,
        /// How far a detour steps out (default: as it does now, else `route::STUB`).
        #[serde(default)]
        stub: Option<f64>,
    },
    /// Set how far a detouring elbow connector steps out from each side.
    SetStub { call: usize, stub: f64 },
    /// Pin a connector's sides (`fixed`) with a `// cetz-editor: fixed`
    /// comment line above it, or unpin them, removing the comment. The
    /// editor re-picks an unpinned connector's sides when its shapes move.
    SetFixed { call: usize, fixed: bool },
    /// Move where a two-corner elbow connector crosses over: `ratio` (0 to
    /// 1) of the way from its start to its end.
    Bend { call: usize, ratio: f64 },
    /// Copy the calls right after themselves, offset by `(dx, dy)`, without
    /// their `name:` so names stay unique.
    Duplicate { calls: Vec<usize>, dx: f64, dy: f64 },
    /// Turn a call `angle` degrees about `(x, y)` in its own frame: wrap it
    /// in `scope({ rotate(<angle>deg, origin: (x, y)) ... })`, or set or add
    /// the `rotate` of the scope it's already wrapped in (`call` may be that
    /// scope). Unlike a group, a scope lets the names inside it be used
    /// outside, so nothing that refers to the call changes. `created` holds
    /// the scope when it's new.
    Rotate { call: usize, angle: f64, x: f64, y: f64 },
    /// Undo `Rotate`: drop the `rotate` from the scope `call` wraps its shape
    /// in (or is), and the scope too when nothing else is left in it, as the
    /// rotation handle does at 0°. `created` holds the shape when unwrapped.
    Unrotate { call: usize },
    /// Scale a call by `factor` about `(x, y)` in its own frame, through the
    /// same `scope({ rotate(..); scale(..); shape })` as `Rotate`: the
    /// scope's `scale` changes, or one is added (wrapping the call the first
    /// time). A factor of 1 drops it, and the scope when nothing's left.
    Scale { call: usize, factor: f64, x: f64, y: f64 },
    /// Wrap the calls in a named `group(name: "group", { ... })` where the
    /// first of them is. They must sit in the same block; later ones move up
    /// to join it. References from outside to their names become
    /// `"group.name"`. `created` holds the group.
    Group { calls: Vec<usize> },
    /// Replace groups with their bodies, turning references like
    /// `"g.r.east"` into `"r.east"`. Fails where that would change the
    /// drawing: a group's own anchors in use, a transform that would leak, a
    /// name clash. `created` holds the calls that were their statements.
    Ungroup { calls: Vec<usize> },
    /// Move the calls (statements of one block) toward the front, later in
    /// the block so they're drawn over what they pass, or toward the back:
    /// past one statement that draws, or as far as they can go. They stop
    /// at an import, a style or transform, and anything they use or that
    /// uses them. `created` holds the calls.
    Arrange { calls: Vec<usize>, to: Layer },
    /// Move the calls to just before (or `after`) the statement of `target`
    /// in their block, refusing like `Arrange` would. `created` holds the
    /// calls, or is empty when nothing changed.
    Reorder { calls: Vec<usize>, target: usize, after: bool },
    /// Move every `anchor(..)` statement (and loop of them) up to the top of
    /// its block, before the first statement that draws, keeping their
    /// order. One stops early below a transform (its coordinates are in that
    /// frame) or anything it uses. Fails if none move.
    GatherAnchors,
    /// Write the editor's grid step for a canvas into the
    /// `// cetz-editor: grid <step>` comment above it, adding one if needed.
    SetGrid { canvas: usize, step: String },
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    Forward,
    Backward,
    Front,
    Back,
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

/// Calls that change how everything after them in their block is drawn.
fn changes_state(call: &Call) -> bool {
    let base = base_name(&call.callee);
    TRANSFORMS.contains(&base) || base == "set-style" || base == "set-ctx"
}

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
                let root = typst_syntax::parse(source);
                let root = LinkedNode::new(&root);
                let slot = find_node(&root, &canvas.body, SyntaxKind::CodeBlock)
                    .and_then(|block| walk::block_code(&block))
                    .and_then(|code| anchor_slot(source, &scene, &code, None));
                match slot {
                    Some(before) => {
                        let (insert, at) = insert_statement(source, before.offset(), &format!("{definition}{value})"));
                        created.push((patches.len(), at + definition.len()));
                        patches.push(insert);
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
            for arg in scene.calls().flat_map(|c| &c.args).filter(|a| a.point == Some(*point)) {
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
            let name = match name {
                Some(n) if !n.is_empty() && !n.contains('.') => n.clone(),
                Some(_) => return Err("a point name can't be empty or contain '.'".into()),
                None => unique_point_name(&scene),
            };
            extract_point(source, &scene, find_call(&scene, *call)?, *arg, &name, &mut patches)?;
        }
        Edit::SharePoint { call, arg, from, from_arg } => {
            if (call, arg) == (from, from_arg) {
                return Err("a point can't be shared with itself".into());
            }
            let at = find_call(&scene, *call)?.range.start;
            let target = find_arg(&scene, *call, *arg)?;
            let from_call = find_call(&scene, *from)?;
            let source_arg = from_call.args.get(*from_arg).ok_or("no such argument")?;
            let late = || Err("can only share a point defined before this shape".to_string());
            let text = if let Some(point) = source_arg.point {
                let point = scene.points.iter().find(|p| p.id == point).ok_or("no such point")?;
                if !defined_before(&scene, &point.range, at) {
                    return late();
                }
                source_arg.text.clone()
            } else {
                let name = unique_point_name(&scene);
                // The new anchor goes above the line it's taken from, which
                // must also put it above this shape.
                let anchor = extract_point(source, &scene, from_call, *from_arg, &name, &mut patches)?;
                if !defined_before(&scene, &(anchor..anchor), at) {
                    return late();
                }
                format!("{name:?}")
            };
            patches.push(patch(target.value_range.clone(), text));
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
        Edit::InsertArgs { call, at, texts } => {
            let call = find_call(&scene, *call)?;
            if texts.is_empty() {
                return Err("nothing to insert".into());
            }
            let list = texts.join(", ");
            match (call.args.get(*at), call.args.last()) {
                (Some(arg), _) => patches.push(patch(arg.range.start..arg.range.start, format!("{list}, "))),
                (None, Some(last)) => patches.push(patch(last.range.end..last.range.end, format!(", {list}"))),
                (None, None) => {
                    let close = call.args_close.ok_or("call has no argument list")?;
                    patches.push(patch(close..close, list));
                }
            }
        }
        Edit::RemoveArg { call, arg, keep } => {
            let call = find_call(&scene, *call)?;
            let target = call.args.get(*arg).filter(|a| a.key.is_none()).ok_or("only a positional argument can be removed")?;
            if call.args.iter().filter(|a| a.key.is_none()).count() <= *keep {
                return Err(format!("a {} needs at least {keep} points", base_name(&call.callee)));
            }
            let range = match (arg.checked_sub(1).and_then(|i| call.args.get(i)), call.args.get(arg + 1)) {
                (_, Some(next)) => target.range.start..next.range.start,
                (Some(prev), None) => prev.range.end..target.range.end,
                (None, None) => target.range.clone(),
            };
            patches.push(patch(range, String::new()));
        }
        Edit::SetNamed { call, key, text } => {
            let call = find_call(&scene, *call)?;
            set_named(call, key, text.as_deref(), &mut patches)?;
        }
        Edit::Delete { calls } => {
            for call in outermost(&scene, calls)? {
                // A pinned connector's comment goes too, rather than pin what comes next.
                if let Some(comment) = scene::fixed_comment(source, call.range.start).filter(|_| call.connector.is_some()) {
                    patches.push(patch(comment, String::new()));
                }
                if whole_body(&scene, call) {
                    // A function that was only this call is left drawing nothing.
                    patches.push(patch(call.range.clone(), "{}".into()));
                } else {
                    patches.push(patch(statement_range(source, &call.range), String::new()));
                }
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
        Edit::InsertLibrary { canvas, module, text } => {
            let canvas = match canvas {
                Some(id) => scene.canvases.iter().find(|c| c.id == *id),
                None => scene.canvases.first(),
            }
            .ok_or("no canvas to insert into")?;
            let (path, import) = library_path(source, module)?;
            patches.extend(import);
            let (at, prefix, suffix) = insertion_point(source, canvas);
            created.push((patches.len(), prefix.len()));
            patches.push(patch(at..at, format!("{prefix}{path}{text}{suffix}")));
        }
        Edit::Batch { edits } => {
            for edit in edits {
                for p in apply(source, edit)?.patches {
                    if !patches.contains(&p) {
                        patches.push(p);
                    }
                }
            }
        }
        Edit::Paste { canvas, text, dx, dy } => {
            let canvas = match canvas {
                Some(id) => scene.canvases.iter().find(|c| c.id == *id),
                None => scene.canvases.first(),
            }
            .ok_or("no canvas to paste into")?;
            let (text, starts) = paste_text(&scene, text, *dx, *dy)?;
            let (at, prefix, suffix) = insertion_point(source, canvas);
            let indent = prefix.trim_start_matches('\n');
            // Every pasted line after the first starts at the canvas's indent.
            let indented = text.replace('\n', &format!("\n{indent}"));
            for start in starts {
                created.push((patches.len(), prefix.len() + start + text[..start].matches('\n').count() * indent.len()));
            }
            patches.push(patch(at..at, format!("{prefix}{indented}{suffix}")));
        }
        Edit::Connect { call, arg, target, anchor } => {
            if call == target {
                return Err("can't connect a call to itself".into());
            }
            let at = find_call(&scene, *call)?.range.start;
            if !defined_before(&scene, &find_call(&scene, *target)?.range, at) {
                return connect_after(source, &scene, *call, *arg, *target, anchor);
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
        Edit::AddConnector { canvas, from, from_anchor, to, to_anchor, route, detour, stub, fixed } => {
            if from == to {
                return Err("a connector needs two different shapes".into());
            }
            let canvas = match canvas {
                Some(id) => scene.canvases.iter().find(|c| c.id == *id),
                None => scene.canvases.first(),
            }
            .ok_or("no canvas to draw a connector in")?;
            let (at, prefix, suffix) = insertion_point(source, canvas);
            let mut fresh = Vec::new();
            let mut end = |id: usize, anchor: &str, patches: &mut Vec<Patch>| -> Result<String, String> {
                let target = find_call(&scene, id)?;
                if !canvas.calls.iter().any(|c| c.id == id) {
                    return Err("a connector's shapes must be in its canvas".into());
                }
                let path = path_from(&scene, target.parent, &(at..at)).ok_or("can't connect to a shape in a group with no name")?;
                let name = match &target.name {
                    Some(name) => name.clone(),
                    None => {
                        let name = unique_name_avoiding(&scene, base_name(&target.callee), &fresh);
                        set_named(target, "name", Some(&format!("{name:?}")), patches)?;
                        fresh.push(name.clone());
                        name
                    }
                };
                Ok(format!("{path}{name}.{anchor}"))
            };
            let from = end(*from, from_anchor, &mut patches)?;
            let to = end(*to, to_anchor, &mut patches)?;
            let text = format!("line({}, mark: (end: \">\"))", route::vertices(&from, &to, *route, 0.5, detour.then(|| stub.unwrap_or(route::STUB))).join(", "));
            let pin = if *fixed { format!("// cetz-editor: fixed\n{}", prefix.trim_start_matches('\n')) } else { String::new() };
            created.push((patches.len(), prefix.len() + pin.len()));
            patches.push(patch(at..at, format!("{prefix}{pin}{text}{suffix}")));
        }
        Edit::Reroute { call, route, from_anchor, to_anchor, keep_bend, detour, stub } => {
            let call = find_call(&scene, *call)?;
            let connector = call.connector.as_ref().ok_or("only a connector can be rerouted")?;
            let from = from_anchor.as_deref().map_or(connector.from.clone(), |a| route::with_anchor(&connector.from, a));
            let to = to_anchor.as_deref().map_or(connector.to.clone(), |a| route::with_anchor(&connector.to, a));
            let bend = if *keep_bend { connector.bend.unwrap_or(0.5) } else { 0.5 };
            patches.push(patch(connector_points(call)?, route::vertices(&from, &to, *route, bend, detour.then(|| stub.or(connector.stub).unwrap_or(route::STUB))).join(", ")));
        }
        Edit::Reconnect { call, to_end, target, from_anchor, to_anchor, fixed, detour, stub } => {
            let call = find_call(&scene, *call)?;
            let connector = call.connector.as_ref().ok_or("only a connector can be reconnected")?;
            let target = find_call(&scene, *target)?;
            let canvas = scene.canvas_of(call.id).ok_or("the connector isn't in a canvas")?;
            if target.id == call.id || !canvas.calls.iter().any(|c| c.id == target.id) {
                return Err("a connector can only join shapes in its own canvas".into());
            }
            let moves = !defined_before(&scene, &target.range, call.range.start);
            let (at, prefix, suffix) = insertion_point(source, canvas);
            let used_at = if moves { at } else { call.range.start };
            let path = path_from(&scene, target.parent, &(used_at..used_at)).ok_or("can't connect to a shape in a group with no name")?;
            let name = match &target.name {
                Some(name) => name.clone(),
                None => {
                    let name = unique_name(&scene, base_name(&target.callee));
                    set_named(target, "name", Some(&format!("{name:?}")), &mut patches)?;
                    name
                }
            };
            let (from, to) = if *to_end {
                (route::with_anchor(&connector.from, from_anchor), format!("{path}{name}.{to_anchor}"))
            } else {
                (format!("{path}{name}.{from_anchor}"), route::with_anchor(&connector.to, to_anchor))
            };
            let points = route::vertices(&from, &to, connector.route, connector.bend.unwrap_or(0.5), detour.then(|| stub.or(connector.stub).unwrap_or(route::STUB))).join(", ");
            let range = connector_points(call)?;
            let pinned = *fixed || connector.fixed;
            if !moves {
                patches.push(patch(range, points));
                if pinned && !connector.fixed {
                    let line_start = source[..call.range.start].rfind('\n').map_or(0, |i| i + 1);
                    let indent = &source[line_start..call.range.start];
                    if !indent.trim().is_empty() {
                        return Err("put the connector on a line of its own to fix its sides".into());
                    }
                    patches.push(patch(line_start..line_start, format!("{indent}// cetz-editor: fixed\n")));
                }
            } else {
                // Its text, with the new points, goes to the end of the canvas, its pin with it.
                let mut text = source[call.range.clone()].to_string();
                text.replace_range(range.start - call.range.start..range.end - call.range.start, &points);
                if let Some(comment) = scene::fixed_comment(source, call.range.start) {
                    patches.push(patch(comment, String::new()));
                }
                patches.push(patch(statement_range(source, &call.range), String::new()));
                let indent = prefix.trim_start_matches('\n');
                let comment = if pinned { format!("// cetz-editor: fixed\n{indent}") } else { String::new() };
                created.push((patches.len(), prefix.len() + comment.len()));
                patches.push(patch(at..at, format!("{prefix}{comment}{text}{suffix}")));
            }
        }
        Edit::SetFixed { call, fixed } => {
            let call = find_call(&scene, *call)?;
            if call.connector.is_none() {
                return Err("only a connector's sides can be fixed".into());
            }
            match (scene::fixed_comment(source, call.range.start), fixed) {
                (None, true) => {
                    let line_start = source[..call.range.start].rfind('\n').map_or(0, |i| i + 1);
                    let indent = &source[line_start..call.range.start];
                    if !indent.trim().is_empty() {
                        return Err("put the connector on a line of its own to fix its sides".into());
                    }
                    patches.push(patch(line_start..line_start, format!("{indent}// cetz-editor: fixed\n")));
                }
                (Some(comment), false) => patches.push(patch(comment, String::new())),
                _ => {}
            }
        }
        Edit::Bend { call, ratio } => {
            let call = find_call(&scene, *call)?;
            let connector = call.connector.as_ref().filter(|c| c.bend.is_some()).ok_or("only an elbow with two corners can bend")?;
            if !(0.0..=1.0).contains(ratio) {
                return Err("a bend must be between its two ends".into());
            }
            patches.push(patch(connector_points(call)?, route::vertices(&connector.from, &connector.to, Route::Elbow, *ratio, connector.detour.then(|| connector.stub.unwrap_or(route::STUB))).join(", ")));
        }
        Edit::SetStub { call, stub } => {
            let call = find_call(&scene, *call)?;
            let connector = call.connector.as_ref().filter(|c| c.detour).ok_or("only a detouring elbow steps out")?;
            if !(*stub > 0.0) {
                return Err("a step out must be more than 0".into());
            }
            let points = route::vertices(&connector.from, &connector.to, Route::Elbow, connector.bend.unwrap_or(0.5), Some(*stub));
            patches.push(patch(connector_points(call)?, points.join(", ")));
        }
        Edit::Duplicate { calls, dx, dy } => {
            for call in outermost(&scene, calls)? {
                let copy = duplicate_text(source, &scene, call, *dx, *dy)?;
                if whole_body(&scene, call) {
                    // `let f() = line(..)` becomes a block holding both.
                    let indent = indent_at(source, call.range.start);
                    let inner = |text: &str| text.replace('\n', "\n  ");
                    let head = format!("{{\n{indent}  {}\n{indent}  ", inner(&source[call.range.clone()]));
                    created.push((patches.len(), head.len()));
                    patches.push(patch(call.range.clone(), format!("{head}{}\n{indent}}}", inner(&copy))));
                    continue;
                }
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
        Edit::Group { calls } => {
            group(source, &scene, calls, &mut patches, &mut created)?;
        }
        Edit::Rotate { call, angle, x, y } => {
            set_transform(source, &scene, *call, "rotate", Some(format!("{}deg", num(*angle))), (*x, *y), &mut patches, &mut created)?;
        }
        Edit::Unrotate { call } => {
            set_transform(source, &scene, *call, "rotate", None, (0.0, 0.0), &mut patches, &mut created)?;
        }
        Edit::Scale { call, factor, x, y } => {
            let value = ((factor - 1.0).abs() > 1e-9).then(|| num(*factor));
            set_transform(source, &scene, *call, "scale", value, (*x, *y), &mut patches, &mut created)?;
        }
        Edit::Ungroup { calls } => {
            let mut names = Vec::new();
            for group in outermost(&scene, calls)? {
                ungroup(source, &scene, group, &mut names, &mut patches, &mut created)?;
            }
        }
        Edit::Arrange { calls, to } => {
            let calls = outermost(&scene, calls)?;
            let root = typst_syntax::parse(source);
            let root = LinkedNode::new(&root);
            let (stmts, selected) = block_stmts(source, &scene, &root, &calls)?;
            let order = arrange(&stmts, &selected, *to)?;
            restack(source, &stmts, &order, &selected, &mut patches, &mut created);
        }
        Edit::SetGrid { canvas, step } => {
            let canvas = scene.canvases.iter().find(|c| c.id == *canvas).ok_or("no such canvas")?;
            match scene::grid_comment(source, canvas.id) {
                Some(value) => patches.push(patch(value, step.clone())),
                None => {
                    let line_start = source[..canvas.id].rfind('\n').map_or(0, |i| i + 1);
                    let indent = indent_at(source, canvas.id);
                    patches.push(patch(line_start..line_start, format!("{indent}// cetz-editor: grid {step}\n")));
                }
            }
        }
        Edit::GatherAnchors => {
            let text = gather_anchors(source)?;
            // One patch over what changed.
            let mut start = source.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
            let mut end = source.bytes().rev().zip(text.bytes().rev()).take_while(|(a, b)| a == b).count().min(source.len().min(text.len()) - start);
            while !source.is_char_boundary(start) || !text.is_char_boundary(start) {
                start -= 1;
            }
            while !source.is_char_boundary(source.len() - end) || !text.is_char_boundary(text.len() - end) {
                end -= 1;
            }
            patches.push(patch(start..source.len() - end, text[start..text.len() - end].to_string()));
        }
        Edit::Reorder { calls, target, after } => {
            let calls = outermost(&scene, calls)?;
            let root = typst_syntax::parse(source);
            let root = LinkedNode::new(&root);
            let (stmts, selected) = block_stmts(source, &scene, &root, &calls)?;
            let target = find_call(&scene, *target)?;
            let target = stmts.iter().position(|s| s.node.range() == target.range).ok_or("can only reorder shapes within the same block")?;
            let order = reorder(&stmts, &selected, target, *after)?;
            restack(source, &stmts, &order, &selected, &mut patches, &mut created);
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

/// Whether the call is the whole body of a drawing function, as in
/// `let f(..) = line(..)`, rather than a statement in a block.
fn whole_body(scene: &Scene, call: &Call) -> bool {
    call.function.and_then(|f| scene.function(f)).is_some_and(|f| f.body == call.range)
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
    // A rotation's or scaling's literal `origin:` moves with what it turns,
    // so the shapes move by the same amount, not along the turned axes.
    for call in calls.iter().filter(|c| matches!(base_name(&c.callee), "rotate" | "scale")) {
        for arg in call.args.iter().filter(|a| a.key.as_deref() == Some("origin") && a.point.is_none()) {
            if let Value::Coord { x, y, x_range, y_range } = &arg.value {
                patches.push(patch(x_range.clone(), num(x + dx)));
                patches.push(patch(y_range.clone(), num(y + dy)));
            }
        }
    }
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
    let calls: Vec<&Call> = scene.calls().collect();
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
        || scene.calls().any(|c| c.name.as_deref() == Some(name))
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

/// Turns a literal coordinate argument into `anchor("name", (x, y))` before
/// its call, used as `"name"`. Returns where the anchor goes in `source`.
fn extract_point(source: &str, scene: &Scene, call: &Call, arg: usize, name: &str, patches: &mut Vec<Patch>) -> Result<usize, String> {
    let arg = call.args.get(arg).ok_or("no such argument")?;
    if !matches!(arg.value, Value::Coord { .. }) || arg.point.is_some() {
        return Err("only a literal coordinate can become a shared point".into());
    }
    let root = typst_syntax::parse(source);
    let root = LinkedNode::new(&root);
    let node = find_node(&root, &call.range, SyntaxKind::FuncCall);
    let before = node
        .as_ref()
        .and_then(|node| node.parent().filter(|p| p.kind() == SyntaxKind::Code))
        .and_then(|code| anchor_slot(source, scene, code, Some(call.range.start)))
        .map_or(call.range.start, |s| s.offset());
    patches.push(insert_statement(source, before, &format!("anchor({:?}, {})", name, arg.text)).0);
    patches.push(patch(arg.value_range.clone(), format!("{name:?}")));
    Ok(before)
}

/// Whether code at offset `at` can use what's defined over `range`: CeTZ
/// resolves names in drawing order, so it must come first, and in the same
/// canvas unless it's outside every canvas.
/// `Connect` to a shape drawn after the call: the call's statement moves to
/// just after the one holding the target in its block, then connects.
fn connect_after(source: &str, scene: &Scene, call: usize, arg: usize, target: usize, anchor: &str) -> Result<EditResult, String> {
    let refuse = |why: &str| format!("can only connect to a shape drawn before this one ({why})");
    let moving = find_call(scene, call)?;
    // The statement holding the target, in the call's block.
    let mut holder = find_call(scene, target)?;
    while holder.parent != moving.parent {
        holder = holder.parent.and_then(|p| scene.call(p)).ok_or_else(|| refuse("it's in another block"))?;
    }
    if holder.id == call {
        return Err("can't connect a call to its own shapes".into());
    }
    let moved = apply(source, &Edit::Reorder { calls: vec![call], target: holder.id, after: true }).map_err(|e| refuse(&e))?;
    let new_call = *moved.created.first().ok_or_else(|| refuse("it can't move"))?;
    // Only the moved call (and what's in it) changes places, so the target
    // keeps its place among all the other calls.
    let others = |scene: &Scene, moved: &Range<usize>| -> Vec<usize> {
        let mut ids: Vec<usize> = scene.calls().filter(|c| !(moved.start <= c.range.start && c.range.end <= moved.end)).map(|c| c.id).collect();
        ids.sort_unstable();
        ids
    };
    let index = others(scene, &moving.range).iter().position(|&id| id == target).ok_or_else(|| refuse("it's lost"))?;
    let after = scene::parse(&moved.source);
    let moved_range = after.call(new_call).ok_or_else(|| refuse("it's lost"))?.range.clone();
    let target = *others(&after, &moved_range).get(index).ok_or_else(|| refuse("it's lost"))?;
    let call = new_call;
    let out = apply(&moved.source, &Edit::Connect { call, arg, target, anchor: anchor.to_string() })?;
    let created = vec![map_offset(&out.patches, call)];
    Ok(EditResult { patches: vec![diff(source, &out.source)], source: out.source, created })
}

/// The range of a connector's points, from its start anchor to its end.
fn connector_points(call: &Call) -> Result<Range<usize>, String> {
    let positional: Vec<&scene::Arg> = call.args.iter().filter(|a| a.key.is_none()).collect();
    let (first, last) = (positional.first().ok_or("no points")?, positional.last().ok_or("no points")?);
    if call.args.iter().any(|a| a.key.is_some() && first.range.start < a.range.start && a.range.end < last.range.end) {
        return Err("a connector's points must come before its named arguments".into());
    }
    Ok(first.range.start..last.range.end)
}

/// Where an offset lands after the patches (each from the same source).
fn map_offset(patches: &[Patch], at: usize) -> usize {
    let shift: isize = patches.iter().filter(|p| p.end <= at).map(|p| p.text.len() as isize - (p.end - p.start) as isize).sum();
    (at as isize + shift) as usize
}

/// One patch that turns `old` into `new`: what lies between their common start and end.
fn diff(old: &str, new: &str) -> Patch {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let most = old.len().min(new.len()) - start;
    let mut end = old.bytes().rev().zip(new.bytes().rev()).take(most).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - end) || !new.is_char_boundary(new.len() - end) {
        end -= 1;
    }
    patch(start..old.len() - end, new[start..new.len() - end].to_string())
}

fn defined_before(scene: &Scene, range: &Range<usize>, at: usize) -> bool {
    range.end <= at && scene.canvases.iter().filter(|c| c.body.contains(&range.start)).all(|c| c.body.contains(&at))
}

/// The statement a new anchor goes before, so anchors gather at the top of a
/// block where every shape can use them: before the first statement that
/// draws, or for the coordinate of `call` (an offset), before that call and
/// after any transform above it, since the coordinate is in its frame. `None`
/// to append.
fn anchor_slot<'a>(source: &str, scene: &Scene, code: &LinkedNode<'a>, call: Option<usize>) -> Option<LinkedNode<'a>> {
    let stmts: Vec<Stmt> = code.children().filter(|c| !is_trivia(c.kind())).map(|n| stmt(source, scene, n, false, code.offset())).collect();
    let at = match call {
        Some(id) => stmts.iter().position(|s| s.node.offset() == id)?,
        None => stmts.len(),
    };
    let first = stmts[..at].iter().position(|s| s.draws || (call.is_none() && s.state)).unwrap_or(at);
    let after_state = if call.is_some() { stmts[..at].iter().rposition(|s| s.state).map_or(0, |i| i + 1) } else { 0 };
    stmts.get(first.max(after_state)).map(|s| s.node.clone())
}

/// A patch putting a statement before the one at `before`: on a line of its
/// own when that one has its own line, else followed by `; `. Also returns
/// where the statement starts in the patch text.
fn insert_statement(source: &str, before: usize, text: &str) -> (Patch, usize) {
    let line_start = source[..before].rfind('\n').map_or(0, |i| i + 1);
    let indent = &source[line_start..before];
    if indent.trim().is_empty() {
        (patch(line_start..line_start, format!("{indent}{text}\n")), indent.len())
    } else {
        (patch(before..before, format!("{text}; ")), 0)
    }
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
    unique_name_avoiding(scene, base, &[])
}

/// Like `unique_name`, also skipping names the edit has just given out.
fn unique_name_avoiding(scene: &Scene, base: &str, given: &[String]) -> String {
    let taken = |n: &str| given.iter().any(|g| g == n) || scene.calls().any(|c| c.name.as_deref() == Some(n));
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

/// How the file reaches CeTZ's `module` library (`"decorations."`), and the
/// change to its CeTZ import that brings the library in, if one's needed.
fn library_path(source: &str, module: &str) -> Result<(String, Option<Patch>), String> {
    fn imports<'a>(node: &LinkedNode<'a>, out: &mut Vec<LinkedNode<'a>>) {
        if node.kind() == SyntaxKind::ModuleImport {
            out.push(node.clone());
        }
        for child in node.children() {
            imports(&child, out);
        }
    }
    let root = typst_syntax::parse(source);
    let mut found = Vec::new();
    imports(&LinkedNode::new(&root), &mut found);
    let is_cetz = |import: &LinkedNode| {
        import.children().any(|c| c.get().cast::<typst_syntax::ast::Str>().is_some_and(|s| s.get().starts_with("@preview/cetz:")))
    };
    let Some(import) = found.iter().find(|i| is_cetz(i)) else {
        return Err(format!("can't add the {module} library: this file doesn't import @preview/cetz"));
    };
    let direct = format!("{module}.");
    if import.children().any(|c| c.kind() == SyntaxKind::Star) {
        return Ok((direct, None));
    }
    if let Some(items) = import.children().find(|c| c.kind() == SyntaxKind::ImportItems) {
        let listed: Vec<LinkedNode> = items.children().filter(|c| matches!(c.kind(), SyntaxKind::ImportItemPath | SyntaxKind::RenamedImportItem)).collect();
        // An item binds its last identifier: `draw`, or `d` in `draw as d`.
        let binds = |item: &LinkedNode| item.children().filter(|c| c.kind() == SyntaxKind::Ident).last().map(|c| source[c.range()].to_string()).or_else(|| Some(source[item.range()].to_string()));
        if listed.iter().any(|item| binds(item).as_deref() == Some(module)) {
            return Ok((direct, None));
        }
        let end = listed.last().map_or(items.range().end, |item| item.range().end);
        return Ok((direct, Some(patch(end..end, format!(", {module}")))));
    }
    // A bare import binds the package by its name, or by its `as` name.
    let name = import.children().filter(|c| c.kind() == SyntaxKind::Ident).last().map_or("cetz".to_string(), |c| source[c.range()].to_string());
    Ok((format!("{name}.{module}."), None))
}

/// Copied statements ready to paste into `scene` (see `Edit::Paste`), and
/// where each top-level call starts in them.
fn paste_text(scene: &Scene, text: &str, dx: f64, dy: f64) -> Result<(String, Vec<usize>), String> {
    // Parse the text as a canvas body of its own.
    const OPEN: &str = "#canvas({\n";
    let wrapped = format!("{OPEN}{}\n}})", text.trim());
    let pasted = scene::parse(&wrapped);
    let calls: Vec<&Call> = pasted.canvases.iter().flat_map(|c| &c.calls).collect();
    if !calls.iter().any(|c| c.parent.is_none()) {
        return Err("nothing to paste: the clipboard has no drawing calls".into());
    }
    // Points defined in the pasted code move once, through their definitions.
    let mut patches = Vec::new();
    move_calls(&pasted, &calls, dx, dy, false, &mut patches);

    // Names already in the scene get fresh ones, and so do references to them.
    let mut renamed: Vec<(String, String)> = Vec::new();
    for call in &calls {
        let Some(name) = call.name.as_deref().filter(|n| name_taken(scene, n)) else { continue };
        let fresh = (2..).map(|i| format!("{name}-{i}")).find(|n| !name_taken(scene, n) && !renamed.iter().any(|(_, r)| r == n)).unwrap();
        set_named(call, "name", Some(&format!("{fresh:?}")), &mut patches)?;
        renamed.push((name.to_string(), fresh));
    }
    if !renamed.is_empty() {
        let root = typst_syntax::parse(&wrapped);
        let mut strs = Vec::new();
        strings(&LinkedNode::new(&root), &mut strs);
        let names: Vec<Range<usize>> = calls.iter().filter_map(|c| c.args.iter().find(|a| a.key.as_deref() == Some("name"))).map(|a| a.value_range.clone()).collect();
        for (range, value) in strs.into_iter().filter(|(r, _)| !names.contains(r)) {
            if let Some((old, fresh)) = renamed.iter().find(|(old, _)| refers(&value, old)) {
                patches.push(patch(range, format!("{:?}", format!("{fresh}{}", &value[old.len()..]))));
            }
        }
    }
    let out = finish(&wrapped, patches, vec![])?.source;
    let body = out[OPEN.len()..out.len() - "\n})".len()].to_string();
    let starts = scene::parse(&out).canvases.iter().flat_map(|c| &c.calls).filter(|c| c.parent.is_none()).map(|c| c.range.start - OPEN.len()).collect();
    Ok((body, starts))
}

/// Wraps the calls in a named group (see `Edit::Group`).
fn group(source: &str, scene: &Scene, ids: &[usize], patches: &mut Vec<Patch>, created: &mut Vec<(usize, usize)>) -> Result<(), String> {
    let calls = outermost(scene, ids)?;
    let Some(&first) = calls.first() else {
        return Err("nothing to group".into());
    };
    if calls.iter().any(|c| c.in_loop) {
        return Err("can't group shapes inside a loop".into());
    }
    let root = typst_syntax::parse(source);
    let root = LinkedNode::new(&root);
    let blocks: Vec<Option<Range<usize>>> =
        calls.iter().map(|c| find_node(&root, &c.range, SyntaxKind::FuncCall).and_then(|n| n.parent().map(|p| p.range()))).collect();
    let code = match &blocks[0] {
        Some(block) if blocks.iter().all(|b| b == &blocks[0]) => find_node(&root, block, SyntaxKind::Code),
        _ => None,
    }
    .ok_or("can only group shapes in the same block")?;
    let selected = |r: &Range<usize>| calls.iter().any(|c| c.range == *r);
    let siblings: Vec<LinkedNode> = code.children().filter(|c| !is_trivia(c.kind())).collect();

    // A transform or style moved into the group would stop applying to the
    // shapes after it that stay outside.
    for c in calls.iter().filter(|c| changes_state(c)) {
        if siblings.iter().any(|s| s.offset() > c.range.start && !selected(&s.range()) && s.kind() != SyntaxKind::LetBinding) {
            return Err(format!("can't group {}(..): it would stop applying to the shapes after it", base_name(&c.callee)));
        }
    }
    // Later calls move up past the statements between them: they mustn't use
    // anything those define.
    for c in &calls[1..] {
        let (mut names, mut vars) = (Vec::new(), Vec::new());
        for s in siblings.iter().filter(|s| s.offset() > first.range.start && s.offset() < c.range.start && !selected(&s.range())) {
            if s.kind() == SyntaxKind::LetBinding {
                vars.extend(let_names(s));
            } else {
                names.extend(defined_names(scene, &s.range()));
            }
        }
        let node = find_node(&root, &c.range, SyntaxKind::FuncCall).ok_or("can't find a shape to group")?;
        if let Some(used) = uses_any(&node, &names, &vars) {
            return Err(format!("can't group: {}(..) uses {used}, which comes between the shapes", base_name(&c.callee)));
        }
    }

    let name = std::iter::once("group".to_string()).chain((2..).map(|i| format!("group-{i}"))).find(|n| !name_taken(scene, n)).unwrap();
    // References to the grouped shapes from outside now go through the group.
    let moved: Vec<String> = calls.iter().filter_map(|c| element_name(c)).collect();
    let inside = |r: &Range<usize>| calls.iter().any(|c| c.range.start <= r.start && r.end <= c.range.end);
    let (body, _) = scene.block_of(first.id).ok_or("shapes outside a canvas")?;
    let mut strs = Vec::new();
    if let Some(body) = find_node(&root, &body, SyntaxKind::CodeBlock).or_else(|| find_node(&root, &body, SyntaxKind::FuncCall)) {
        strings(&body, &mut strs);
    }
    for (range, value) in strs {
        if range.start <= first.range.start || inside(&range) {
            continue;
        }
        let Some(prefix) = path_from(scene, first.parent, &range) else { continue };
        if let Some(rest) = value.strip_prefix(prefix.as_str()).filter(|rest| moved.iter().any(|n| refers(rest, n))) {
            patches.push(patch(range, format!("{:?}", format!("{prefix}{name}.{rest}"))));
        }
    }

    let indent = indent_at(source, first.range.start);
    let line_start = source[..first.range.start].rfind('\n').map_or(0, |i| i + 1);
    let texts = calls.iter().map(|c| &source[c.range.clone()]);
    let text = if source[line_start..first.range.start].trim().is_empty() {
        let body: String = texts.map(|t| format!("\n{indent}  {}", t.replace('\n', "\n  "))).collect();
        format!("group(name: {name:?}, {{{body}\n{indent}}})")
    } else {
        format!("group(name: {name:?}, {{ {} }})", texts.collect::<Vec<_>>().join("; "))
    };
    created.push((patches.len(), 0));
    patches.push(patch(first.range.clone(), text));
    for c in &calls[1..] {
        patches.push(patch(statement_range(source, &c.range), String::new()));
    }
    Ok(())
}

/// Replaces a group with its body (see `Edit::Ungroup`). `released` holds
/// the names earlier groups in the same edit let out, and gains this one's.
fn ungroup(
    source: &str,
    scene: &Scene,
    group: &Call,
    released: &mut Vec<String>,
    patches: &mut Vec<Patch>,
    created: &mut Vec<(usize, usize)>,
) -> Result<(), String> {
    if base_name(&group.callee) != "group" {
        return Err("only a group can be ungrouped".into());
    }
    if let Some(key) = group.args.iter().filter_map(|a| a.key.as_deref()).find(|k| !matches!(*k, "name" | "padding")) {
        return Err(format!("can't ungroup: its {key}: argument would be lost"));
    }
    let root = typst_syntax::parse(source);
    let root = LinkedNode::new(&root);
    let node = find_node(&root, &group.range, SyntaxKind::FuncCall).ok_or("can't find the group")?;
    let block = walk::args(&node)
        .and_then(|a| a.children().filter(|c| c.kind() == SyntaxKind::CodeBlock).last())
        .ok_or("this group has no { } body to unwrap")?;
    let stmts: Vec<LinkedNode> =
        walk::block_code(&block).map(|code| code.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::Semicolon)).collect()).unwrap_or_default();
    let children: Vec<&Call> = scene.calls().filter(|c| c.parent == Some(group.id)).collect();

    let after = node.parent().is_some_and(|code| code.children().any(|s| s.offset() > group.range.start && !is_trivia(s.kind()) && s.kind() != SyntaxKind::LetBinding));
    if let Some(c) = children.iter().find(|c| changes_state(c)).filter(|_| after) {
        return Err(format!("can't ungroup: its {}(..) would apply to the shapes after the group", base_name(&c.callee)));
    }
    let names: Vec<String> = children.iter().filter_map(|c| element_name(c)).collect();
    let (body, calls) = scene.block_of(group.id).ok_or("group is outside a canvas")?;
    let outside = calls.iter().filter(|c| c.parent == group.parent && c.id != group.id).filter_map(|c| element_name(c));
    if let Some(clash) = outside.chain(released.iter().cloned()).find(|n| names.contains(n)) {
        return Err(format!("can't ungroup: another shape is already named \"{clash}\""));
    }
    released.extend(names.iter().cloned());
    // `"g.r.east"` becomes `"r.east"`; the group's own anchors have no stand-in.
    if let Some(g) = &group.name {
        let within = |r: &Range<usize>| group.range.start <= r.start && r.end <= group.range.end;
        let mut strs = Vec::new();
        if let Some(body) = find_node(&root, &body, SyntaxKind::CodeBlock).or_else(|| find_node(&root, &body, SyntaxKind::FuncCall)) {
            strings(&body, &mut strs);
        }
        for (range, value) in strs.into_iter().filter(|(r, _)| !within(r)) {
            let Some(prefix) = path_from(scene, group.parent, &range) else { continue };
            let Some(rest) = value.strip_prefix(prefix.as_str()).filter(|rest| refers(rest, g)) else { continue };
            match rest[g.len()..].strip_prefix('.').filter(|child| names.iter().any(|n| refers(child, n))) {
                Some(child) => patches.push(patch(range, format!("{:?}", format!("{prefix}{child}")))),
                None => return Err(format!("can't ungroup: \"{value}\" uses the group's own anchors")),
            }
        }
    }

    if let Some((scope, transforms, _)) = wrapper_of(scene, group) {
        // A group turned by the transform handles: each statement gets its own
        // copy of the scope, so its shapes come apart and stay turned. A
        // `let` or a style would stop reaching the statements after it.
        let plain = stmts.iter().all(|s| matches!(s.kind(), SyntaxKind::FuncCall | SyntaxKind::LineComment | SyntaxKind::BlockComment));
        if plain && !children.iter().any(|c| changes_state(c)) && stmts.iter().any(|s| s.kind() == SyntaxKind::FuncCall) {
            let indent = indent_at(source, scope.range.start);
            let child_indent = stmts.first().map_or("", |s| indent_at(source, s.offset()));
            let rotate = transforms.iter().map(|t| &source[t.range.clone()]).collect::<Vec<_>>().join(&format!("\n{indent}  "));
            let mut text = String::new();
            for s in &stmts {
                if !text.is_empty() {
                    text.push_str(&format!("\n{indent}"));
                }
                let body = source[s.range()].replace(&format!("\n{child_indent}"), &format!("\n{indent}  "));
                if s.kind() == SyntaxKind::FuncCall {
                    created.push((patches.len(), text.len()));
                    text.push_str(&format!("scope({{\n{indent}  {rotate}\n{indent}  {body}\n{indent}}})"));
                } else {
                    text.push_str(&body);
                }
            }
            patches.push(patch(scope.range.clone(), text));
            return Ok(());
        }
    }

    let (Some(first), Some(last)) = (stmts.first(), stmts.last()) else {
        patches.push(patch(statement_range(source, &group.range), String::new()));
        return Ok(());
    };
    let line_start = source[..first.offset()].rfind('\n').map_or(0, |i| i + 1);
    let own_line = line_start > block.offset() && source[line_start..first.offset()].trim().is_empty();
    let (child_indent, group_indent) = (indent_at(source, first.offset()), indent_at(source, group.range.start));
    let dedent = |t: &str| if own_line { t.replace(&format!("\n{child_indent}"), &format!("\n{group_indent}")) } else { t.to_string() };
    let mut text = dedent(&source[first.offset()..last.range().end]);
    // A trailing `// comment` mustn't swallow what follows the group on its line.
    let rest_of_line = source[group.range.end..].split('\n').next().unwrap_or_default();
    if last.kind() == SyntaxKind::LineComment && !rest_of_line.trim().is_empty() {
        text.push_str(&format!("\n{group_indent}"));
    }
    for s in stmts.iter().filter(|s| s.kind() == SyntaxKind::FuncCall) {
        created.push((patches.len(), dedent(&source[first.offset()..s.offset()]).len()));
    }
    patches.push(patch(group.range.clone(), text));
    Ok(())
}

/// A `scope({ rotate(..); scale(..); shape })` the transform handles wrap a
/// shape in (either transform may be missing, not both): the scope, its
/// transforms in order, and the shape.
fn wrapped<'a>(scene: &'a Scene, scope: &Call) -> Option<(&'a Call, Vec<&'a Call>, &'a Call)> {
    let scope = scene.call(scope.id).filter(|s| base_name(&s.callee) == "scope" && !s.in_loop)?;
    let mut children: Vec<&Call> = scene.calls().filter(|c| c.parent == Some(scope.id)).collect();
    children.sort_by_key(|c| c.id);
    let (&shape, transforms) = children.split_last()?;
    let kinds: Vec<&str> = transforms.iter().map(|t| base_name(&t.callee)).collect();
    let ok = matches!(kinds[..], ["rotate"] | ["scale"] | ["rotate", "scale"]) && !changes_state(shape);
    ok.then(|| (scope, transforms.to_vec(), shape))
}

/// The wrapper scope `call` is the shape of, if it's in one.
fn wrapper_of<'a>(scene: &'a Scene, call: &Call) -> Option<(&'a Call, Vec<&'a Call>, &'a Call)> {
    wrapped(scene, scene.call(call.parent?)?).filter(|(_, _, shape)| shape.id == call.id)
}

/// Sets the `kind` transform (`"rotate"` or `"scale"`) of a call to `value`
/// (`30deg`, `1.5`), or with `None` removes it, in the call's wrapper scope,
/// which it creates or unwraps as needed. `call` is the shape or its scope.
#[allow(clippy::too_many_arguments)]
fn set_transform(
    source: &str,
    scene: &Scene,
    call: usize,
    kind: &str,
    value: Option<String>,
    (x, y): (f64, f64),
    patches: &mut Vec<Patch>,
    created: &mut Vec<(usize, usize)>,
) -> Result<(), String> {
    let call = find_call(scene, call)?;
    let wrapper = wrapped(scene, call).or_else(|| wrapper_of(scene, call));
    let statement = |value: &str| format!("{kind}({value}, origin: ({}, {}))", num(x), num(y));
    match (wrapper, value) {
        (None, None) => {}
        (None, Some(value)) => {
            if call.in_loop {
                return Err(format!("can't {kind} a shape inside a loop"));
            }
            let indent = indent_at(source, call.range.start);
            let line_start = source[..call.range.start].rfind('\n').map_or(0, |i| i + 1);
            let text = &source[call.range.clone()];
            let text = if source[line_start..call.range.start].trim().is_empty() {
                format!("scope({{\n{indent}  {}\n{indent}  {}\n{indent}}})", statement(&value), text.replace('\n', "\n  "))
            } else {
                format!("scope({{ {}; {text} }})", statement(&value))
            };
            created.push((patches.len(), 0));
            patches.push(patch(call.range.clone(), text));
        }
        (Some((scope, transforms, shape)), value) => {
            let existing = transforms.iter().find(|t| base_name(&t.callee) == kind);
            match (existing, value) {
                (Some(t), Some(value)) => {
                    let arg = t.args.iter().find(|a| a.key.is_none()).ok_or(format!("its {kind}(..) has no amount to change"))?;
                    patches.push(patch(arg.value_range.clone(), value));
                }
                (None, Some(value)) => {
                    // A rotate goes first; a scale after any rotate.
                    let next = if kind == "rotate" { transforms[0] } else { transforms.iter().find(|t| base_name(&t.callee) != "rotate").copied().unwrap_or(shape) };
                    let sep = if source[..next.range.start].ends_with(['{', ' ', ';']) && !indent_at(source, next.range.start).is_empty() { format!("\n{}", indent_at(source, next.range.start)) } else { "; ".into() };
                    patches.push(patch(next.range.start..next.range.start, format!("{}{sep}", statement(&value))));
                }
                (Some(t), None) if transforms.len() > 1 => patches.push(patch(statement_range(source, &t.range), String::new())),
                (Some(_), None) => {
                    // The last transform: the shape takes the scope's place.
                    // Anything else in its body (a comment) would be lost.
                    let root = typst_syntax::parse(source);
                    let root = LinkedNode::new(&root);
                    let count = find_node(&root, &scope.range, SyntaxKind::FuncCall)
                        .and_then(|node| walk::args(&node))
                        .and_then(|args| args.children().find(|c| c.kind() == SyntaxKind::CodeBlock))
                        .and_then(|block| walk::block_code(&block).map(|code| code.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::Semicolon)).count()));
                    if count != Some(2) {
                        return Err("can't unwrap: the scope holds more than its transform and shape".into());
                    }
                    let text = source[shape.range.clone()].replace(&format!("\n{}", indent_at(source, shape.range.start)), &format!("\n{}", indent_at(source, scope.range.start)));
                    created.push((patches.len(), 0));
                    patches.push(patch(scope.range.clone(), text));
                }
                (None, None) => {}
            }
        }
    }
    Ok(())
}

/// Calls that draw nothing, so passing them doesn't change what's in front.
const INVISIBLE: &[&str] = &["anchor", "copy-anchors", "register-mark", "get-ctx", "hide"];

/// A statement of a block, as `Arrange` and `Reorder` move it.
struct Stmt<'a> {
    node: LinkedNode<'a>,
    /// What moves: the statement, with the comments on and above its line
    /// when every statement in the block has a line of its own.
    text: Range<usize>,
    label: String,
    /// Element names and variables it defines.
    names: Vec<String>,
    vars: Vec<String>,
    /// An import or a rule: nothing moves past it.
    fixed: bool,
    /// A style or transform: it applies to what comes after it.
    state: bool,
    draws: bool,
    is_let: bool,
}

/// The statements of the block the calls are in (they must share one) and
/// which of them are the calls.
fn block_stmts<'a>(source: &str, scene: &Scene, root: &LinkedNode<'a>, calls: &[&Call]) -> Result<(Vec<Stmt<'a>>, Vec<usize>), String> {
    let mut code: Option<LinkedNode> = None;
    for c in calls {
        let node = find_node(root, &c.range, SyntaxKind::FuncCall).ok_or("can't find the shape")?;
        let parent = node
            .parent()
            .filter(|p| p.kind() == SyntaxKind::Code)
            .ok_or_else(|| format!("can't reorder {}(..): it's part of another expression", base_name(&c.callee)))?;
        match &code {
            Some(k) if k.range() != parent.range() => return Err("can only reorder shapes within the same block".into()),
            Some(_) => {}
            None => code = Some(parent.clone()),
        }
    }
    let stmts = code_stmts(source, scene, &code.ok_or("nothing to reorder")?);
    let selected = calls.iter().filter_map(|c| stmts.iter().position(|s| s.node.range() == c.range)).collect();
    Ok((stmts, selected))
}

fn code_stmts<'a>(source: &str, scene: &Scene, code: &LinkedNode<'a>) -> Vec<Stmt<'a>> {
    let nodes: Vec<LinkedNode> = code.children().filter(|c| !is_trivia(c.kind())).collect();
    let own_lines = nodes.iter().all(|n| own_line(source, &n.range()));
    nodes.into_iter().map(|node| stmt(source, scene, node, own_lines, code.offset())).collect()
}

/// The source with each block's anchors gathered (see `Edit::GatherAnchors`).
fn gather_anchors(source: &str) -> Result<String, String> {
    let mut text = source.to_string();
    // A block at a time, re-parsing after each since moves shift offsets.
    'blocks: loop {
        let scene = scene::parse(&text);
        let root = typst_syntax::parse(&text);
        let root = LinkedNode::new(&root);
        let mut blocks = Vec::new();
        for canvas in &scene.canvases {
            if let Some(body) = find_node(&root, &canvas.body, SyntaxKind::CodeBlock) {
                code_blocks(&body, &mut blocks);
            }
        }
        for code in blocks {
            let stmts = code_stmts(&text, &scene, &code);
            let order = gathered(&scene, &stmts);
            if order.iter().enumerate().any(|(i, &o)| i != o) {
                let (mut patches, mut created) = (Vec::new(), Vec::new());
                restack(&text, &stmts, &order, &[], &mut patches, &mut created);
                text = finish(&text, patches, vec![])?.source;
                continue 'blocks;
            }
        }
        break;
    }
    if text == source {
        return Err("the anchors are already at the top".into());
    }
    Ok(text)
}

/// Every `Code` node in the tree: block bodies, nested ones included.
fn code_blocks<'a>(node: &LinkedNode<'a>, out: &mut Vec<LinkedNode<'a>>) {
    if node.kind() == SyntaxKind::Code {
        out.push(node.clone());
    }
    for child in node.children() {
        code_blocks(&child, out);
    }
}

/// The block's order with its anchor statements moved up in turn, each as
/// far as it can go past statements that draw.
fn gathered(scene: &Scene, stmts: &[Stmt]) -> Vec<usize> {
    let is_anchor = |s: &Stmt| {
        let calls = top_calls(scene, &s.node.range());
        !s.is_let && !calls.is_empty() && calls.iter().all(|c| matches!(base_name(&c.callee), "anchor" | "copy-anchors"))
    };
    let mut order: Vec<usize> = (0..stmts.len()).collect();
    for s in (0..stmts.len()).filter(|&s| is_anchor(&stmts[s])) {
        let at = order.iter().position(|&o| o == s).unwrap();
        let mut dest = at;
        for k in (0..at).rev() {
            if conflict(&stmts[s], &stmts[order[k]]).is_some() {
                break;
            }
            if stmts[order[k]].draws {
                dest = k;
            }
        }
        order.remove(at);
        order.insert(dest, s);
    }
    order
}

/// Whether the statement at `range` has its line to itself, but for a `;` and
/// a trailing `// comment`.
fn own_line(source: &str, range: &Range<usize>) -> bool {
    let line_start = source[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[range.end..].find('\n').map_or(source.len(), |i| range.end + i);
    let after = source[range.end..line_end].trim_start().trim_start_matches(';').trim_start();
    source[line_start..range.start].trim().is_empty() && (after.is_empty() || after.starts_with("//"))
}

fn stmt<'a>(source: &str, scene: &Scene, node: LinkedNode<'a>, own_lines: bool, block_start: usize) -> Stmt<'a> {
    let range = node.range();
    let mut text = range.clone();
    if own_lines {
        // `// comment` lines right above it, and one after it on its line.
        loop {
            let line_start = source[..text.start].rfind('\n').map_or(0, |i| i + 1);
            let Some(prev_start) = line_start.checked_sub(1).map(|end| source[..end].rfind('\n').map_or(0, |i| i + 1)) else { break };
            let prev = &source[prev_start..line_start - 1];
            if prev_start <= block_start || !prev.trim_start().starts_with("//") {
                break;
            }
            text.start = prev_start + prev.len() - prev.trim_start().len();
        }
        let line_end = source[range.end..].find('\n').map_or(source.len(), |i| range.end + i);
        if source[range.end..line_end].contains("//") {
            text.end = line_end;
        }
    }
    let kind = node.kind();
    let is_let = kind == SyntaxKind::LetBinding;
    let calls = top_calls(scene, &range);
    let call = calls.iter().find(|c| c.range == range);
    let label = match (kind, call) {
        (SyntaxKind::FuncCall, Some(c)) => match (base_name(&c.callee), element_name(c)) {
            ("anchor", Some(name)) => format!("anchor({name:?})"),
            (base, Some(name)) => format!("{base}(name: {name:?})"),
            (base, None) => format!("{base}(..)"),
        },
        (SyntaxKind::ForLoop | SyntaxKind::WhileLoop, _) => "the loop".into(),
        (SyntaxKind::Conditional, _) => "the if".into(),
        _ => source[range.clone()].split(['(', '=', '{', '\n']).next().unwrap_or_default().trim().to_string(),
    };
    let fixed = matches!(kind, SyntaxKind::ModuleImport | SyntaxKind::ModuleInclude | SyntaxKind::SetRule | SyntaxKind::ShowRule);
    Stmt {
        text,
        label,
        names: if is_let { vec![] } else { calls.iter().filter_map(|c| element_name(c)).collect() },
        vars: if is_let { let_names(&node) } else { vec![] },
        fixed,
        state: !is_let && calls.iter().any(|c| changes_state(c)),
        // A loop of `anchor(k, p)` draws nothing either.
        draws: !is_let && !fixed && (calls.is_empty() || calls.iter().any(|c| !INVISIBLE.contains(&base_name(&c.callee)) && !changes_state(c))),
        is_let,
        node,
    }
}

/// Why `a` can't change places with `b`, if it can't.
fn conflict(a: &Stmt, b: &Stmt) -> Option<String> {
    let past = format!("can't move {} past {}", a.label, b.label);
    if a.fixed || b.fixed {
        return Some(past);
    }
    if let Some(s) = [a, b].into_iter().find(|s| s.state).filter(|_| !a.is_let && !b.is_let) {
        return Some(format!("{past}: it would change what {} applies to", s.label));
    }
    for (user, other) in [(a, b), (b, a)] {
        if let Some(used) = uses_any(&user.node, &other.names, &other.vars) {
            return Some(format!("{past}: {} uses {used}", user.label));
        }
    }
    None
}

/// The block's new order for `Edit::Arrange`, as statement indices.
fn arrange(stmts: &[Stmt], selected: &[usize], to: Layer) -> Result<Vec<usize>, String> {
    let mut order: Vec<usize> = (0..stmts.len()).collect();
    let forward = matches!(to, Layer::Forward | Layer::Front);
    let one_step = matches!(to, Layer::Forward | Layer::Backward);
    let mut moving = selected.to_vec();
    moving.sort();
    // The one nearest where they're going goes first, so the rest don't hop
    // over it.
    if forward {
        moving.reverse();
    }
    let (mut moved, mut blocked) = (false, None);
    for s in moving {
        let at = order.iter().position(|&o| o == s).unwrap();
        // What moves is a run `trial[lo..=hi]`: the statement, plus anything
        // drawing nothing that it's tied to (an anchor it uses), carried along.
        let mut trial = order.clone();
        let (mut lo, mut hi) = (at, at);
        let mut best = None;
        loop {
            let next = if forward { hi + 1 } else { lo.wrapping_sub(1) };
            let Some(&other) = trial.get(next).filter(|o| !selected.contains(o)) else { break };
            match trial[lo..=hi].iter().find_map(|&m| conflict(&stmts[m], &stmts[other])) {
                None => {
                    trial.remove(next);
                    trial.insert(if forward { lo } else { hi }, other);
                    (lo, hi) = if forward { (lo + 1, hi + 1) } else { (lo - 1, hi - 1) };
                    // Only passing something drawn changes what's in front.
                    if stmts[other].draws {
                        best = Some(trial.clone());
                        if one_step {
                            break;
                        }
                    }
                }
                Some(_) if !stmts[other].draws && !stmts[other].fixed && !stmts[other].state => {
                    (lo, hi) = if forward { (lo, hi + 1) } else { (lo - 1, hi) };
                }
                Some(why) => {
                    // With nothing drawn beyond it, it's as far as it can show.
                    let beyond = if forward { &trial[next..] } else { &trial[..=next] };
                    if beyond.iter().any(|&o| stmts[o].draws) {
                        blocked.get_or_insert(why);
                    }
                    break;
                }
            }
        }
        if let Some(best) = best {
            order = best;
            moved = true;
        }
    }
    if !moved {
        return Err(blocked.unwrap_or_else(|| format!("already at the {}", if forward { "front" } else { "back" })));
    }
    Ok(order)
}

/// The block's new order for `Edit::Reorder`, as statement indices.
fn reorder(stmts: &[Stmt], selected: &[usize], target: usize, after: bool) -> Result<Vec<usize>, String> {
    let n = stmts.len();
    if selected.contains(&target) {
        return Ok((0..n).collect());
    }
    let mut order: Vec<usize> = (0..n).filter(|i| !selected.contains(i)).collect();
    let at = order.iter().position(|&o| o == target).unwrap() + usize::from(after);
    let mut moving = selected.to_vec();
    moving.sort();
    order.splice(at..at, moving);
    let pos = |s: usize| order.iter().position(|&o| o == s).unwrap();
    for &s in selected {
        for u in (0..n).filter(|u| !selected.contains(u) && (s < *u) != (pos(s) < pos(*u))) {
            if let Some(why) = conflict(&stmts[s], &stmts[u]) {
                return Err(why);
            }
        }
    }
    Ok(order)
}

/// Rewrites the block in `order`, leaving the space between statements where
/// it was. `created` gets the selected statements.
fn restack(source: &str, stmts: &[Stmt], order: &[usize], selected: &[usize], patches: &mut Vec<Patch>, created: &mut Vec<(usize, usize)>) {
    let changed: Vec<usize> = (0..order.len()).filter(|&k| order[k] != k).collect();
    let (Some(&first), Some(&last)) = (changed.first(), changed.last()) else { return };
    let mut text = String::new();
    for (slot, &s) in order.iter().enumerate().take(last + 1).skip(first) {
        if slot > first {
            text.push_str(&source[stmts[slot - 1].text.end..stmts[slot].text.start]);
        }
        if selected.contains(&s) {
            created.push((patches.len(), text.len() + stmts[s].node.offset() - stmts[s].text.start));
        }
        text.push_str(&source[stmts[s].text.clone()]);
    }
    patches.push(patch(stmts[first].text.start..stmts[last].text.end, text));
    // Selected statements that stay put stay selected, through empty patches.
    for &s in selected.iter().filter(|&&s| s < first || s > last) {
        let at = stmts[s].node.offset();
        created.push((patches.len(), 0));
        patches.push(patch(at..at, String::new()));
    }
}

fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::Space | SyntaxKind::Semicolon | SyntaxKind::LineComment | SyntaxKind::BlockComment)
}

/// The name other calls use for the element a call draws: its `name:`, or
/// the name an `anchor("A", ..)` defines.
fn element_name(call: &Call) -> Option<String> {
    match (base_name(&call.callee), call.args.first().map(|a| &a.value)) {
        ("anchor", Some(Value::Str { value })) => Some(value.clone()),
        _ => call.name.clone(),
    }
}

/// Names of the elements drawn by the statement at `range` (not those nested
/// in its groups, which are reached through the group's name).
fn defined_names(scene: &Scene, range: &Range<usize>) -> Vec<String> {
    top_calls(scene, range).iter().filter_map(|c| element_name(c)).collect()
}

/// The calls in `range` that aren't nested in another call there.
fn top_calls<'s>(scene: &'s Scene, range: &Range<usize>) -> Vec<&'s Call> {
    let calls: Vec<&Call> =
        scene.calls().filter(|c| range.start <= c.range.start && c.range.end <= range.end).collect();
    calls.iter().filter(|c| !c.parent.is_some_and(|p| calls.iter().any(|o| o.id == p))).copied().collect()
}

/// The variables a `let` binds.
fn let_names(binding: &LinkedNode) -> Vec<String> {
    fn idents(node: &LinkedNode, out: &mut Vec<String>) {
        if node.kind() == SyntaxKind::Ident {
            out.push(node.get().leaf_text().to_string());
        }
        for child in node.children() {
            idents(&child, out);
        }
    }
    let mut out = Vec::new();
    for child in binding.children() {
        match child.kind() {
            SyntaxKind::Eq => break,
            // `let f(x) = ..`: just the function's name.
            SyntaxKind::Closure => out.extend(child.children().find(|c| c.kind() == SyntaxKind::Ident).map(|c| c.get().leaf_text().to_string())),
            _ => idents(&child, &mut out),
        }
    }
    out
}

/// The first of the element names or variables the node uses, if any.
fn uses_any(node: &LinkedNode, names: &[String], vars: &[String]) -> Option<String> {
    let hit = match node.kind() {
        SyntaxKind::Str => node.get().cast::<typst_syntax::ast::Str>().and_then(|s| names.iter().find(|n| refers(&s.get(), n)).cloned()),
        SyntaxKind::Ident if !names_something(node) => vars.iter().find(|v| node.get().leaf_text() == v.as_str()).cloned(),
        _ => None,
    };
    hit.or_else(|| node.children().find_map(|c| uses_any(&c, names, vars)))
}

/// Whether an anchor reference like `"a.east"` points into the element `name`.
fn refers(value: &str, name: &str) -> bool {
    value.strip_prefix(name).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Every string literal in the node, with its value.
fn strings(node: &LinkedNode, out: &mut Vec<(Range<usize>, String)>) {
    if let Some(s) = node.get().cast::<typst_syntax::ast::Str>() {
        out.push((node.range(), s.get().to_string()));
    }
    for child in node.children() {
        strings(&child, out);
    }
}

/// How code at `at` reaches the elements in `parent`'s body: through the
/// names of the groups around them that `at` is outside of (`"g."`), or
/// `None` if one of those groups has no name. A `scope` lets its names
/// through, so it adds nothing.
fn path_from(scene: &Scene, parent: Option<usize>, at: &Range<usize>) -> Option<String> {
    let mut path = Vec::new();
    let mut next = parent.and_then(|id| scene.call(id));
    while let Some(call) = next {
        if call.range.start <= at.start && at.end <= call.range.end {
            break;
        }
        if base_name(&call.callee) != "scope" {
            path.push(call.name.clone()?);
        }
        next = call.parent.and_then(|id| scene.call(id));
    }
    Some(path.iter().rev().map(|n| format!("{n}.")).collect())
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
    fn rotate_wraps_the_call_in_a_scope() {
        let out = run(Edit::Rotate { call: id("content"), angle: 30.0, x: 2.0, y: 3.0 });
        assert!(out.source.contains("  scope({\n    rotate(30deg, origin: (2, 3))\n    content((2, 3), [Hi])\n  })"), "{}", out.source);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 6], "scope(");
        assert!(apply(SRC, &Edit::Rotate { call: id("circle"), angle: 30.0, x: 0.0, y: 0.0 }).is_err());
    }

    #[test]
    fn ungroup_splits_a_rotated_group_into_rotated_shapes() {
        let src = "#import \"@preview/cetz:0.5.2\": canvas, draw\n#canvas({\n  import draw: *\n  scope({\n    rotate(30deg, origin: (4, 1))\n    group(name: \"g\", {\n      circle((3, 1))\n      // the box\n      rect((5, 0), (6, 1), name: \"r\")\n    })\n  })\n  line(\"g.r.east\", (8, 0))\n})";
        let out = apply(src, &Edit::Ungroup { calls: vec![src.find("group(").unwrap()] }).unwrap();
        let want = "  scope({\n    rotate(30deg, origin: (4, 1))\n    circle((3, 1))\n  })\n  // the box\n  scope({\n    rotate(30deg, origin: (4, 1))\n    rect((5, 0), (6, 1), name: \"r\")\n  })\n  line(\"r.east\", (8, 0))";
        assert!(out.source.contains(want), "{}", out.source);
        let created: Vec<&str> = out.created.iter().map(|&at| &out.source[at..at + 6]).collect();
        assert_eq!(created, ["scope(", "scope("]);

        // A style inside would stop reaching the shapes after it: keep one scope.
        let styled = src.replace("circle((3, 1))", "set-style(fill: red)\n      circle((3, 1))");
        let out = apply(&styled, &Edit::Ungroup { calls: vec![styled.find("group(").unwrap()] }).unwrap();
        assert!(out.source.contains("  scope({\n    rotate(30deg, origin: (4, 1))\n    set-style(fill: red)\n    circle((3, 1))"), "{}", out.source);
    }

    #[test]
    fn unrotate_undoes_rotate() {
        let wrapped = run(Edit::Rotate { call: id("content"), angle: 30.0, x: 2.0, y: 3.0 }).source;
        let out = apply(&wrapped, &Edit::Unrotate { call: wrapped.find("scope(").unwrap() }).unwrap();
        assert_eq!(out.source, SRC);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 8], "content(");

        let src = "#import \"@preview/cetz:0.5.2\": canvas, draw\n#canvas({\n  import draw: *\n  scope({\n    rotate(30deg)\n    // a comment\n    rect((0, 0), (1, 1))\n  })\n})";
        assert!(apply(src, &Edit::Unrotate { call: src.find("scope(").unwrap() }).is_err());
    }

    #[test]
    fn scale_shares_the_rotation_scope() {
        let rotated = run(Edit::Rotate { call: id("content"), angle: 30.0, x: 2.0, y: 3.0 }).source;
        let scope = rotated.find("scope(").unwrap();
        let scaled = apply(&rotated, &Edit::Scale { call: scope, factor: 2.0, x: 2.0, y: 3.0 }).unwrap().source;
        assert!(scaled.contains("  scope({\n    rotate(30deg, origin: (2, 3))\n    scale(2, origin: (2, 3))\n    content((2, 3), [Hi])\n  })"), "{scaled}");
        let again = apply(&scaled, &Edit::Scale { call: scope, factor: 1.5, x: 2.0, y: 3.0 }).unwrap().source;
        assert!(again.contains("scale(1.5, origin: (2, 3))"), "{again}");
        // Turning back to 0° keeps the scale, and scale 1 then unwraps.
        let unturned = apply(&again, &Edit::Unrotate { call: scope }).unwrap().source;
        assert!(unturned.contains("  scope({\n    scale(1.5, origin: (2, 3))\n    content((2, 3), [Hi])\n  })"), "{unturned}");
        let plain = apply(&unturned, &Edit::Scale { call: scope, factor: 1.0, x: 2.0, y: 3.0 }).unwrap();
        assert_eq!(plain.source, SRC);
        // A rotation added to a scaled shape goes first.
        let both = apply(&unturned, &Edit::Rotate { call: scope, angle: 45.0, x: 2.0, y: 3.0 }).unwrap().source;
        assert!(both.contains("  scope({\n    rotate(45deg, origin: (2, 3))\n    scale(1.5, origin: (2, 3))\n"), "{both}");
    }

    #[test]
    fn move_carries_a_rotation_origin() {
        let src = "#import \"@preview/cetz:0.5.2\": canvas, draw\n#canvas({\n  import draw: *\n  scope({\n    rotate(30deg, origin: (1, 1))\n    scale(2, origin: (1, 1))\n    rect((0, 0), (2, 2))\n  })\n})";
        let out = apply(src, &Edit::Move { calls: vec![src.find("scope(").unwrap()], dx: 1.0, dy: 0.5, detach: false }).unwrap();
        assert!(out.source.contains("rotate(30deg, origin: (2, 1.5))") && out.source.contains("scale(2, origin: (2, 1.5))"), "{}", out.source);
        assert!(out.source.contains("rect((1, 0.5), (3, 2.5))"), "{}", out.source);
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
    fn insert_args_adds_vertices_anywhere() {
        let line = id("line");
        let texts = vec!["(9, 9)".to_string(), "\"B\"".to_string()];
        let out = run(Edit::InsertArgs { call: line, at: 0, texts: texts.clone() });
        assert!(out.source.contains(r#"line((9, 9), "B", (0, 0), (1.5,-2)"#), "{}", out.source);
        let out = run(Edit::InsertArgs { call: line, at: 3, texts: texts.clone() });
        assert!(out.source.contains(r#""a.east", (9, 9), "B", stroke: red"#), "{}", out.source);
        let src = "#canvas({ line((0, 0), (1, 1)) })";
        let out = apply(src, &Edit::InsertArgs { call: 10, at: 2, texts }).unwrap();
        assert_eq!(out.source, r#"#canvas({ line((0, 0), (1, 1), (9, 9), "B") })"#);
    }

    #[test]
    fn remove_arg_keeps_the_commas_right() {
        let line = id("line");
        let out = run(Edit::RemoveArg { call: line, arg: 1, keep: 2 });
        assert!(out.source.contains(r#"line((0, 0), "a.east", stroke"#), "{}", out.source);
        let out = run(Edit::RemoveArg { call: line, arg: 0, keep: 2 });
        assert!(out.source.contains(r#"line((1.5,-2), "a.east""#), "{}", out.source);
        assert!(apply(SRC, &Edit::RemoveArg { call: line, arg: 0, keep: 3 }).is_err());
        assert!(apply(SRC, &Edit::RemoveArg { call: line, arg: 3, keep: 2 }).is_err());
        let src = "#canvas({ line((0, 0), (1, 1), (2, 2)) })";
        let out = apply(src, &Edit::RemoveArg { call: 10, arg: 2, keep: 2 }).unwrap();
        assert_eq!(out.source, "#canvas({ line((0, 0), (1, 1)) })");
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
    fn insert_library_imports_and_names_the_library() {
        let run = |src: &str| apply(src, &Edit::InsertLibrary { canvas: None, module: "decorations".into(), text: "brace((0, 0), (2, 0))".into() });
        let out = run(SRC).unwrap();
        assert!(out.source.starts_with("#import \"@preview/cetz:0.5.2\": canvas, draw, decorations\n"), "{}", out.source);
        assert!(out.source.contains("  decorations.brace((0, 0), (2, 0))\n})"), "{}", out.source);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 17], "decorations.brace");
        // Already imported, or everything is.
        let again = run(&out.source).unwrap();
        assert!(again.source.starts_with("#import \"@preview/cetz:0.5.2\": canvas, draw, decorations\n"));
        let star = SRC.replace(": canvas, draw", ": *");
        assert!(run(&star).unwrap().source.starts_with("#import \"@preview/cetz:0.5.2\": *\n"));
        // A bare import goes through the package's name.
        let bare = SRC.replace("#import \"@preview/cetz:0.5.2\": canvas, draw", "#import \"@preview/cetz:0.5.2\"").replace("#canvas", "#cetz.canvas");
        assert!(run(&bare).unwrap().source.contains("cetz.decorations.brace("), "{}", run(&bare).unwrap().source);
        assert!(run("#canvas({ line((0,0), (1,1)) })").is_err());
    }

    #[test]
    fn batch_combines_edits() {
        let out = run(Edit::Batch {
            edits: vec![
                Edit::SetCoord { call: id("line"), arg: 0, x: 1.0, y: 2.0 },
                Edit::Move { calls: vec![id("content")], dx: 1.0, dy: 0.0, detach: false },
                Edit::Move { calls: vec![id("content")], dx: 1.0, dy: 0.0, detach: false },
            ],
        });
        assert!(out.source.contains("line((1, 2), (1.5,-2)") && out.source.contains("content((3, 3), [Hi])"), "{}", out.source);
        let clash = Edit::Batch {
            edits: vec![
                Edit::SetCoord { call: id("line"), arg: 0, x: 1.0, y: 2.0 },
                Edit::SetCoord { call: id("line"), arg: 0, x: 3.0, y: 2.0 },
            ],
        };
        assert!(apply(SRC, &clash).is_err());
    }

    #[test]
    fn paste_appends_moved_copies_with_fresh_names() {
        let text = "rect((0, 0), (1, 1), name: \"box\")\nline(\"box.east\", (3, 0))\nscope({\n  rotate(30deg, origin: (1, 1))\n  circle((1, 1), name: \"c\")\n  anchor(\"x\", (2, 2))\n})\nline(\"x\", (4, 4))";
        let src = "#import \"@preview/cetz:0.5.2\": canvas, draw\n#canvas({\n  import draw: *\n  rect((0, 0), (1, 1), name: \"box\")\n})\n";
        let out = apply(src, &Edit::Paste { canvas: None, text: text.into(), dx: 0.5, dy: -0.5 }).unwrap();
        let want = "  rect((0.5, -0.5), (1.5, 0.5), name: \"box-2\")\n  line(\"box-2.east\", (3.5, -0.5))\n  scope({\n    rotate(30deg, origin: (1.5, 0.5))\n    circle((1.5, 0.5), name: \"c\")\n    anchor(\"x\", (2.5, 1.5))\n  })\n  line(\"x\", (4.5, 3.5))\n})";
        assert!(out.source.ends_with(&format!("{want}\n")), "{}", out.source);
        let created: Vec<&str> = out.created.iter().map(|&at| &out.source[at..at + 5]).collect();
        assert_eq!(created, ["rect(", "line(", "scope", "line("]);
        assert!(apply(src, &Edit::Paste { canvas: None, text: "let x = 1".into(), dx: 0.0, dy: 0.0 }).is_err());
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

    #[test]
    fn edits_reach_into_drawing_functions() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#let plate(x) = {
  rect((x, 0), (x + 1, 5), stroke: red)
}
#canvas({
  import draw: *
  let face(..a) = line(..a.pos(), close: true)
  plate(0)
  plate(3)
  face((0, 0), (1, 0), (0, 1))
})"#;
        let scene = crate::parse(src);
        let rect = scene.functions[0].calls[0].id;
        let out = apply(src, &Edit::SetNamed { call: rect, key: "stroke".into(), text: Some("blue".into()) }).unwrap();
        assert!(out.source.contains("rect((x, 0), (x + 1, 5), stroke: blue)"), "{}", out.source);

        // A function that is one call keeps a body when it's deleted or copied.
        let line = scene.functions[1].calls[0].id;
        let out = apply(src, &Edit::Delete { calls: vec![line] }).unwrap();
        assert!(out.source.contains("let face(..a) = {}\n"), "{}", out.source);
        let out = apply(src, &Edit::Duplicate { calls: vec![line], dx: 1.0, dy: 0.0 }).unwrap();
        assert!(
            out.source.contains("let face(..a) = {\n    line(..a.pos(), close: true)\n    line(..a.pos(), close: true)\n  }\n"),
            "{}",
            out.source
        );
        assert_eq!(&out.source[out.created[0]..out.created[0] + 4], "line");
        assert_ne!(out.created[0], line);
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
            let count = |s: &Scene| s.calls().count();
            let total = count(&scene);
            if let Ok(out) = apply(&src, &Edit::GatherAnchors) {
                let summary = crate::summarize(&out.source);
                assert!(summary.errors.is_empty(), "gathering anchors in {path:?}: {:?}\n{}", summary.errors, out.source);
                assert_eq!(count(&crate::parse(&out.source)), total);
            }
            for call in scene.calls() {
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
                // These may refuse; when they apply, the file stays valid.
                let arrange = [Layer::Forward, Layer::Backward, Layer::Front, Layer::Back].map(|to| Edit::Arrange { calls: vec![call.id], to });
                for edit in [Edit::Group { calls: vec![call.id] }, Edit::Ungroup { calls: vec![call.id] }].into_iter().chain(arrange) {
                    if let Ok(out) = apply(&src, &edit) {
                        let summary = crate::summarize(&out.source);
                        assert!(summary.errors.is_empty(), "{edit:?} on {}: {:?}\n{}", call.callee, summary.errors, out.source);
                    }
                }
            }
        }
    }

    #[test]
    fn connect_names_the_target_when_needed() {
        let src = "#canvas({\n  import draw: *\n  circle((0, 0))\n  line((1, 1), (2, 2))\n})";
        let out = apply(src, &Edit::Connect { call: src.find("line(").unwrap(), arg: 0, target: src.find("circle(").unwrap(), anchor: "east".into() }).unwrap();
        assert!(out.source.contains(r#"circle((0, 0), name: "circle")"#), "{}", out.source);
        assert!(out.source.contains(r#"line("circle.east", (2, 2))"#), "{}", out.source);
        let out = run(Edit::Connect { call: id("content"), arg: 0, target: id("group"), anchor: "north".into() });
        assert!(out.source.contains(r#"content("g.north", [Hi])"#));
        assert!(apply(SRC, &Edit::Connect { call: id("line"), arg: 0, target: id("line"), anchor: "end".into() }).is_err());
    }

    #[test]
    fn connect_moves_a_label_after_a_shape_drawn_later() {
        let src = "#canvas({\n  import draw: *\n  content((0, 0), [Hi])\n  rect((1, 1), (2, 2))\n  scope({\n    rotate(30deg, origin: (4, 4))\n    circle((4, 4), name: \"c\")\n  })\n})";
        let content = src.find("content(").unwrap();
        let out = apply(src, &Edit::Connect { call: content, arg: 0, target: src.find("rect(").unwrap(), anchor: "east".into() }).unwrap();
        assert!(out.source.contains("  rect((1, 1), (2, 2), name: \"rect\")\n  content(\"rect.east\", [Hi])\n"), "{}", out.source);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 8], "content(");
        assert_eq!(finish(src, out.patches.clone(), vec![]).unwrap().source, out.source);
        // Inside a rotation's scope: after the scope.
        let out = apply(src, &Edit::Connect { call: content, arg: 0, target: src.find("circle(").unwrap(), anchor: "north".into() }).unwrap();
        assert!(out.source.contains("  })\n  content(\"c.north\", [Hi])\n})"), "{}", out.source);
    }

    #[test]
    fn connect_refuses_shapes_drawn_later() {
        // CeTZ resolves names in drawing order: `g` isn't known yet at the rect.
        let src = "#canvas({\n  import draw: *\n  scope({\n    rotate(30deg)\n    rect((0, 0), (1, 1))\n  })\n  group(name: \"g\", { circle((3, 1)) })\n})\n#canvas({\n  import draw: *\n  circle((0, 0))\n})";
        let rect = src.find("rect(").unwrap();
        let connect = |target: &str| apply(src, &Edit::Connect { call: rect, arg: 1, target: src.find(target).unwrap(), anchor: "south-west".into() });
        assert!(connect("group(").is_err());
        assert!(connect("scope(").is_err(), "a scope around the shape isn't drawn before it");
        assert!(connect("circle((0").is_err(), "another canvas");
        let out = apply(src, &Edit::Connect { call: src.find("circle((3").unwrap(), arg: 0, target: rect, anchor: "north".into() }).unwrap();
        assert!(out.source.contains(r#"circle("rect.north")"#), "{}", out.source);
    }

    #[test]
    fn share_point_refuses_points_defined_later() {
        let src = "#canvas({\n  import draw: *\n  line((0, 0), (1, 1))\n  rotate(30deg)\n  line((2, 2), (3, 3))\n})";
        let [first, second] = [src.find("line(").unwrap(), src.rfind("line(").unwrap()];
        // Past a transform the new anchor can't move above the first line.
        assert!(apply(src, &Edit::SharePoint { call: first, arg: 1, from: second, from_arg: 0 }).is_err());
        let out = apply(src, &Edit::SharePoint { call: second, arg: 0, from: first, from_arg: 1 }).unwrap();
        assert!(out.source.contains("anchor(\"P1\", (1, 1))\n  line((0, 0), \"P1\")"), "{}", out.source);
        // Without one, the anchor goes to the top of the block, above both.
        let src = src.replace("  rotate(30deg)\n", "");
        let [first, second] = [src.find("line(").unwrap(), src.rfind("line(").unwrap()];
        let out = apply(&src, &Edit::SharePoint { call: first, arg: 1, from: second, from_arg: 0 }).unwrap();
        assert!(out.source.contains("anchor(\"P1\", (2, 2))\n  line((0, 0), \"P1\")"), "{}", out.source);
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
    fn share_point_names_a_literal_or_reuses_a_name() {
        let src = "#canvas({\n  import draw: *\n  line((0, 0), (2, 0), (2, 2))\n  line((3, 0), (4, 1))\n})\n";
        let (a, b) = (shared_id(src, "line((0"), shared_id(src, "line((3"));
        let out = apply(src, &Edit::SharePoint { call: b, arg: 1, from: a, from_arg: 1 }).unwrap();
        assert_eq!(out.source, "#canvas({\n  import draw: *\n  anchor(\"P1\", (2, 0))\n  line((0, 0), \"P1\", (2, 2))\n  line((3, 0), \"P1\")\n})\n");
        // Already shared: the other argument just uses it too.
        let a = shared_id(&out.source, "line((0");
        let b = shared_id(&out.source, "line((3");
        let again = apply(&out.source, &Edit::SharePoint { call: b, arg: 0, from: a, from_arg: 1 }).unwrap();
        assert!(again.source.contains("line(\"P1\", \"P1\")") && !again.source.contains("P2"), "{}", again.source);
        assert!(apply(src, &Edit::SharePoint { call: a, arg: 1, from: a, from_arg: 1 }).is_err());
    }

    #[test]
    fn extract_point_inserts_an_anchor() {
        let call = shared_id(SHARED, "rect(");
        let out = apply(SHARED, &Edit::ExtractPoint { call, arg: 1, name: None }).unwrap();
        // With the other anchors at the top, not just above the rect.
        assert!(out.source.contains("{ anchor(k, p) }\n  anchor(\"P1\", (1, 2))\n  line(\"A\", \"B\")"), "{}", out.source);
        assert!(out.source.contains("  rect((0, 1), \"P1\")"), "{}", out.source);
        let scene = crate::parse(&out.source);
        let rect = scene.canvases[0].calls.iter().find(|c| c.callee == "rect").unwrap();
        assert_eq!(scene.point(rect.args[1].point.unwrap()).unwrap().path, "P1");
        // Already shared or not a literal: refused.
        assert!(apply(SHARED, &Edit::ExtractPoint { call: shared_id(SHARED, r#"line("A""#), arg: 0, name: None }).is_err());
    }

    #[test]
    fn new_anchors_stay_in_their_shape_frame_and_block() {
        // Below a transform the shape is drawn under.
        let src = "#canvas({\n  import draw: *\n  circle((0, 0))\n  rotate(30deg)\n  rect((0, 0), (1, 1))\n})\n";
        let out = apply(src, &Edit::ExtractPoint { call: shared_id(src, "rect("), arg: 1, name: None }).unwrap();
        assert!(out.source.contains("rotate(30deg)\n  anchor(\"P1\", (1, 1))\n  rect((0, 0), \"P1\")"), "{}", out.source);
        // Inside the shape's group, at its top.
        let src = "#canvas({\n  import draw: *\n  circle((0, 0))\n  group({\n    circle((1, 1))\n    rect((0, 0), (1, 1))\n  })\n})\n";
        let out = apply(src, &Edit::ExtractPoint { call: shared_id(src, "rect("), arg: 1, name: None }).unwrap();
        assert!(out.source.contains("group({\n    anchor(\"P1\", (1, 1))\n    circle((1, 1))"), "{}", out.source);
        // The point tool adds after the anchors already at the top.
        let src = "#canvas({\n  import draw: *\n  anchor(\"P1\", (0, 0))\n  rect((0, 0), \"P1\")\n})\n";
        let out = apply(src, &Edit::AddPoint { canvas: None, x: 2.0, y: 1.0, name: None }).unwrap();
        assert!(out.source.contains("anchor(\"P1\", (0, 0))\n  anchor(\"P2\", (2, 1))\n  rect("), "{}", out.source);
        assert_eq!(&out.source[out.created[0]..out.created[0] + 6], "(2, 1)");
    }

    #[test]
    fn gather_anchors_moves_them_up_in_order_within_their_frame() {
        let src = "#canvas({\n  import draw: *\n  anchor(\"A\", (0, 0))\n  rect(\"A\", (1, 1), name: \"r\")\n  // the tip\n  anchor(\"B\", (2, 2))\n  anchor(\"C\", \"r.east\")\n  line(\"A\", \"B\", \"C\")\n  rotate(30deg)\n  circle((0, 0))\n  anchor(\"D\", (1, 0))\n  group({\n    circle((0, 0))\n    anchor(\"E\", (1, 1))\n  })\n})\n";
        let out = apply(src, &Edit::GatherAnchors).unwrap();
        assert_eq!(
            out.source,
            "#canvas({\n  import draw: *\n  anchor(\"A\", (0, 0))\n  // the tip\n  anchor(\"B\", (2, 2))\n  rect(\"A\", (1, 1), name: \"r\")\n  anchor(\"C\", \"r.east\")\n  line(\"A\", \"B\", \"C\")\n  rotate(30deg)\n  anchor(\"D\", (1, 0))\n  circle((0, 0))\n  group({\n    anchor(\"E\", (1, 1))\n    circle((0, 0))\n  })\n})\n"
        );
        assert_eq!(apply(&out.source, &Edit::GatherAnchors).unwrap_err(), "the anchors are already at the top");
        // A loop of anchors stays below the dictionary it reads.
        let out = apply(SHARED, &Edit::GatherAnchors);
        assert!(out.is_err() || out.unwrap().source.contains("let pts"));
    }

    #[test]
    fn set_grid_writes_the_comment_above_the_canvas() {
        let src = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\n#canvas({\n  draw.circle((0, 0))\n})\n";
        let id = shared_id(src, "canvas({");
        assert_eq!(crate::parse(src).canvases[0].grid, None);
        let out = apply(src, &Edit::SetGrid { canvas: id, step: "0.2".into() }).unwrap();
        assert!(out.source.contains("\n\n// cetz-editor: grid 0.2\n#canvas({"), "{}", out.source);
        let scene = crate::parse(&out.source);
        assert_eq!(scene.canvases[0].grid.as_deref(), Some("0.2"));
        let out = apply(&out.source, &Edit::SetGrid { canvas: scene.canvases[0].id, step: "1/3".into() }).unwrap();
        assert!(out.source.contains("// cetz-editor: grid 1/3\n#canvas({"), "{}", out.source);
        // Found among other comments, but not across a blank line.
        let src = "// cetz-editor: grid 0.5\n// The plan\n#canvas({})\n\n// cetz-editor: grid 2\n\n#canvas({})\n";
        let grids: Vec<_> = crate::parse(src).canvases.into_iter().map(|c| c.grid).collect();
        assert_eq!(grids, [Some("0.5".to_string()), None]);
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
    fn group_wraps_calls_and_redirects_references() {
        let src = "#canvas({\n  import draw: *\n  rect((0, 0), (4, 2), name: \"box\")\n  content(\"box.center\", [Hi])\n  line(\"box.east\", (6, 1))\n})\n";
        let calls = vec![shared_id(src, "rect("), shared_id(src, "content(")];
        let out = apply(src, &Edit::Group { calls }).unwrap();
        assert_eq!(
            out.source,
            "#canvas({\n  import draw: *\n  group(name: \"group\", {\n    rect((0, 0), (4, 2), name: \"box\")\n    content(\"box.center\", [Hi])\n  })\n  line(\"group.box.east\", (6, 1))\n})\n"
        );
        let scene = crate::parse(&out.source);
        assert_eq!(scene.call(out.created[0]).unwrap().callee, "group");
        assert_eq!(scene.canvases[0].calls.iter().filter(|c| c.parent == Some(out.created[0])).count(), 2);
    }

    #[test]
    fn group_moves_later_calls_up_unless_they_depend_on_what_they_pass() {
        let src = "#canvas({\n  import draw: *\n  circle((0, 0))\n  rect((1, 1), (2, 2), name: \"r\")\n  line((0, 0), \"r.east\")\n})\n";
        let out = apply(src, &Edit::Group { calls: vec![shared_id(src, "circle("), shared_id(src, "line(")] });
        assert!(out.unwrap_err().contains("uses r"));
        let out = apply(src, &Edit::Group { calls: vec![shared_id(src, "circle("), shared_id(src, "rect(")] }).unwrap();
        assert!(out.source.contains("    circle((0, 0))\n    rect((1, 1), (2, 2), name: \"r\")\n  })\n  line((0, 0), \"group.r.east\")"), "{}", out.source);
        let skip = apply(src, &Edit::Group { calls: vec![shared_id(src, "rect("), shared_id(src, "circle(")] }).unwrap();
        assert_eq!(skip.source, out.source);
    }

    const STACK: &str = "#canvas({\n  import draw: *\n  // the box\n  rect((0, 0), (4, 2), name: \"box\")\n  anchor(\"A\", (1, 1))\n  circle((1, 1)) // dot\n  content(\"box.center\", [Hi])\n})\n";

    fn arranged(src: &str, callee: &str, to: Layer) -> Result<String, String> {
        apply(src, &Edit::Arrange { calls: vec![shared_id(src, callee)], to }).map(|out| out.source)
    }

    #[test]
    fn arrange_moves_past_one_drawn_statement_with_its_comments() {
        let out = arranged(STACK, "circle(", Layer::Backward).unwrap();
        assert_eq!(
            out,
            "#canvas({\n  import draw: *\n  circle((1, 1)) // dot\n  // the box\n  rect((0, 0), (4, 2), name: \"box\")\n  anchor(\"A\", (1, 1))\n  content(\"box.center\", [Hi])\n})\n"
        );
        // Forward passes the anchor, which draws nothing, on its way past
        // the circle; then the content, which uses the box, stops it.
        let out = arranged(STACK, "rect(", Layer::Forward).unwrap();
        assert!(out.contains("draw: *\n  anchor(\"A\", (1, 1))\n  circle((1, 1)) // dot\n  // the box\n  rect("), "{out}");
        assert_eq!(arranged(STACK, "rect(", Layer::Front).unwrap(), out);
        let err = arranged(&out, "rect(", Layer::Forward).unwrap_err();
        assert!(err.contains("content(..) uses box"), "{err}");
        let out = arranged(STACK, "anchor(", Layer::Front).unwrap();
        assert!(out.contains("content(\"box.center\", [Hi])\n  anchor(\"A\", (1, 1))\n})"), "{out}");
    }

    #[test]
    fn arrange_to_the_back_stops_at_the_import_and_reports_the_edges() {
        let out = arranged(STACK, "circle(", Layer::Back).unwrap();
        assert!(out.starts_with("#canvas({\n  import draw: *\n  circle((1, 1)) // dot\n  // the box"), "{out}");
        assert!(arranged(&out, "circle(", Layer::Back).unwrap_err().contains("already at the back"));
        assert!(arranged(STACK, "content(", Layer::Forward).unwrap_err().contains("already at the front"));
    }

    #[test]
    fn arrange_carries_the_anchors_a_shape_uses() {
        let src = "#canvas({\n  import draw: *\n  rect((0, 0), (1, 1))\n  anchor(\"P8\", (2, 2))\n  line(\"P8\", (0, 0))\n  anchor(\"Q\", \"l.end\")\n})\n";
        let out = arranged(src, "line(", Layer::Back).unwrap();
        assert!(out.contains("draw: *\n  anchor(\"P8\", (2, 2))\n  line(\"P8\", (0, 0))\n  rect((0, 0), (1, 1))\n"), "{out}");
        // Behind only the anchor it uses, it's already at the back.
        assert_eq!(arranged(&out, "line(", Layer::Back).unwrap_err(), "already at the back");
        assert_eq!(arranged(&out, "line(", Layer::Backward).unwrap_err(), "already at the back");
        // An anchor that uses what's passed stops the carry.
        let src = "#canvas({\n  import draw: *\n  rect((0, 0), (1, 1), name: \"r\")\n  anchor(\"P\", \"r.east\")\n  line(\"P\", (0, 0))\n})\n";
        let err = arranged(src, "line(", Layer::Back).unwrap_err();
        assert_eq!(err, "can't move anchor(\"P\") past rect(name: \"r\"): anchor(\"P\") uses r");
    }

    #[test]
    fn arrange_keeps_the_selection_and_moves_it_together() {
        let calls = vec![shared_id(STACK, "anchor("), shared_id(STACK, "circle(")];
        let out = apply(STACK, &Edit::Arrange { calls, to: Layer::Backward }).unwrap();
        assert!(out.source.contains("draw: *\n  anchor(\"A\", (1, 1))\n  circle((1, 1)) // dot\n  // the box\n  rect("), "{}", out.source);
        let mut picked: Vec<&str> = out.created.iter().map(|&id| &out.source[id..id + 6]).collect();
        picked.sort();
        assert_eq!(picked, ["anchor", "circle"]);
    }

    #[test]
    fn arrange_refuses_to_cross_styles_and_works_inline() {
        let src = "#canvas({ import draw: *; rect((0, 0), (1, 1)); set-style(fill: red); circle((0, 0)); line((0, 0), (1, 1)) })";
        assert!(arranged(src, "circle(", Layer::Backward).unwrap_err().contains("what set-style(..) applies to"));
        assert_eq!(
            arranged(src, "circle(", Layer::Forward).unwrap(),
            "#canvas({ import draw: *; rect((0, 0), (1, 1)); set-style(fill: red); line((0, 0), (1, 1)); circle((0, 0)) })"
        );
    }

    #[test]
    fn reorder_drops_before_or_after_a_target() {
        let (circle, rect) = (shared_id(STACK, "circle("), shared_id(STACK, "rect("));
        let out = apply(STACK, &Edit::Reorder { calls: vec![circle], target: rect, after: false }).unwrap();
        assert_eq!(out.source, arranged(STACK, "circle(", Layer::Backward).unwrap());
        assert_eq!(&out.source[out.created[0]..out.created[0] + 6], "circle");
        // Onto itself: nothing to do.
        let out = apply(STACK, &Edit::Reorder { calls: vec![circle], target: circle, after: true }).unwrap();
        assert_eq!(out.source, STACK);
        let err = apply(STACK, &Edit::Reorder { calls: vec![rect], target: shared_id(STACK, "content("), after: true }).unwrap_err();
        assert!(err.contains("uses box"), "{err}");
    }

    #[test]
    fn group_refuses_what_would_change_the_drawing() {
        // Different blocks.
        assert!(apply(SRC, &Edit::Group { calls: vec![id("rect"), id("content")] }).is_err());
        // Inside a loop.
        assert!(apply(SRC, &Edit::Group { calls: vec![id("circle")] }).is_err());
        // A transform the next shape relies on.
        let src = "#canvas({\n  import draw: *\n  rotate(10deg)\n  rect((0, 0), (1, 1))\n  circle((0, 0))\n})\n";
        assert!(apply(src, &Edit::Group { calls: vec![shared_id(src, "rotate("), shared_id(src, "rect(")] }).is_err());
        assert!(apply(src, &Edit::Group { calls: vec![shared_id(src, "rotate("), shared_id(src, "rect("), shared_id(src, "circle(")] }).is_ok());
    }

    #[test]
    fn group_inside_a_group_keeps_the_outer_path() {
        let src = "#canvas({\n  import draw: *\n  group(name: \"g\", {\n    rect((0, 0), (1, 1), name: \"r\")\n    circle((0, 0), name: \"c\")\n  })\n  line(\"g.r.east\", \"g.c\")\n})\n";
        let out = apply(src, &Edit::Group { calls: vec![shared_id(src, "rect(")] }).unwrap();
        assert!(out.source.contains("line(\"g.group.r.east\", \"g.c\")"), "{}", out.source);
        assert!(out.source.contains("    group(name: \"group\", {\n      rect((0, 0), (1, 1), name: \"r\")\n    })\n"), "{}", out.source);
    }

    #[test]
    fn group_inline_calls() {
        let src = "#canvas({ import draw: *; circle((0, 0)); rect((0, 0), (1, 1)) })";
        let out = apply(src, &Edit::Group { calls: vec![shared_id(src, "circle("), shared_id(src, "rect(")] }).unwrap();
        assert_eq!(out.source, "#canvas({ import draw: *; group(name: \"group\", { circle((0, 0)); rect((0, 0), (1, 1)) });  })");
        assert!(crate::summarize(&out.source).errors.is_empty());
    }

    #[test]
    fn ungroup_unwraps_and_redirects_references() {
        let src = "#canvas({\n  import draw: *\n  group(name: \"g\", {\n    rect((0, 0), (4, 2), name: \"box\")\n    content(\"box.center\", [Hi])\n  })\n  line(\"g.box.east\", (6, 1))\n})\n";
        let out = apply(src, &Edit::Ungroup { calls: vec![shared_id(src, "group(")] }).unwrap();
        assert_eq!(
            out.source,
            "#canvas({\n  import draw: *\n  rect((0, 0), (4, 2), name: \"box\")\n  content(\"box.center\", [Hi])\n  line(\"box.east\", (6, 1))\n})\n"
        );
        let created: Vec<&str> = out.created.iter().map(|&i| &out.source[i..i + 5]).collect();
        assert_eq!(created, ["rect(", "conte"]);
        // Group and ungroup round-trip.
        // Several at once.
        let two = "#canvas({\n  import draw: *\n  group({ circle((0, 0)) })\n  group({\n    rect((0, 0), (1, 1))\n    line((0, 0), (1, 1))\n  })\n})\n";
        let groups = crate::parse(two).canvases[0].calls.iter().filter(|c| c.callee == "group").map(|c| c.id).collect();
        let out2 = apply(two, &Edit::Ungroup { calls: groups }).unwrap();
        assert_eq!(out2.source, "#canvas({\n  import draw: *\n  circle((0, 0))\n  rect((0, 0), (1, 1))\n  line((0, 0), (1, 1))\n})\n");
        assert_eq!(out2.created.len(), 3);
        let grouped = apply(&out.source, &Edit::Group { calls: out.created.clone() }).unwrap();
        let back = apply(&grouped.source, &Edit::Ungroup { calls: grouped.created.clone() }).unwrap();
        assert_eq!(back.source, out.source);
    }

    #[test]
    fn ungroup_refuses_what_would_change_the_drawing() {
        // The group's own anchor in use.
        let src = "#canvas({\n  import draw: *\n  group(name: \"g\", { rect((0, 0), (1, 1)) })\n  line(\"g.north\", (2, 2))\n})\n";
        assert!(apply(src, &Edit::Ungroup { calls: vec![shared_id(src, "group(")] }).unwrap_err().contains("g.north"));
        // A rotation that would leak to what follows.
        assert!(apply(SRC, &Edit::Ungroup { calls: vec![id("group")] }).is_err());
        // A clashing name.
        let src = "#canvas({\n  import draw: *\n  circle((0, 0), name: \"r\")\n  group({ rect((0, 0), (1, 1), name: \"r\") })\n})\n";
        assert!(apply(src, &Edit::Ungroup { calls: vec![shared_id(src, "group(")] }).is_err());
        // Not a group.
        assert!(apply(SRC, &Edit::Ungroup { calls: vec![id("line")] }).is_err());
        // Two groups whose children share a name.
        let src = "#canvas({\n  import draw: *\n  group({ circle((0, 0), name: \"a\") })\n  group({ circle((1, 0), name: \"a\") })\n})\n";
        let groups = crate::parse(src).canvases[0].calls.iter().filter(|c| c.callee == "group").map(|c| c.id).collect();
        assert!(apply(src, &Edit::Ungroup { calls: groups }).is_err());
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

    #[test]
    fn add_connector_names_its_shapes_and_draws_after_them() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  rect((0, 0), (2, 1))
  group(name: "g", { rect((0, -3), (2, -2), name: "b") })
  rect((4, 0), (5, 1))
})
"#;
        let a = src.find("rect((0, 0)").unwrap();
        let b = src.find("rect((0, -3)").unwrap();
        let c = src.find("rect((4, 0)").unwrap();
        let edit = |from, to, route| Edit::AddConnector { canvas: None, from, from_anchor: "south".into(), to, to_anchor: "north".into(), route, detour: false, stub: None, fixed: false };
        let out = apply(src, &edit(a, b, Route::Elbow)).unwrap();
        assert!(out.source.contains(r#"rect((0, 0), (2, 1), name: "rect")"#), "{}", out.source);
        assert!(out.source.contains(r#"line("rect.south", ("rect.south", "|-", ("rect.south", 50%, "g.b.north")), ("g.b.north", "|-", ("rect.south", 50%, "g.b.north")), "g.b.north", mark: (end: ">"))"#), "{}", out.source);
        assert!(out.source[out.created[0]..].starts_with("line("));

        // Two unnamed shapes get two different names.
        let out = apply(src, &edit(a, c, Route::Straight)).unwrap();
        assert!(out.source.contains(r#"name: "rect")"#) && out.source.contains(r#"name: "rect-2")"#), "{}", out.source);
        assert!(out.source.contains(r#"line("rect.south", "rect-2.north", mark: (end: ">"))"#), "{}", out.source);

        assert!(apply(src, &edit(a, a, Route::Straight)).is_err());

        let pinned = Edit::AddConnector { canvas: None, from: a, from_anchor: "south".into(), to: c, to_anchor: "north".into(), route: Route::Straight, detour: false, stub: None, fixed: true };
        let out = apply(src, &pinned).unwrap();
        assert!(out.source.contains("  // cetz-editor: fixed\n  line(\"rect.south\", \"rect-2.north\""), "{}", out.source);
        assert!(out.source[out.created[0]..].starts_with("line("));
    }

    #[test]
    fn reconnect_moves_an_end_keeping_route_and_bend() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  rect((0, 0), (2, 1), name: "a")
  rect((3, -3), (5, -2), name: "b")
  rect((6, 0), (8, 1))
  line("a.south", ("a.south", "|-", ("a.south", 30%, "b.north")), ("b.north", "|-", ("a.south", 30%, "b.north")), "b.north", mark: (end: ">"))
  rect((0, -6), (2, -5), name: "late")
})
"#;
        let line = src.find("line(").unwrap();
        let c = src.find("rect((6, 0)").unwrap();
        let reconnect = |target: usize, to_end, from_anchor: &str, to_anchor: &str, fixed| Edit::Reconnect {
            call: line,
            to_end,
            target,
            from_anchor: from_anchor.into(),
            to_anchor: to_anchor.into(),
            fixed,
            detour: false,
            stub: None,
        };
        // To a shape drawn before it: rewritten in place, the shape named.
        let out = apply(src, &reconnect(c, true, "south", "north", false)).unwrap().source;
        assert!(out.contains(r#"rect((6, 0), (8, 1), name: "rect")"#), "{out}");
        assert!(out.contains(r#"line("a.south", ("a.south", "|-", ("a.south", 30%, "rect.north")), ("rect.north", "|-", ("a.south", 30%, "rect.north")), "rect.north", mark"#), "{out}");
        // Onto an anchor: pinned.
        let out = apply(src, &reconnect(c, false, "west", "north", true)).unwrap().source;
        assert!(out.contains("  // cetz-editor: fixed\n  line(\"rect.west\", (\"rect.west\", \"-|\", \"b.north\")"), "{out}");
        // To a shape drawn after it: moved to the end, after that shape.
        let late = src.find("rect((0, -6)").unwrap();
        let out = apply(src, &reconnect(late, true, "south", "north", true)).unwrap();
        assert!(out.source.ends_with("  rect((0, -6), (2, -5), name: \"late\")\n  // cetz-editor: fixed\n  line(\"a.south\", (\"a.south\", \"|-\", (\"a.south\", 30%, \"late.north\")), (\"late.north\", \"|-\", (\"a.south\", 30%, \"late.north\")), \"late.north\", mark: (end: \">\"))\n})\n"), "{}", out.source);
        assert!(out.source[out.created[0]..].starts_with("line("));
        assert!(apply(src, &reconnect(line, true, "south", "north", false)).is_err());
    }

    #[test]
    fn reroute_switches_between_straight_and_elbow() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  rect((0, 0), (2, 1), name: "a")
  rect((3, -3), (5, -2), name: "b")
  line("a.south", "b.north", mark: (end: ">"), stroke: red)
})
"#;
        let line = src.find("line(").unwrap();
        let scene = scene::parse(src);
        assert_eq!(scene.call(line).unwrap().connector.as_ref().map(|c| c.route), Some(Route::Straight));

        let reroute = |src: &str, route, from_anchor: Option<&str>| {
            let line = src.find("line(").unwrap();
            apply(src, &Edit::Reroute { call: line, route, from_anchor: from_anchor.map(Into::into), to_anchor: None, keep_bend: false, detour: false, stub: None }).unwrap().source
        };
        let elbow = reroute(src, Route::Elbow, None);
        let call = scene::parse(&elbow).call(line).unwrap().clone();
        assert_eq!(call.connector.map(|c| c.route), Some(Route::Elbow), "{elbow}");
        assert!(elbow.contains(r#""b.north", mark: (end: ">"), stroke: red)"#));

        let side = reroute(&elbow, Route::Elbow, Some("east"));
        assert!(side.contains(r#"line("a.east", ("a.east", "-|", "b.north"), "b.north", mark"#), "{side}");
        assert_eq!(reroute(&elbow, Route::Straight, None), src);

        // Pinning adds a comment above it; unpinning or deleting it takes the comment away.
        let fixed = apply(src, &Edit::SetFixed { call: line, fixed: true }).unwrap().source;
        assert!(fixed.contains("  // cetz-editor: fixed\n  line(\"a.south\""), "{fixed}");
        let fixed_line = fixed.find("line(").unwrap();
        assert!(scene::parse(&fixed).call(fixed_line).unwrap().connector.as_ref().unwrap().fixed);
        assert_eq!(apply(&fixed, &Edit::SetFixed { call: fixed_line, fixed: false }).unwrap().source, src);
        assert!(!apply(&fixed, &Edit::Delete { calls: vec![fixed_line] }).unwrap().source.contains("cetz-editor"));

        // Bending moves where it crosses over; rerouting puts it back halfway.
        let line_at = elbow.find("line(").unwrap();
        assert_eq!(scene::parse(&elbow).call(line_at).unwrap().connector.as_ref().unwrap().bend, Some(0.5));
        let bent = apply(&elbow, &Edit::Bend { call: line_at, ratio: 0.25 }).unwrap().source;
        assert!(bent.contains(r#"("a.south", "|-", ("a.south", 25%, "b.north")), ("b.north", "|-", ("a.south", 25%, "b.north"))"#), "{bent}");
        assert_eq!(scene::parse(&bent).call(line_at).unwrap().connector.as_ref().unwrap().bend, Some(0.25));
        assert_eq!(reroute(&bent, Route::Elbow, None), elbow);
        let kept = apply(&bent, &Edit::Reroute { call: line_at, route: Route::Elbow, from_anchor: Some("east".into()), to_anchor: None, keep_bend: true, detour: false, stub: None }).unwrap().source;
        assert!(kept.contains(r#"line("a.east", ("a.east", "-|", "b.north")"#), "an L has no bend to keep: {kept}");
        let kept = apply(&bent, &Edit::Reroute { call: line_at, route: Route::Elbow, from_anchor: None, to_anchor: None, keep_bend: true, detour: false, stub: None }).unwrap().source;
        assert_eq!(kept, bent);
        assert!(apply(src, &Edit::Bend { call: line, ratio: 0.25 }).is_err(), "a straight connector doesn't bend");

        // A detour reads back as one, bend and all, and bends as one.
        let detour = apply(src, &Edit::Reroute { call: line, route: Route::Elbow, from_anchor: None, to_anchor: None, keep_bend: false, detour: true, stub: None }).unwrap().source;
        let c = scene::parse(&detour).call(line).unwrap().connector.clone().unwrap();
        assert!(c.detour && c.route == Route::Elbow && c.bend == Some(0.5), "{detour}");
        let bent = apply(&detour, &Edit::Bend { call: line, ratio: 0.25 }).unwrap().source;
        assert!(bent.contains(r#"((rel: (0, -0.5), to: "a.south"), "-|", ("a.south", 25%, "b.north"))"#), "{bent}");
        // Its step out can change, and stays through a bend.
        let far = apply(&bent, &Edit::SetStub { call: line, stub: 1.0 }).unwrap().source;
        let c = scene::parse(&far).call(line).unwrap().connector.clone().unwrap();
        assert_eq!((c.stub, c.bend), (Some(1.0), Some(0.25)), "{far}");
        let rebent = apply(&far, &Edit::Bend { call: line, ratio: 0.5 }).unwrap().source;
        assert!(rebent.contains(r#"(rel: (0, -1), to: "a.south")"#), "{rebent}");
        assert!(apply(src, &Edit::SetStub { call: line, stub: 1.0 }).is_err(), "a plain elbow has no step out");

        let plain = src.replace(r#""a.south", "b.north""#, "(0, 0), (1, 1)");
        let line = plain.find("line(").unwrap();
        assert!(apply(&plain, &Edit::Reroute { call: line, route: Route::Elbow, from_anchor: None, to_anchor: None, keep_bend: false, detour: false, stub: None }).is_err());
    }
}
