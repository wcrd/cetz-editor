//! The editor's view of a source file: each CeTZ canvas and the draw calls in
//! its body, with their arguments and exact byte ranges. Geometry is not
//! computed here — it comes from the CeTZ probe, keyed by call offset.

use std::ops::Range;

use serde::Serialize;
use typst_syntax::{LinkedNode, SyntaxKind, ast};

use crate::points::{self, Point};
use crate::route::{self, Connector};
use crate::walk::{self, Context};

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub canvases: Vec<Canvas>,
    /// Literal points defined once and shared by reference.
    pub points: Vec<Point>,
    /// Every `let` binding, at the top level or in a canvas.
    pub variables: Vec<Variable>,
    /// Loops in canvas bodies that contain draw calls.
    pub loops: Vec<Loop>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variable {
    pub name: String,
    /// The whole `let` binding.
    pub range: Range<usize>,
    pub value_range: Range<usize>,
    pub kind: VariableKind,
    /// The value's source, shortened to one line.
    pub summary: String,
    /// The canvas the binding is in, if any.
    pub canvas: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VariableKind {
    /// A literal coordinate: `let A = (0, 0)`.
    Point,
    /// A dictionary or array holding literal coordinates.
    Points,
    Function,
    Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Loop {
    /// Byte offset of the `for`/`while`.
    pub id: usize,
    pub range: Range<usize>,
    /// `(k, p)` in `for (k, p) in pts`; empty for `while`.
    pub pattern: String,
    /// `pts` in `for (k, p) in pts`, or the `while` condition.
    pub iterable: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Canvas {
    /// Byte offset of the `canvas(...)` call.
    pub id: usize,
    /// Byte range of the body block, braces included.
    pub body: Range<usize>,
    pub calls: Vec<Call>,
    /// The editor's grid step for it, as written in a
    /// `// cetz-editor: grid 0.2` comment on a line just above.
    pub grid: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Call {
    /// Byte offset of the call: the same id the probe reports.
    pub id: usize,
    pub range: Range<usize>,
    /// The function called, as written (`line`, `draw.line`, `face`).
    pub callee: String,
    /// The `name:` argument, when it's a string literal.
    pub name: Option<String>,
    pub args: Vec<Arg>,
    /// Offset of the closing `)` of the argument list.
    pub args_close: Option<usize>,
    /// Enclosing draw call for calls inside block arguments (`group({...})`).
    pub parent: Option<usize>,
    /// Inside a loop: one call, possibly many shapes.
    pub in_loop: bool,
    /// The innermost loop around the call.
    pub loop_id: Option<usize>,
    pub conditional: bool,
    /// The connector it draws, when it's a `line` between two anchors.
    pub connector: Option<Connector>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Arg {
    /// `None` for positional arguments.
    pub key: Option<String>,
    /// Range of the whole argument, including `key: ` for named ones.
    pub range: Range<usize>,
    /// Range of the value expression.
    pub value_range: Range<usize>,
    pub text: String,
    pub value: Value,
    /// The shared point this argument uses (`A`, `pts.A`, `"A"`), if any.
    pub point: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Value {
    /// A literal 2D/3D coordinate like `(1, -2.5)`.
    Coord { x: f64, y: f64, x_range: Range<usize>, y_range: Range<usize> },
    Number { value: f64 },
    /// A string literal; also how CeTZ refers to anchors (`"a.east"`).
    Str { value: String },
    /// A content block like `[Equipment]`.
    Content { inner: Range<usize> },
    /// Anything else, kept as opaque source text.
    Expr,
}

impl Scene {
    pub fn call(&self, id: usize) -> Option<&Call> {
        self.canvases.iter().flat_map(|c| &c.calls).find(|c| c.id == id)
    }

    pub fn point(&self, id: usize) -> Option<&Point> {
        self.points.iter().find(|p| p.id == id)
    }

    pub fn canvas_of(&self, call: usize) -> Option<&Canvas> {
        self.canvases.iter().find(|c| c.calls.iter().any(|k| k.id == call))
    }
}

impl Call {
    pub fn coords(&self) -> impl Iterator<Item = (usize, &Arg)> {
        self.args
            .iter()
            .enumerate()
            .filter(|(_, a)| a.key.is_none() && matches!(a.value, Value::Coord { .. }))
    }

    pub fn named(&self, key: &str) -> Option<(usize, &Arg)> {
        self.args.iter().enumerate().find(|(_, a)| a.key.as_deref() == Some(key))
    }
}

/// The editor's grid comment for the canvas call at `canvas`: among the
/// `//` comment lines right above its line, one reading
/// `// cetz-editor: grid <step>`. Returns the step's range.
pub fn grid_comment(source: &str, canvas: usize) -> Option<Range<usize>> {
    let mut line_start = source[..canvas].rfind('\n').map_or(0, |i| i + 1);
    while line_start > 0 {
        let start = source[..line_start - 1].rfind('\n').map_or(0, |i| i + 1);
        let line = &source[start..line_start - 1];
        let comment = line.trim_start().strip_prefix("//")?;
        let step = comment.trim_start().strip_prefix("cetz-editor:").map(str::trim_start).and_then(|rest| rest.strip_prefix("grid"));
        if let Some(step) = step.filter(|s| s.starts_with(char::is_whitespace)) {
            let value = step.trim();
            let at = start + line.len() - step.trim_start().len();
            return (!value.is_empty()).then(|| at..at + value.len());
        }
        line_start = start;
    }
    None
}

/// The `// cetz-editor: fixed` comment line right above the call at
/// `call` (among the comment lines there), which pins a connector's sides:
/// the range of the whole line, its line break included.
pub fn fixed_comment(source: &str, call: usize) -> Option<Range<usize>> {
    let mut line_start = source[..call].rfind('\n').map_or(0, |i| i + 1);
    if !source[line_start..call].trim().is_empty() {
        return None;
    }
    while line_start > 0 {
        let start = source[..line_start - 1].rfind('\n').map_or(0, |i| i + 1);
        let comment = source[start..line_start - 1].trim_start().strip_prefix("//")?;
        if comment.trim_start().strip_prefix("cetz-editor:").is_some_and(|rest| rest.trim() == "fixed") {
            return Some(start..line_start);
        }
        line_start = start;
    }
    None
}

pub fn parse(source: &str) -> Scene {
    let root = typst_syntax::parse(source);
    let linked = LinkedNode::new(&root);
    let points = points::collect(&linked);
    let mut canvases = Vec::new();
    let mut loops = Vec::new();
    walk::for_each_canvas(&linked, &mut |canvas, body| {
        let mut calls = Vec::new();
        walk::for_each_call(body, Context::default(), &mut |call, ctx| calls.push(parse_call(source, &points, call, ctx)));
        let grid = grid_comment(source, canvas.offset()).map(|range| source[range].to_string());
        canvases.push(Canvas { id: canvas.offset(), body: body.range(), calls, grid });
    });
    let loop_ids: std::collections::BTreeSet<usize> = canvases.iter().flat_map(|c| &c.calls).filter_map(|c| c.loop_id).collect();
    collect_loops(source, &linked, &loop_ids, &mut loops);
    let mut variables = Vec::new();
    collect_variables(source, &linked, &canvases, &points, &mut variables);
    Scene { canvases, points, variables, loops }
}

fn collect_loops(source: &str, node: &LinkedNode, ids: &std::collections::BTreeSet<usize>, out: &mut Vec<Loop>) {
    if matches!(node.kind(), SyntaxKind::ForLoop | SyntaxKind::WhileLoop) && ids.contains(&node.offset()) {
        let parts: Vec<_> = node
            .children()
            .filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::For | SyntaxKind::In | SyntaxKind::While))
            .collect();
        let (pattern, iterable) = match (node.kind(), parts.as_slice()) {
            (SyntaxKind::ForLoop, [pattern, iterable, ..]) => (source[pattern.range()].to_string(), source[iterable.range()].to_string()),
            (_, [condition, ..]) => (String::new(), source[condition.range()].to_string()),
            _ => (String::new(), String::new()),
        };
        out.push(Loop { id: node.offset(), range: node.range(), pattern, iterable });
    }
    for child in node.children() {
        collect_loops(source, &child, ids, out);
    }
}

fn collect_variables(source: &str, node: &LinkedNode, canvases: &[Canvas], points: &[Point], out: &mut Vec<Variable>) {
    if node.kind() == SyntaxKind::LetBinding {
        let parts: Vec<_> = node.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::Let | SyntaxKind::Eq)).collect();
        let found = match parts.as_slice() {
            // `let f(x) = ...` parses as a closure holding the name.
            [closure] if closure.kind() == SyntaxKind::Closure => closure
                .children()
                .find(|c| c.kind() == SyntaxKind::Ident)
                .map(|name| (name.get().leaf_text().to_string(), closure.clone(), VariableKind::Function)),
            [pattern, value] if pattern.kind() == SyntaxKind::Ident => {
                let name = pattern.get().leaf_text().to_string();
                let kind = if coord(value).is_some() {
                    VariableKind::Point
                } else if points.iter().any(|p| p.path.starts_with(&format!("{name}.")) || p.path.starts_with(&format!("{name}["))) {
                    VariableKind::Points
                } else if value.kind() == SyntaxKind::Closure {
                    VariableKind::Function
                } else {
                    VariableKind::Value
                };
                Some((name, value.clone(), kind))
            }
            _ => None,
        };
        if let Some((name, value, kind)) = found {
            let text = &source[value.range()];
            let first = text.lines().next().unwrap_or_default().trim();
            let summary = if first.chars().count() > 60 || text.contains('\n') {
                format!("{}…", first.chars().take(60).collect::<String>())
            } else {
                first.to_string()
            };
            let canvas = canvases.iter().find(|c| c.body.start <= node.offset() && node.offset() < c.body.end).map(|c| c.id);
            out.push(Variable { name, range: node.range(), value_range: value.range(), kind, summary, canvas });
        }
    }
    for child in node.children() {
        collect_variables(source, &child, canvases, points, out);
    }
}

fn parse_call(source: &str, points: &[Point], call: &LinkedNode, ctx: Context) -> Call {
    let callee = walk::callee(call).map(|c| source[c.range()].to_string()).unwrap_or_default();
    let mut args = Vec::new();
    let mut args_close = None;
    if let Some(list) = walk::args(call) {
        args_close = list.children().find(|c| c.kind() == SyntaxKind::RightParen).map(|c| c.offset());
        for item in list.children() {
            match item.kind() {
                SyntaxKind::Named => {
                    let key = item.children().next().map(|k| k.get().leaf_text().to_string());
                    if let Some(value) = item.children().last() {
                        args.push(arg(source, points, key, item.range(), &value));
                    }
                }
                SyntaxKind::LeftParen
                | SyntaxKind::RightParen
                | SyntaxKind::Comma
                | SyntaxKind::Space
                | SyntaxKind::LineComment
                | SyntaxKind::BlockComment
                | SyntaxKind::Spread => {}
                _ => args.push(arg(source, points, None, item.range(), &item)),
            }
        }
    }
    let name = args.iter().find(|a| a.key.as_deref() == Some("name")).and_then(|a| match &a.value {
        Value::Str { value } => Some(value.clone()),
        _ => None,
    });
    let connector = route::detect(&callee, &args).map(|c| Connector { fixed: fixed_comment(source, call.offset()).is_some(), ..c });
    Call {
        id: call.offset(),
        range: call.range(),
        callee,
        name,
        args,
        args_close,
        parent: ctx.parent,
        in_loop: ctx.in_loop,
        loop_id: ctx.loop_id,
        conditional: ctx.conditional,
        connector,
    }
}

fn arg(source: &str, points: &[Point], key: Option<String>, range: Range<usize>, value: &LinkedNode) -> Arg {
    // A literal can itself be a shared point (`anchor("A", (0, 0))`).
    let point = points
        .iter()
        .find(|p| p.range == value.range())
        .map(|p| p.id)
        .or_else(|| points::link(value).and_then(|l| points::resolve(points, &l, value.offset())));
    Arg {
        key,
        range,
        value_range: value.range(),
        text: source[value.range()].to_string(),
        value: classify(value),
        point,
    }
}

/// A literal 2D/3D coordinate: `(x, y)` or `(x, y, z)` of plain numbers.
pub(crate) fn coord(node: &LinkedNode) -> Option<(f64, f64, Range<usize>, Range<usize>)> {
    if node.kind() != SyntaxKind::Array {
        return None;
    }
    let items: Vec<_> = node
        .children()
        .filter(|c| !matches!(c.kind(), SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Comma | SyntaxKind::Space))
        .collect();
    if !(2..=3).contains(&items.len()) {
        return None;
    }
    let (x, y) = (number(&items[0])?, number(&items[1])?);
    if items.get(2).is_some_and(|z| number(z).is_none()) {
        return None;
    }
    Some((x, y, items[0].range(), items[1].range()))
}

fn classify(node: &LinkedNode) -> Value {
    match node.kind() {
        SyntaxKind::Array => match coord(node) {
            Some((x, y, x_range, y_range)) => Value::Coord { x, y, x_range, y_range },
            None => Value::Expr,
        },
        SyntaxKind::Str => Value::Str { value: node.get().cast::<ast::Str>().map(|s| s.get().to_string()).unwrap_or_default() },
        SyntaxKind::ContentBlock => {
            let inner = node.children().find(|c| c.kind() == SyntaxKind::Markup).map(|m| m.range());
            Value::Content { inner: inner.unwrap_or(node.offset() + 1..node.offset() + 1) }
        }
        _ => number(node).map_or(Value::Expr, |value| Value::Number { value }),
    }
}

/// A plain numeric literal, optionally negated: `2`, `-1.5`.
fn number(node: &LinkedNode) -> Option<f64> {
    match node.kind() {
        SyntaxKind::Int => node.get().cast::<ast::Int>().map(|i| i.get() as f64),
        SyntaxKind::Float => node.get().cast::<ast::Float>().map(|f| f.get()),
        SyntaxKind::Unary => {
            let mut children = node.children().filter(|c| c.kind() != SyntaxKind::Space);
            let (op, operand) = (children.next()?, children.next()?);
            (op.kind() == SyntaxKind::Minus).then(|| number(&operand)).flatten().map(|n| -n)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"#import "@preview/cetz:0.5.2": canvas, draw
#canvas({
  import draw: *
  line((0, 0), (1.5, -2), "a.east", stroke: red, name: "l")
  for x in (1, 2) { circle((x, 0)) }
  group(name: "g", { rect((0,0), (1,1)) })
  content((2, 3), [Hi *there*], frame: "rect")
})"#;

    #[test]
    fn parses_calls_and_arguments() {
        let scene = parse(SRC);
        assert_eq!(scene.canvases.len(), 1);
        let calls = &scene.canvases[0].calls;
        let callees: Vec<_> = calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["line", "circle", "group", "rect", "content"]);

        let line = &calls[0];
        assert_eq!(line.name.as_deref(), Some("l"));
        assert!(matches!(line.args[0].value, Value::Coord { x: 0.0, y: 0.0, .. }));
        assert!(matches!(line.args[1].value, Value::Coord { x: 1.5, y: -2.0, .. }));
        assert!(matches!(&line.args[2].value, Value::Str { value } if value == "a.east"));
        assert_eq!(line.args[3].key.as_deref(), Some("stroke"));
        assert_eq!(line.args[3].text, "red");
        assert_eq!(&SRC[line.args[3].range.clone()], "stroke: red");

        assert!(calls[1].in_loop);
        assert!(matches!(calls[1].args[0].value, Value::Expr), "(x, 0) isn't literal");
        assert_eq!(calls[3].parent, Some(calls[2].id));

        let Value::Content { inner } = &calls[4].args[1].value else { panic!() };
        assert_eq!(&SRC[inner.clone()], "Hi *there*");
    }

    #[test]
    fn links_arguments_to_shared_points() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#let O = (0, 0)
#canvas({
  import draw: *
  let pts = (A: (1, 0), B: (2, 1))
  for (k, p) in pts { anchor(k, p) }
  anchor("C", (5, 5))
  anchor("D", pts.B)
  line("A", "B", "C", "D", O, pts.A, (9, 9), "x.east")
})"#;
        let scene = parse(src);
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let paths: Vec<_> = scene.points.iter().map(|p| (p.path.as_str(), sorted(p.anchors.clone()))).collect();
        assert_eq!(
            paths,
            [
                ("O", vec![]),
                ("pts.A", vec!["A".to_string()]),
                ("pts.B", vec!["B".to_string(), "D".to_string()]),
                ("C", vec!["C".to_string()]),
            ]
        );
        let line = scene.canvases[0].calls.iter().find(|c| c.callee == "line").unwrap();
        let path_of = |i: usize| line.args[i].point.map(|id| scene.point(id).unwrap().path.as_str());
        assert_eq!(
            (0..8).map(path_of).collect::<Vec<_>>(),
            [Some("pts.A"), Some("pts.B"), Some("C"), Some("pts.B"), Some("O"), Some("pts.A"), None, None]
        );
    }

    #[test]
    fn lists_variables_and_loops() {
        let src = r#"#import "@preview/cetz:0.5.2": canvas, draw
#let O = (0, 0)
#canvas({
  import draw: *
  let pts = (A: (1, 0), B: (2, 1))
  let face(..a) = line(..a.pos(), close: true)
  let s = (stroke: red)
  for (k, p) in pts { anchor(k, p) }
  for (a, b) in (("A", "B"),) { line(a, b) }
})"#;
        let scene = parse(src);
        let vars: Vec<_> = scene.variables.iter().map(|v| (v.name.as_str(), v.kind, v.canvas.is_some())).collect();
        assert_eq!(
            vars,
            [
                ("O", VariableKind::Point, false),
                ("pts", VariableKind::Points, true),
                ("face", VariableKind::Function, true),
                ("s", VariableKind::Value, true),
            ]
        );
        assert_eq!(scene.variables[3].summary, "(stroke: red)");
        let loops: Vec<_> = scene.loops.iter().map(|l| (l.pattern.as_str(), l.iterable.as_str())).collect();
        assert_eq!(loops, [("(k, p)", "pts"), ("(a, b)", r#"(("A", "B"),)"#)]);
        let line = scene.canvases[0].calls.iter().find(|c| c.callee == "line").unwrap();
        assert_eq!(line.loop_id, Some(scene.loops[1].id));
    }

    #[test]
    fn ids_match_instrumented_probe_ids() {
        let scene = parse(SRC);
        let ids: Vec<_> = scene.canvases[0].calls.iter().map(|c| c.id).collect();
        assert_eq!(ids, crate::instrument(SRC).calls);
    }
}
