//! The editor's view of a source file: each CeTZ canvas and the draw calls in
//! its body, with their arguments and exact byte ranges. Geometry is not
//! computed here — it comes from the CeTZ probe, keyed by call offset.

use std::ops::Range;

use serde::Serialize;
use typst_syntax::{LinkedNode, SyntaxKind, ast};

use crate::points::{self, Point};
use crate::walk::{self, Context};

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub canvases: Vec<Canvas>,
    /// Literal points defined once and shared by reference.
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Canvas {
    /// Byte offset of the `canvas(...)` call.
    pub id: usize,
    /// Byte range of the body block, braces included.
    pub body: Range<usize>,
    pub calls: Vec<Call>,
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
    pub conditional: bool,
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

pub fn parse(source: &str) -> Scene {
    let root = typst_syntax::parse(source);
    let linked = LinkedNode::new(&root);
    let points = points::collect(&linked);
    let mut canvases = Vec::new();
    walk::for_each_canvas(&linked, &mut |canvas, body| {
        let mut calls = Vec::new();
        walk::for_each_call(body, Context::default(), &mut |call, ctx| calls.push(parse_call(source, &points, call, ctx)));
        canvases.push(Canvas { id: canvas.offset(), body: body.range(), calls });
    });
    Scene { canvases, points }
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
    Call {
        id: call.offset(),
        range: call.range(),
        callee,
        name,
        args,
        args_close,
        parent: ctx.parent,
        in_loop: ctx.in_loop,
        conditional: ctx.conditional,
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
    fn ids_match_instrumented_probe_ids() {
        let scene = parse(SRC);
        let ids: Vec<_> = scene.canvases[0].calls.iter().map(|c| c.id).collect();
        assert_eq!(ids, crate::instrument(SRC).calls);
    }
}
