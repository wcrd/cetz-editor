//! A light structural view of one argument value, so the inspector can offer
//! fields for the parts of `text(5pt)[Hi]`, `1pt + red` or `(end: ">")`
//! instead of a single code box. Ranges are byte offsets into the parsed text.

use std::ops::Range;

use serde::Serialize;
use typst_syntax::{LinkedNode, SyntaxKind, ast};

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub range: Range<usize>,
    #[serde(flatten)]
    pub form: Form,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// `None` for positional arguments and array items.
    pub key: Option<String>,
    /// The whole item, including `key: `.
    pub range: Range<usize>,
    pub value: Node,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Form {
    Number { value: f64 },
    /// A number with a unit: `5pt`, `85%`, `45deg`.
    Numeric { value: f64, unit: &'static str },
    Str { value: String },
    Bool { value: bool },
    None,
    Auto,
    Ident { name: String },
    /// `target.field`, e.g. `color.red`.
    Field { target: Box<Node>, field: String },
    /// A content block; `body` is the markup between the brackets.
    Content { body: Range<usize> },
    /// `callee(args)[body]`. Content arguments, trailing or not, are `args`.
    Call { callee: Box<Node>, args: Vec<Item>, close: Option<usize> },
    Dict { items: Vec<Item>, close: usize },
    Array { items: Vec<Item>, close: usize },
    /// `lhs + rhs`, as in a stroke `1pt + red`.
    Add { lhs: Box<Node>, rhs: Box<Node> },
    Other,
}

/// Parses a single code expression, or `None` if `text` isn't exactly one.
pub fn parse(text: &str) -> Option<Node> {
    let root = typst_syntax::parse_code(text);
    if !root.errors_and_warnings().0.is_empty() {
        return None;
    }
    let linked = LinkedNode::new(&root);
    let mut exprs = linked.children().filter(|c| !c.kind().is_trivia());
    let expr = exprs.next()?;
    exprs.next().is_none().then(|| node(&expr))
}

fn node(n: &LinkedNode) -> Node {
    let form = match n.kind() {
        SyntaxKind::Int => Form::Number { value: n.get().cast::<ast::Int>().map_or(0.0, |i| i.get() as f64) },
        SyntaxKind::Float => Form::Number { value: n.get().cast::<ast::Float>().map_or(0.0, |f| f.get()) },
        SyntaxKind::Numeric => match n.get().cast::<ast::Numeric>() {
            Some(num) => {
                let (value, unit) = num.get();
                Form::Numeric { value, unit: unit_name(unit) }
            }
            None => Form::Other,
        },
        SyntaxKind::Unary => negated(n).unwrap_or(Form::Other),
        SyntaxKind::Str => Form::Str { value: n.get().cast::<ast::Str>().map(|s| s.get().to_string()).unwrap_or_default() },
        SyntaxKind::Bool => Form::Bool { value: n.get().leaf_text() == "true" },
        SyntaxKind::None => Form::None,
        SyntaxKind::Auto => Form::Auto,
        SyntaxKind::Ident => Form::Ident { name: n.get().leaf_text().to_string() },
        SyntaxKind::FieldAccess => {
            let mut parts = n.children().filter(|c| !c.kind().is_trivia());
            match (parts.next(), parts.nth(1)) {
                (Some(target), Some(field)) => Form::Field { target: Box::new(node(&target)), field: field.get().leaf_text().to_string() },
                _ => Form::Other,
            }
        }
        SyntaxKind::ContentBlock => {
            let body = n.children().find(|c| c.kind() == SyntaxKind::Markup).map(|m| m.range());
            Form::Content { body: body.unwrap_or(n.offset() + 1..n.offset() + 1) }
        }
        SyntaxKind::FuncCall => {
            let mut children = n.children();
            match (children.next(), children.find(|c| c.kind() == SyntaxKind::Args)) {
                (Some(callee), Some(args)) => Form::Call {
                    callee: Box::new(node(&callee)),
                    args: items(&args),
                    close: args.children().find(|c| c.kind() == SyntaxKind::RightParen).map(|c| c.offset()),
                },
                _ => Form::Other,
            }
        }
        SyntaxKind::Dict => Form::Dict { items: items(n), close: n.range().end - 1 },
        SyntaxKind::Array => Form::Array { items: items(n), close: n.range().end - 1 },
        SyntaxKind::Parenthesized => {
            return n.children().find(|c| !c.kind().is_trivia() && !matches!(c.kind(), SyntaxKind::LeftParen | SyntaxKind::RightParen)).map_or(
                Node { range: n.range(), form: Form::Other },
                |inner| Node { range: n.range(), form: node(&inner).form },
            );
        }
        SyntaxKind::Binary => {
            let mut parts = n.children().filter(|c| !c.kind().is_trivia());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(lhs), Some(op), Some(rhs)) if op.kind() == SyntaxKind::Plus => {
                    Form::Add { lhs: Box::new(node(&lhs)), rhs: Box::new(node(&rhs)) }
                }
                _ => Form::Other,
            }
        }
        _ => Form::Other,
    };
    Node { range: n.range(), form }
}

