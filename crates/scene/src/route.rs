//! Connectors: lines from one shape's anchor to another's, straight or
//! elbowed. An elbow's corners are written relative to the two anchors
//! (`("a.south", "|-", ("a.south", 50%, "b.north"))`), so CeTZ routes it
//! afresh whenever either shape moves. When a side faces away from the other
//! end (out of `a.south` to a shape above), the elbow detours: each end
//! steps `STUB` out from its side first (`(rel: (0, -0.5), to: "a.south")`)
//! and the route joins those two points. Which form fits depends on where
//! the shapes are, which the editor knows and CeTZ code can't ask, so the
//! editor picks it when it writes the connector.

use serde::{Deserialize, Serialize};

use crate::edit::num;
use crate::scene::{Arg, Value};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    Straight,
    Elbow,
}

#[derive(Debug, Clone, Serialize)]
pub struct Connector {
    pub route: Route,
    /// The anchors it runs between, as written (`"a.south"`, `"g.b.north"`).
    pub from: String,
    pub to: String,
    /// How far from `from` to `to` (0 to 1) a two-corner elbow crosses
    /// over, when it's written as a plain percentage.
    pub bend: Option<f64>,
    /// Steps out from each side first, as an elbow whose side faces away
    /// from the other end does.
    pub detour: bool,
    /// Pinned by a `// cetz-editor: fixed` comment above it: its sides stay
    /// put when the shapes it joins move.
    pub fixed: bool,
}

/// The connector a `line` call draws, if it is one: its first and last
/// positional arguments name anchors, and any in between are elbow corners
/// worked out from those anchors.
pub fn detect(callee: &str, args: &[Arg]) -> Option<Connector> {
    if callee.rsplit('.').next() != Some("line") {
        return None;
    }
    let positional: Vec<&Arg> = args.iter().filter(|a| a.key.is_none()).collect();
    let anchor = |a: &Arg| match &a.value {
        Value::Str { value } => Some(value.clone()),
        _ => None,
    };
    let (from, to) = (anchor(positional.first()?)?, anchor(positional.last()?)?);
    let corners = &positional[1..positional.len() - 1];
    let route = match corners.len() {
        0 if positional.len() == 2 => Route::Straight,
        1..=4 if corners.iter().all(|a| matches!(a.value, Value::Expr) && (is_stub(&a.text) || a.text.contains("\"|-\"") || a.text.contains("\"-|\""))) => Route::Elbow,
        _ => return None,
    };
    let detour = corners.iter().any(|a| is_stub(&a.text));
    let bend = if detour || corners.len() == 2 { corners.iter().find_map(|c| bend(&c.text, &from)) } else { None };
    Some(Connector { route, from, to, bend, detour, fixed: false })
}

/// How far (canvas units) a detouring elbow steps out from each side.
pub const STUB: f64 = 0.5;

fn is_stub(text: &str) -> bool {
    text.starts_with("(rel:")
}

/// The point `STUB` out from an anchor, the way its side faces:
/// `(rel: (0, -0.5), to: "a.south")`.
fn stub(anchor: &str) -> String {
    let side = anchor.rsplit('.').next().unwrap_or(anchor);
    let (dx, dy) = match side {
        "east" => (STUB, 0.0),
        "west" => (-STUB, 0.0),
        s if s.contains("north") => (0.0, STUB),
        _ => (0.0, -STUB),
    };
    format!("(rel: ({}, {}), to: {anchor:?})", num(dx), num(dy))
}

/// The `50%` in a corner `("a.south", "|-", ("a.south", 50%, "b.north"))`.
fn bend(corner: &str, from: &str) -> Option<f64> {
    let start = corner.rfind(&format!("({from:?}, "))? + from.len() + 4;
    let (percent, _) = corner[start..].split_once('%')?;
    percent.trim().parse::<f64>().ok().map(|p| p / 100.0)
}

