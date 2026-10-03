//! Syntax highlighting, using Typst's own classification of the tree.

use std::ops::Range;

use typst_syntax::{LinkedNode, Tag, highlight as tag_of};

/// Highlighted byte ranges with their CSS class (see [`Tag::css_class`]),
/// outer nodes before the nodes they contain.
pub fn highlight(source: &str) -> Vec<(Range<usize>, &'static str)> {
    let root = typst_syntax::parse(source);
    let mut spans = Vec::new();
    collect(&mut spans, &LinkedNode::new(&root));
    spans
}

fn collect(spans: &mut Vec<(Range<usize>, &'static str)>, node: &LinkedNode) {
    // Errors are reported separately; tinting them here just adds noise.
    if let Some(tag) = tag_of(node).filter(|&t| t != Tag::Error) {
        spans.push((node.range(), tag.css_class()));
    }
    for child in node.children() {
        collect(spans, &child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_code() {
        let src = "#let x = line((0, 0), (1, 2)) // done";
        let spans = highlight(src);
        let class_of = |text: &str| {
            let start = src.find(text).unwrap();
            spans.iter().find(|(r, _)| *r == (start..start + text.len())).map(|(_, c)| *c)
        };
        assert_eq!(class_of("let"), Some("typ-key"));
        assert_eq!(class_of("line"), Some("typ-func"));
        assert_eq!(class_of("2"), Some("typ-num"));
        assert_eq!(class_of("// done"), Some("typ-comment"));
    }
}
