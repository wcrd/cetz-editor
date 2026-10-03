//! JavaScript bindings for the editor core.

use wasm_bindgen::prelude::*;

/// The end offset of the function call starting at `offset`, if there is one.
#[wasm_bindgen]
pub fn call_end(source: &str, offset: usize) -> Option<usize> {
    cetz_scene::call_range(source, offset).map(|r| r.end)
}

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

/// The scene model (canvases, draw calls, arguments) as JSON.
#[wasm_bindgen]
pub fn scene(source: &str) -> String {
    serde_json::to_string(&cetz_scene::parse(source)).unwrap_or_else(|_| "{\"canvases\":[]}".into())
}

/// Applies an edit (JSON, see `cetz_scene::Edit`) and returns the
/// `EditResult` as JSON.
#[wasm_bindgen]
pub fn apply_edit(source: &str, edit: &str) -> Result<String, JsError> {
    let edit: cetz_scene::Edit = serde_json::from_str(edit).map_err(|e| JsError::new(&format!("invalid edit: {e}")))?;
    let result = cetz_scene::apply(source, &edit).map_err(|e| JsError::new(&e))?;
    serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
}
