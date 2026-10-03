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
    /// JSON array of per-draw-call geometry recorded by the CeTZ probe.
    pub probes: Option<String>,
    /// Height of each page in points; pages are stacked in `svg`.
    pub page_heights: Vec<f64>,
}

#[wasm_bindgen(getter_with_clone)]
pub struct ExportOutput {
    /// The file's contents, if compilation and export succeeded.
    pub data: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
    /// Packages to fetch and add before exporting again, as `@ns/name:version`.
    pub missing_packages: Vec<String>,
}

impl From<cetz_compile::Diagnostic> for Diagnostic {
    fn from(d: cetz_compile::Diagnostic) -> Self {
        Diagnostic {
            error: d.error,
            message: d.message,
            file: d.file,
            line: d.line.map(|n| n as u32),
            column: d.column.map(|n| n as u32),
        }
    }
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

    /// Set the source to compile. It is compiled instrumented, so the output
    /// includes CeTZ geometry per draw call; the render is unchanged.
    pub fn set_main(&mut self, source: &str) {
        self.world.set_main_probed(source);
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
            diagnostics: out.diagnostics.into_iter().map(Diagnostic::from).collect(),
            missing_packages: out.missing_packages,
            probes: out.probes,
            page_heights: out.page_heights,
        }
    }

    /// Compiles `source` as written and writes it as `format`: "pdf", "svg",
    /// or "png" at `pixel_per_pt`. The next compile sets its own source.
    pub fn export(&mut self, source: &str, format: &str, pixel_per_pt: f32) -> Result<ExportOutput, JsError> {
        let format = match format {
            "pdf" => cetz_compile::Format::Pdf,
            "svg" => cetz_compile::Format::Svg,
            "png" => cetz_compile::Format::Png(pixel_per_pt),
            _ => return Err(JsError::new(&format!("unknown export format: {format}"))),
        };
        let out = self.world.export(source, format);
        Ok(ExportOutput {
            data: out.data,
            diagnostics: out.diagnostics.into_iter().map(Diagnostic::from).collect(),
            missing_packages: out.missing_packages,
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}