fn negated(n: &LinkedNode) -> Option<Form> {
    let mut parts = n.children().filter(|c| !c.kind().is_trivia());
    let (op, operand) = (parts.next()?, parts.next()?);
    if op.kind() != SyntaxKind::Minus {
        return None;
    }
    match node(&operand).form {
        Form::Number { value } => Some(Form::Number { value: -value }),
        Form::Numeric { value, unit } => Some(Form::Numeric { value: -value, unit }),
        _ => None,
    }
}

/// Arguments, dictionary entries or array items.
fn items(list: &LinkedNode) -> Vec<Item> {
    let mut out = Vec::new();
    for item in list.children() {
        match item.kind() {
            SyntaxKind::Named => {
                let mut parts = item.children().filter(|c| !c.kind().is_trivia());
                if let (Some(key), Some(value)) = (parts.next(), parts.nth(1)) {
                    out.push(Item { key: Some(key.get().leaf_text().to_string()), range: item.range(), value: node(&value) });
                }
            }
            SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Comma | SyntaxKind::Colon => {}
            kind if kind.is_trivia() => {}
            _ => out.push(Item { key: None, range: item.range(), value: node(&item) }),
        }
    }
    out
}

fn unit_name(unit: ast::Unit) -> &'static str {
    match unit {
        ast::Unit::Pt => "pt",
        ast::Unit::Mm => "mm",
        ast::Unit::Cm => "cm",
        ast::Unit::In => "in",
        ast::Unit::Rad => "rad",
        ast::Unit::Deg => "deg",
        ast::Unit::Em => "em",
        ast::Unit::Fr => "fr",
        ast::Unit::Percent => "%",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, r: &Range<usize>) -> String {
        text[r.clone()].to_string()
    }

    #[test]
    fn styled_text() {
        let src = "text(5pt, fill: red)[type: .Pinned]";
        let n = parse(src).unwrap();
        let Form::Call { callee, args, close } = n.form else { panic!() };
        assert!(matches!(callee.form, Form::Ident { name } if name == "text"));
        assert_eq!(args.len(), 3);
        assert!(matches!(args[0].value.form, Form::Numeric { value: 5.0, unit: "pt" }));
        assert_eq!(args[1].key.as_deref(), Some("fill"));
        assert_eq!(at(src, &args[1].range), "fill: red");
        let Form::Content { body } = &args[2].value.form else { panic!() };
        assert_eq!(at(src, body), "type: .Pinned");
        assert_eq!(close, Some(19));
    }

    #[test]
    fn strokes_and_colors() {
        let n = parse("0.8pt + orange.lighten(85%)").unwrap();
        let Form::Add { lhs, rhs } = n.form else { panic!() };
        assert!(matches!(lhs.form, Form::Numeric { unit: "pt", .. }));
        let Form::Call { callee, args, .. } = rhs.form else { panic!() };
        assert!(matches!(callee.form, Form::Field { ref field, .. } if field == "lighten"));
        assert!(matches!(args[0].value.form, Form::Numeric { value: 85.0, unit: "%" }));

        let src = r#"(end: ">", fill: black)"#;
        let Form::Dict { items, close } = parse(src).unwrap().form else { panic!() };
        assert_eq!(items.iter().map(|i| i.key.clone().unwrap()).collect::<Vec<_>>(), ["end", "fill"]);
        assert_eq!(close, src.len() - 1);
        assert!(matches!(parse("-2").unwrap().form, Form::Number { value: -2.0 }));
        assert!(matches!(parse("none").unwrap().form, Form::None));
    }

    #[test]
    fn rejects_non_expressions() {
        assert!(parse("1pt red").is_none());
        assert!(parse("(a: ").is_none());
        assert!(parse("").is_none());
    }
}
