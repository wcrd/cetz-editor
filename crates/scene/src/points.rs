//! Shared points: literal coordinates defined once and used by several draw
//! calls, either as Typst variables (`let A = (0, 0)`, `let pts = (A: ...)`)
//! or as CeTZ named anchors (`anchor("A", (0, 0))`, or a loop like
//! `for (k, p) in pts { anchor(k, p) }`). Moving a shared point edits its
//! definition, so every shape that uses it follows.

use std::ops::Range;

use serde::Serialize;
use typst_syntax::{LinkedNode, SyntaxKind, ast};

use crate::scene::coord;
use crate::walk;

#[derive(Debug, Clone, Serialize)]
pub struct Point {
    /// Byte offset of the coordinate literal.
    pub id: usize,
    /// How code refers to it: `A`, `pts.A`, `pts[0]`, or an anchor name.
    pub path: String,
    pub x: f64,
    pub y: f64,
    pub range: Range<usize>,
    pub x_range: Range<usize>,
    pub y_range: Range<usize>,
    /// CeTZ anchor names bound to this point.
    pub anchors: Vec<String>,
}

/// How an argument refers to a point, before resolving it.
#[derive(Debug, Clone)]
pub(crate) enum Link {
    /// A variable path: `A` or `pts.A`.
    Path(String),
    /// A CeTZ anchor name: `"A"`.
    Anchor(String),
}

/// Collects every literal point definition in the file.
pub(crate) fn collect(root: &LinkedNode) -> Vec<Point> {
    let mut points = Vec::new();
    collect_lets(root, &mut points);
    walk::for_each_canvas(root, &mut |_, body| {
        walk::for_each_call(body, walk::Context::default(), &mut |call, _| anchor_call(call, &mut points));
        collect_anchor_loops(body, &mut points);
    });
    points.sort_by_key(|p| p.id);
    points
}

fn point(path: String, node: &LinkedNode) -> Option<Point> {
    let (x, y, x_range, y_range) = coord(node)?;
    Some(Point { id: node.offset(), path, x, y, range: node.range(), x_range, y_range, anchors: Vec::new() })
}

/// `let A = (0, 0)`, `let pts = (A: (0, 0), ...)`, `let ps = ((0, 0), ...)`.
fn collect_lets(node: &LinkedNode, points: &mut Vec<Point>) {
    if node.kind() == SyntaxKind::LetBinding {
        let mut children = node.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::Let | SyntaxKind::Eq));
        if let (Some(pattern), Some(value)) = (children.next(), children.last())
            && pattern.kind() == SyntaxKind::Ident
        {
            let name = pattern.get().leaf_text().to_string();
            if let Some(p) = point(name.clone(), &value) {
                points.push(p);
            } else if value.kind() == SyntaxKind::Dict {
                for entry in value.children().filter(|c| c.kind() == SyntaxKind::Named) {
                    let key = entry.children().next().map(|k| k.get().leaf_text().to_string());
                    if let (Some(key), Some(v)) = (key, entry.children().last())
                        && let Some(p) = point(format!("{name}.{key}"), &v)
                    {
                        points.push(p);
                    }
                }
            } else if value.kind() == SyntaxKind::Array {
                let items = value.children().filter(|c| is_item(c));
                for (i, item) in items.enumerate() {
                    if let Some(p) = point(format!("{name}[{i}]"), &item) {
                        points.push(p);
                    }
                }
            }
        }
    }
    for child in node.children() {
        collect_lets(&child, points);
    }
}

fn is_item(node: &LinkedNode) -> bool {
    !matches!(node.kind(), SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Comma | SyntaxKind::Space)
}

/// `anchor("A", (0, 0))` defines a point; `anchor("A", pts.A)` names one.
fn anchor_call(call: &LinkedNode, points: &mut Vec<Point>) {
    let Some(callee) = walk::callee(call) else { return };
    if callee.get().leaf_text() != "anchor" && !callee.get().full_text().ends_with(".anchor") {
        return;
    }
    let Some(args) = walk::args(call) else { return };
    let items: Vec<_> = args.children().filter(|c| is_item(c)).collect();
    let (Some(name), Some(value)) = (items.first(), items.get(1)) else { return };
    let Some(name) = name.get().cast::<ast::Str>().map(|s| s.get().to_string()) else { return };
    if let Some(mut p) = point(name.clone(), value) {
        p.anchors.push(name);
        points.push(p);
    } else if let Some(Link::Path(path)) = link(value) {
        pending_name(points, &path, value.offset(), name);
    }
}

