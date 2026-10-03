//! JavaScript bindings for the editor core.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(getter_with_clone)]
pub struct ParseSummary {
    pub nodes: u32,
    pub errors: Vec<String>,
    pub lossless: bool,
}

#[wasm_bindgen]
pub fn summarize(source: &str) -> ParseSummary {
    let s = cetz_scene::summarize(source);
    ParseSummary {
        nodes: s.nodes as u32,
        errors: s.errors,
        lossless: s.lossless,
    }
}
