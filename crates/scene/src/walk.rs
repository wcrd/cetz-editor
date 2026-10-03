//! Finds CeTZ canvas bodies and the draw calls in them. Shared by the scene
//! parser and the instrumenter so both agree on what a draw call is.

use typst_syntax::{LinkedNode, SyntaxKind};

/// Where a draw call sits relative to control flow and other calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// Offset of the enclosing draw call, for calls in block arguments
    /// such as `group({ ... })`.
    pub parent: Option<usize>,
    /// Inside a `for`/`while` body: the call may produce several instances.
    pub in_loop: bool,
    /// Offset of the innermost enclosing loop.
    pub loop_id: Option<usize>,
    /// Inside an `if`/`else` body.
    pub conditional: bool,
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
