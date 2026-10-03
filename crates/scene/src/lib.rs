//! Parses CeTZ source into a scene the editor can manipulate.

use typst_syntax::SyntaxNode;

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
    fn reports_syntax_errors() {
        assert!(!summarize("#let x = (").errors.is_empty());
    }
}
