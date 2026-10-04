//! Connectors: lines from one shape's anchor to another's, straight or
//! elbowed. An elbow's corners are written relative to the two anchors
//! (`("a.south", "|-", ("a.south", 50%, "b.north"))`), so CeTZ routes it
//! afresh whenever either shape moves.

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
        1 | 2 if corners.iter().all(|a| matches!(a.value, Value::Expr) && (a.text.contains("\"|-\"") || a.text.contains("\"-|\""))) => Route::Elbow,
        _ => return None,
    };
    let bend = (corners.len() == 2).then(|| bend(&corners[0].text, &from)).flatten();
    Some(Connector { route, from, to, bend })
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
/// directions meet otherwise (east to north).
pub fn vertices(from: &str, to: &str, route: Route, bend: f64) -> Vec<String> {
    let (a, b) = (format!("{from:?}"), format!("{to:?}"));
    if route == Route::Straight {
        return vec![a, b];
    }
    let corner = |p: &str, turn: &str, q: &str| format!("({p}, {turn:?}, {q})");
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
        assert_eq!(vertices("a.south", "b.north", Route::Straight, 0.5), [r#""a.south""#, r#""b.north""#]);
        assert_eq!(
            vertices("a.south", "b.north", Route::Elbow, 0.5),
            [
                r#""a.south""#,
                r#"("a.south", "|-", ("a.south", 50%, "b.north"))"#,
                r#"("b.north", "|-", ("a.south", 50%, "b.north"))"#,
                r#""b.north""#,
            ]
        );
        assert_eq!(vertices("a.east", "b.west", Route::Elbow, 0.25)[1], r#"("a.east", "-|", ("a.east", 25%, "b.west"))"#);
        assert_eq!(vertices("a.east", "b.north", Route::Elbow, 0.5), [r#""a.east""#, r#"("a.east", "-|", "b.north")"#, r#""b.north""#]);
        assert_eq!(vertices("a.south", "b.west", Route::Elbow, 0.5)[1], r#"("a.south", "|-", "b.west")"#);
    }

    #[test]
    fn swaps_anchors() {
        assert_eq!(with_anchor("g.a.south", "east"), "g.a.east");
        assert_eq!(with_anchor("a", "east"), "a.east");
    }
}
