//! Parses CeTZ source into a scene the editor can manipulate.

mod edit;
pub mod expr;
mod highlight;
mod instrument;
mod points;
pub mod route;
mod scene;
mod walk;

pub use edit::{Edit, EditResult, Patch, apply};
pub use highlight::highlight;
pub use instrument::{Instrumented, PROBE_PATH, instrument};
pub use points::Point;
pub use scene::{Arg, Call, Canvas, Scene, Value, parse};

use std::ops::Range;

use typst_syntax::{LinkedNode, Side, SyntaxKind, SyntaxNode};

/// Basic facts about a parsed source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Total syntax nodes in the tree.
    pub nodes: usize,
    /// Parser error messages.
    pub errors: Vec<String>,
    /// Whether the syntax tree reproduces the source byte-for-byte.
    pub lossless: bool,
}

pub fn summarize(source: &str) -> Summary {
    let root = typst_syntax::parse(source);
    Summary {
        nodes: count(&root),
        errors: root.errors_and_warnings().0.into_iter().map(|e| e.message.to_string()).collect(),
        lossless: root.full_text() == source,
    }
}

/// The byte range of the function call that starts exactly at `offset`.
pub fn call_range(source: &str, offset: usize) -> Option<Range<usize>> {
    let root = typst_syntax::parse(source);
    let leaf = LinkedNode::new(&root).leaf_at(offset, Side::After)?;
    let mut node = Some(&leaf);
    while let Some(n) = node {
        if n.kind() == SyntaxKind::FuncCall && n.offset() == offset {
            return Some(n.range());
        }
        node = n.parent();
    }
    None
}

fn count(node: &SyntaxNode) -> usize {
    1 + node.children().map(count).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn fixtures_parse_losslessly() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut seen = 0;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "typ") {
                let summary = summarize(&fs::read_to_string(&path).unwrap());
                assert!(summary.errors.is_empty(), "{}: {:?}", path.display(), summary.errors);
                assert!(summary.lossless, "{} did not round-trip", path.display());
                seen += 1;
            }
        }
        assert!(seen > 0, "no .typ fixtures found");
    }

    #[test]
    fn finds_call_ranges() {
        let src = "#canvas({ line((0, 0), (1, 1)) })";
        let at = src.find("line").unwrap();
        assert_eq!(call_range(src, at), Some(at..src.len() - 3));
        assert_eq!(call_range(src, at + 1), None);
    }

    #[test]
    fn reports_syntax_errors() {
        assert!(!summarize("#let x = (").errors.is_empty());
    }
}