/// The positional arguments (source text) of a connector between two
/// anchors. An elbow leaves and enters each shape square to the side it's
/// on: two corners halfway along when both sides face the same way (south
/// to north), `bend` (0 to 1) of the way along, or one where the two
/// directions meet otherwise (east to north). A `detour` steps out from
/// both sides first, then joins those points the same way, its middle
/// segment `bend` of the way across when the sides face the same way.
pub fn vertices(from: &str, to: &str, route: Route, bend: f64, detour: bool) -> Vec<String> {
    let (a, b) = (format!("{from:?}"), format!("{to:?}"));
    if route == Route::Straight {
        return vec![a, b];
    }
    let corner = |p: &str, turn: &str, q: &str| format!("({p}, {turn:?}, {q})");
    if detour {
        let (sa, sb) = (stub(from), stub(to));
        let mid = format!("({a}, {}%, {b})", num(bend * 100.0));
        let joins = match (vertical(from), vertical(to)) {
            (true, true) => vec![corner(&sa, "-|", &mid), corner(&mid, "|-", &sb)],
            (false, false) => vec![corner(&sa, "|-", &mid), corner(&mid, "-|", &sb)],
            (true, false) => vec![corner(&sa, "-|", &sb)],
            (false, true) => vec![corner(&sa, "|-", &sb)],
        };
        return [vec![a, sa], joins, vec![sb, b]].concat();
    }
    match (vertical(from), vertical(to)) {
        (true, true) | (false, false) => {
            let turn = if vertical(from) { "|-" } else { "-|" };
            let mid = format!("({a}, {}%, {b})", num(bend * 100.0));
            vec![a.clone(), corner(&a, turn, &mid), corner(&b, turn, &mid), b]
        }
        (true, false) => vec![a.clone(), corner(&a, "|-", &b), b],
        (false, true) => vec![a.clone(), corner(&a, "-|", &b), b],
    }
}

/// Whether a line leaves (or enters) this anchor up or down: true for
/// `north` and `south` and for anything that isn't a side (a centre), false
/// for `east` and `west`.
fn vertical(anchor: &str) -> bool {
    let side = anchor.rsplit('.').next().unwrap_or(anchor);
    !matches!(side, "east" | "west")
}

/// `"g.a.south"` with its anchor swapped for `side`.
pub fn with_anchor(anchor: &str, side: &str) -> String {
    let element = anchor.rsplit_once('.').map_or(anchor, |(element, _)| element);
    format!("{element}.{side}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elbows_turn_where_the_sides_meet() {
        assert_eq!(vertices("a.south", "b.north", Route::Straight, 0.5, false), [r#""a.south""#, r#""b.north""#]);
        assert_eq!(
            vertices("a.south", "b.north", Route::Elbow, 0.5, false),
            [
                r#""a.south""#,
                r#"("a.south", "|-", ("a.south", 50%, "b.north"))"#,
                r#"("b.north", "|-", ("a.south", 50%, "b.north"))"#,
                r#""b.north""#,
            ]
        );
        assert_eq!(vertices("a.east", "b.west", Route::Elbow, 0.25, false)[1], r#"("a.east", "-|", ("a.east", 25%, "b.west"))"#);
        assert_eq!(vertices("a.east", "b.north", Route::Elbow, 0.5, false), [r#""a.east""#, r#"("a.east", "-|", "b.north")"#, r#""b.north""#]);
        assert_eq!(vertices("a.south", "b.west", Route::Elbow, 0.5, false)[1], r#"("a.south", "|-", "b.west")"#);
    }

    #[test]
    fn detours_step_out_from_each_side() {
        assert_eq!(
            vertices("a.south", "b.north", Route::Elbow, 0.5, true),
            [
                r#""a.south""#,
                r#"(rel: (0, -0.5), to: "a.south")"#,
                r#"((rel: (0, -0.5), to: "a.south"), "-|", ("a.south", 50%, "b.north"))"#,
                r#"(("a.south", 50%, "b.north"), "|-", (rel: (0, 0.5), to: "b.north"))"#,
                r#"(rel: (0, 0.5), to: "b.north")"#,
                r#""b.north""#,
            ]
        );
        assert_eq!(
            vertices("c.east", "d.north", Route::Elbow, 0.5, true),
            [
                r#""c.east""#,
                r#"(rel: (0.5, 0), to: "c.east")"#,
                r#"((rel: (0.5, 0), to: "c.east"), "|-", (rel: (0, 0.5), to: "d.north"))"#,
                r#"(rel: (0, 0.5), to: "d.north")"#,
                r#""d.north""#,
            ]
        );
    }

    #[test]
    fn swaps_anchors() {
        assert_eq!(with_anchor("g.a.south", "east"), "g.a.east");
        assert_eq!(with_anchor("a", "east"), "a.east");
    }
}