/// `for (k, p) in pts { anchor(k, p) }` names each entry of `pts` by its key.
fn collect_anchor_loops(node: &LinkedNode, points: &mut Vec<Point>) {
    if node.kind() == SyntaxKind::ForLoop {
        let parts: Vec<_> = node.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::For | SyntaxKind::In)).collect();
        if let [pattern, iterable, body] = parts.as_slice()
            && pattern.kind() == SyntaxKind::Destructuring
            && iterable.kind() == SyntaxKind::Ident
        {
            let vars: Vec<String> = pattern.children().filter(|c| c.kind() == SyntaxKind::Ident).map(|c| c.get().leaf_text().to_string()).collect();
            let dict = iterable.get().leaf_text().to_string();
            if let [k, v] = vars.as_slice()
                && body_has_anchor(body, k, v)
            {
                let prefix = format!("{dict}.");
                for p in points.iter_mut().filter(|p| p.path.starts_with(&prefix) && p.id < node.offset()) {
                    let key = p.path[prefix.len()..].to_string();
                    p.anchors.push(key);
                }
            }
        }
    }
    for child in node.children() {
        collect_anchor_loops(&child, points);
    }
}

fn body_has_anchor(node: &LinkedNode, k: &str, v: &str) -> bool {
    if node.kind() == SyntaxKind::FuncCall
        && walk::callee(node).is_some_and(|c| c.get().leaf_text() == "anchor")
        && let Some(args) = walk::args(node)
    {
        let items: Vec<String> = args.children().filter(|c| is_item(c)).map(|c| c.get().full_text().to_string()).collect();
        if items == [k, v] {
            return true;
        }
    }
    node.children().any(|c| body_has_anchor(&c, k, v))
}

/// Gives an anchor name to the point a variable path resolves to.
fn pending_name(points: &mut [Point], path: &str, used_at: usize, name: String) {
    if let Some(i) = resolve_index(points, path, used_at) {
        points[i].anchors.push(name);
    }
}

/// The nearest definition of `path` before `used_at` (else any).
fn resolve_index(points: &[Point], path: &str, used_at: usize) -> Option<usize> {
    let matches = points.iter().enumerate().filter(|(_, p)| p.path == path);
    matches.clone().filter(|(_, p)| p.id < used_at).max_by_key(|(_, p)| p.id).or_else(|| matches.min_by_key(|(_, p)| p.id)).map(|(i, _)| i)
}

/// How an argument value refers to a point, if it does.
pub(crate) fn link(value: &LinkedNode) -> Option<Link> {
    match value.kind() {
        SyntaxKind::Ident => Some(Link::Path(value.get().leaf_text().to_string())),
        SyntaxKind::FieldAccess => {
            let parts: Vec<_> = value.children().filter(|c| c.kind() != SyntaxKind::Dot).collect();
            match parts.as_slice() {
                [a, b] if a.kind() == SyntaxKind::Ident && b.kind() == SyntaxKind::Ident => {
                    Some(Link::Path(format!("{}.{}", a.get().leaf_text(), b.get().leaf_text())))
                }
                _ => None,
            }
        }
        SyntaxKind::Str => {
            let s = value.get().cast::<ast::Str>()?.get().to_string();
            (!s.contains('.')).then_some(Link::Anchor(s))
        }
        _ => None,
    }
}

/// Resolves a link used at `used_at` to a point id.
pub(crate) fn resolve(points: &[Point], link: &Link, used_at: usize) -> Option<usize> {
    match link {
        Link::Path(path) => resolve_index(points, path, used_at).map(|i| points[i].id),
        Link::Anchor(name) => points.iter().find(|p| p.anchors.iter().any(|a| a == name)).map(|p| p.id),
    }
}
