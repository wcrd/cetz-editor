//! Finds CeTZ canvas bodies, the functions they draw with, and the draw calls
//! in both. Shared by the scene parser and the instrumenter so both agree on
//! what a draw call is.

use std::collections::BTreeSet;

use typst_syntax::{LinkedNode, SyntaxKind};

/// Where a draw call sits relative to control flow and other calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// Offset of the enclosing draw call, for calls in block arguments
    /// such as `group({ ... })`; for a statement of a drawing function's
    /// body, the function's `let` binding.
    pub parent: Option<usize>,
    /// Inside a `for`/`while` body: the call may produce several instances.
    pub in_loop: bool,
    /// Offset of the innermost enclosing loop.
    pub loop_id: Option<usize>,
    /// Inside an `if`/`else` body.
    pub conditional: bool,
    /// Offset of the `let` binding of the drawing function the call is in.
    pub function: Option<usize>,
}

/// A function that draws: `let plate(x) = { rect(..); .. }`, called as a
/// statement from a canvas body or from another such function. Its draw
/// calls run once per use.
#[derive(Debug, Clone)]
pub struct Function<'a> {
    /// The whole `let` binding.
    pub binding: LinkedNode<'a>,
    pub name: String,
    /// The parameter list, parentheses included.
    pub params: Option<LinkedNode<'a>>,
    /// The expression after `=`: a block, or a single call like `group({..})`.
    pub body: LinkedNode<'a>,
}

/// Every drawing function in the file (see `Function`), in source order.
pub fn drawing_functions<'a>(root: &LinkedNode<'a>) -> Vec<Function<'a>> {
    let mut all = Vec::new();
    collect_functions(root, &mut all);
    // Names called as statements: from the canvases first, then from the
    // functions those reach, until nothing new turns up.
    let mut called = BTreeSet::new();
    for_each_canvas(root, &mut |_, body| for_each_call(body, Context::default(), &mut |call, _| called.extend(ident_callee(call))));
    let mut drawing = vec![false; all.len()];
    loop {
        let mut grew = false;
        for (i, function) in all.iter().enumerate() {
            if !drawing[i] && called.contains(&function.name) {
                drawing[i] = true;
                grew = true;
                walk_stmt(&function.body, Context::default(), &mut |call, _| {
                    called.extend(ident_callee(call));
                });
            }
        }
        if !grew {
            break;
        }
    }
    all.into_iter().zip(drawing).filter_map(|(f, d)| d.then_some(f)).collect()
}

/// Visits every statement-level draw call in a drawing function's body.
pub fn for_each_function_call<'a>(function: &Function<'a>, f: &mut impl FnMut(&LinkedNode<'a>, Context)) {
    let id = function.binding.offset();
    walk_stmt(&function.body, Context { parent: Some(id), function: Some(id), ..Context::default() }, f);
}

/// `let f(..) = body` and `let f = (..) => body`.
fn collect_functions<'a>(node: &LinkedNode<'a>, out: &mut Vec<Function<'a>>) {
    if node.kind() == SyntaxKind::LetBinding {
        let parts: Vec<_> = node.children().filter(|c| !matches!(c.kind(), SyntaxKind::Space | SyntaxKind::Let | SyntaxKind::Eq)).collect();
        let found = match parts.as_slice() {
            [closure] if closure.kind() == SyntaxKind::Closure => {
                closure.children().find(|c| c.kind() == SyntaxKind::Ident).map(|name| (name.get().leaf_text().to_string(), closure.clone()))
            }
            [name, closure] if name.kind() == SyntaxKind::Ident && closure.kind() == SyntaxKind::Closure => {
                Some((name.get().leaf_text().to_string(), closure.clone()))
            }
            _ => None,
        };
        if let Some((name, closure)) = found {
            let params = closure.children().find(|c| c.kind() == SyntaxKind::Params);
            if let Some(body) = closure.children().filter(|c| !c.kind().is_trivia()).last() {
                out.push(Function { binding: node.clone(), name, params, body });
            }
        }
    }
    for child in node.children() {
        collect_functions(&child, out);
    }
}

/// The name a call calls, when it's a plain identifier (`plate(..)`).
pub fn ident_callee(call: &LinkedNode) -> Option<String> {
    callee(call).filter(|c| c.kind() == SyntaxKind::Ident).map(|c| c.get().leaf_text().to_string())
}

/// Finds `canvas(..., { body })` / `cetz.canvas(...)` calls and visits the
/// canvas call and the `CodeBlock` of its body.
pub fn for_each_canvas<'a>(node: &LinkedNode<'a>, f: &mut impl FnMut(&LinkedNode<'a>, &LinkedNode<'a>)) {
    if node.kind() == SyntaxKind::FuncCall && is_canvas_callee(node) {
        if let Some(body) = args(node).and_then(|a| a.children().filter(|c| c.kind() == SyntaxKind::CodeBlock).last()) {
            f(node, &body);
        }
    }
    for child in node.children() {
        for_each_canvas(&child, f);
    }
}

/// Visits every statement-level draw call in a canvas body block.
pub fn for_each_call<'a>(block: &LinkedNode<'a>, ctx: Context, f: &mut impl FnMut(&LinkedNode<'a>, Context)) {
    if let Some(code) = block_code(block) {
        for stmt in code.children() {
            walk_stmt(&stmt, ctx, f);
        }
    }
}

fn walk_stmt<'a>(stmt: &LinkedNode<'a>, ctx: Context, f: &mut impl FnMut(&LinkedNode<'a>, Context)) {
    match stmt.kind() {
        SyntaxKind::FuncCall => {
            f(stmt, ctx);
            let inner = Context { parent: Some(stmt.offset()), ..ctx };
            if let Some(args) = args(stmt) {
                for arg in args.children().filter(|a| a.kind() == SyntaxKind::CodeBlock) {
                    for_each_call(&arg, inner, f);
                }
            }
        }
        SyntaxKind::ForLoop | SyntaxKind::WhileLoop => {
            let inner = Context { in_loop: true, loop_id: Some(stmt.offset()), ..ctx };
            for child in stmt.children().filter(|c| c.kind() == SyntaxKind::CodeBlock) {
                for_each_call(&child, inner, f);
            }
        }
        SyntaxKind::Conditional => {
            let inner = Context { conditional: true, ..ctx };
            for child in stmt.children().filter(|c| c.kind() == SyntaxKind::CodeBlock) {
                for_each_call(&child, inner, f);
            }
        }
        SyntaxKind::CodeBlock => for_each_call(stmt, ctx, f),
        _ => {}
    }
}

fn is_canvas_callee(call: &LinkedNode) -> bool {
    match callee(call) {
        Some(c) if c.kind() == SyntaxKind::Ident => c.get().leaf_text() == "canvas",
        Some(c) if c.kind() == SyntaxKind::FieldAccess => {
            c.children().last().is_some_and(|f| f.get().leaf_text() == "canvas")
        }
        _ => false,
    }
}

pub fn callee<'a>(call: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    call.children().next()
}

pub fn args<'a>(call: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    call.children().find(|c| c.kind() == SyntaxKind::Args)
}

pub fn block_code<'a>(block: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    block.children().find(|c| c.kind() == SyntaxKind::Code)
}
