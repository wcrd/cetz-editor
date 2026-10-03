//! JavaScript bindings for the Typst compiler, run inside a Web Worker.

use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen(getter_with_clone)]
pub struct Diagnostic {
    pub error: bool,
    pub message: String,
    /// The file the diagnostic points into; undefined for the main source.
    pub file: Option<String>,
    /// Zero-based line and column, when known.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[wasm_bindgen(getter_with_clone)]
pub struct CompileOutput {
    pub svg: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Packages to fetch and add before compiling again, as `@ns/name:version`.
    pub missing_packages: Vec<String>,
}

/// A Typst compiler holding one main source and any added packages.
#[wasm_bindgen]
pub struct Compiler {
    world: cetz_compile::EditorWorld,
}

#[wasm_bindgen]
impl Compiler {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Compiler {
        Compiler { world: cetz_compile::EditorWorld::new() }
    }

    pub fn set_main(&mut self, source: &str) {
        self.world.set_main(source);
    }

    pub fn has_package(&self, spec: &str) -> bool {
        self.world.has_package(spec)
    }

    /// Add a package from its `.tar.gz` archive.
    pub fn add_package(&mut self, spec: &str, archive: &[u8]) -> Result<(), JsError> {
        self.world.add_package_archive(spec, archive).map_err(|e| JsError::new(&e))
    }

    pub fn compile(&mut self) -> CompileOutput {
        let out = self.world.compile();
        CompileOutput {
            svg: out.svg,
            diagnostics: out
                .diagnostics
                .into_iter()
                .map(|d| Diagnostic {
                    error: d.error,
                    message: d.message,
                    file: d.file,
                    line: d.line.map(|n| n as u32),
                    column: d.column.map(|n| n as u32),
                })
                .collect(),
            missing_packages: out.missing_packages,
        }
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}
