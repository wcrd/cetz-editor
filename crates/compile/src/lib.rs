//! An in-memory Typst world: one main source, embedded fonts, and packages
//! supplied by the host (the browser fetches them; tests read the local cache).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::sync::Mutex;

use cetz_scene::Instrumented;
use typst::diag::{FileError, FileResult, PackageError, Severity, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Duration, Label, Selector};
use typst::introspection::{Introspector, MetadataElem};
use typst::layout::Abs;
use typst_layout::PagedDocument;
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, Lines, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, PicoStr, Scalar};
use typst::{Library, LibraryExt, World, WorldExt};

/// The result of one compilation.
#[derive(Debug, Default)]
pub struct Output {
    /// The rendered document, all pages merged, if compilation succeeded.
    pub svg: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Packages the source imported that haven't been added yet, as
    /// `@namespace/name:version`. Add them and compile again.
    pub missing_packages: Vec<String>,
    /// Geometry recorded by the probe for each draw call, as a JSON array.
    /// Only set for sources added with [`EditorWorld::set_main_probed`].
    pub probes: Option<String>,
    /// Height of each page in points; pages are stacked top to bottom in `svg`.
    pub page_heights: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub error: bool,
    pub message: String,
    /// The file the diagnostic points into: `None` for the main source.
    pub file: Option<String>,
    /// Zero-based line and column in that file, when known.
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// A file format [`EditorWorld::export`] writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Pdf,
    /// All pages stacked into one image, as the editor shows them.
    Svg,
    /// All pages stacked into one image, at this many pixels per point.
    Png(f32),
}

/// The result of one export.
#[derive(Debug, Default)]
pub struct Export {
    /// The file's contents, if compilation and export succeeded.
    pub data: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
    /// As in [`Output::missing_packages`]: add them and export again.
    pub missing_packages: Vec<String>,
}

impl Diagnostic {
    /// An error that isn't tied to a place in any file.
    fn message(message: String) -> Self {
        Self { error: true, message, file: None, line: None, column: None }
    }
}

struct Probe {
    instrumented: Instrumented,
    original: Lines<String>,
}

pub struct EditorWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    /// Set when `main` holds an instrumented copy of the user's source.
    probe: Option<Probe>,
    probe_id: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    packages: HashSet<PackageSpec>,
    missing: Mutex<BTreeSet<String>>,
}

impl Default for EditorWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorWorld {
    pub fn new() -> Self {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        let main_id = RootedPath::new(VirtualRoot::Project, VirtualPath::new("/main.typ").unwrap())
            .intern();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
            main: Source::new(main_id, String::new()),
            probe: None,
            probe_id: RootedPath::new(
                VirtualRoot::Project,
                VirtualPath::new(cetz_scene::PROBE_PATH).unwrap(),
            )
            .intern(),
            sources: HashMap::new(),
            files: HashMap::new(),
            packages: HashSet::new(),
            missing: Mutex::new(BTreeSet::new()),
        }
    }

    /// Replace the main source text. Reparses incrementally.
    pub fn set_main(&mut self, text: &str) {
        self.main.replace(text);
        self.probe = None;
    }

    /// Like [`set_main`](Self::set_main), but compiles an instrumented copy so
    /// [`Output::probes`] reports CeTZ geometry per draw call. The render is
    /// unchanged; diagnostics still point into the original text.
    pub fn set_main_probed(&mut self, text: &str) {
        let instrumented = cetz_scene::instrument(text);
        self.main.replace(&instrumented.text);
        self.probe = Some(Probe { instrumented, original: Lines::new(text.to_owned()) });
    }

    pub fn has_package(&self, spec: &str) -> bool {
        spec.parse::<PackageSpec>().is_ok_and(|s| self.packages.contains(&s))
    }

    /// Add a package from its `.tar.gz` archive, as served by packages.typst.org.
    pub fn add_package_archive(&mut self, spec: &str, archive: &[u8]) -> Result<(), String> {
        let spec: PackageSpec = spec.parse().map_err(|e| format!("{e}"))?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        let entries = tar.entries().map_err(|e| format!("malformed archive: {e}"))?;
        for entry in entries {
            let mut entry = entry.map_err(|e| format!("malformed archive: {e}"))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            self.insert_package_file(&spec, &path, data)?;
        }
        self.packages.insert(spec);
        Ok(())
    }

    /// Add a package from individual files (paths relative to the package root).
    pub fn add_package_files(
        &mut self,
        spec: &str,
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<(), String> {
        let spec: PackageSpec = spec.parse().map_err(|e| format!("{e}"))?;
        for (path, data) in files {
            self.insert_package_file(&spec, &path, data)?;
        }
        self.packages.insert(spec);
        Ok(())
    }

    fn insert_package_file(&mut self, spec: &PackageSpec, path: &str, data: Vec<u8>) -> Result<(), String> {
        let path = format!("/{}", path.trim_start_matches("./").trim_start_matches('/'));
        let vpath = VirtualPath::new(&path).map_err(|e| format!("{path}: {e}"))?;
        let id = RootedPath::new(VirtualRoot::Package(spec.clone()), vpath).intern();
        if path.ends_with(".typ")
            && let Ok(text) = String::from_utf8(data.clone())
        {
            self.sources.insert(id, Source::new(id, text));
        }
        self.files.insert(id, Bytes::new(data));
        Ok(())
    }

    pub fn compile(&mut self) -> Output {
        let (doc, diagnostics, missing_packages) = self.compile_document();
        let mut out = Output { diagnostics, missing_packages, ..Default::default() };
        if let Some(doc) = doc {
            out.svg = Some(typst_svg::svg_merged(&doc, &Default::default(), Abs::zero()));
            out.page_heights = doc.pages().iter().map(|p| p.frame.height().to_pt()).collect();
            if self.probe.is_some() {
                out.probes = Some(probes_json(&doc));
            }
        }
        out
    }

    /// Compiles `text` as written, not instrumented, and writes it as `format`.
    pub fn export(&mut self, text: &str, format: Format) -> Export {
        self.set_main(text);
        let (doc, diagnostics, missing_packages) = self.compile_document();
        let mut out = Export { diagnostics, missing_packages, ..Default::default() };
        let Some(doc) = doc else { return out };
        out.data = match format {
            Format::Svg => Some(typst_svg::svg_merged(&doc, &Default::default(), Abs::zero()).into_bytes()),
            Format::Png(pixel_per_pt) => {
                let options = typst_render::RenderOptions {
                    pixel_per_pt: Scalar::new(pixel_per_pt.into()),
                    ..Default::default()
                };
                let pixmap = typst_render::render_merged(&doc, &options, Abs::zero(), None);
                match pixmap.encode_png() {
                    Ok(png) => Some(png),
                    Err(err) => {
                        out.diagnostics.push(Diagnostic::message(format!("couldn't encode the PNG: {err}")));
                        None
                    }
                }
            }
            Format::Pdf => match typst_pdf::pdf(&doc, &Default::default()) {
                Ok(pdf) => Some(pdf),
                Err(errors) => {
                    out.diagnostics.extend(errors.iter().map(|d| self.diagnostic(d)));
                    None
                }
            },
        };
        out
    }

    /// Compiles the main source: the document if it succeeded, the
    /// diagnostics, and the packages it's missing.
    fn compile_document(&mut self) -> (Option<PagedDocument>, Vec<Diagnostic>, Vec<String>) {
        self.missing.lock().unwrap().clear();
        let Warned { output, warnings } = typst::compile::<PagedDocument>(self);
        // Bound comemo's cache; recent results stay for incremental recompiles.
        typst::comemo::evict(10);

        let mut diagnostics = Vec::new();
        let doc = match output {
            Ok(doc) => Some(doc),
            Err(errors) => {
                diagnostics.extend(errors.iter().map(|d| self.diagnostic(d)));
                None
            }
        };
        diagnostics.extend(warnings.iter().map(|d| self.diagnostic(d)));
        let missing = self.missing.lock().unwrap().iter().cloned().collect();
        (doc, diagnostics, missing)
    }

    fn diagnostic(&self, diag: &SourceDiagnostic) -> Diagnostic {
        let mut result = Diagnostic {
            error: diag.severity == Severity::Error,
            message: diag.message.to_string(),
            file: None,
            line: None,
            column: None,
        };
        let Some(id) = diag.span.id() else { return result };
        if id == self.main.id() {
            if let Some(range) = self.range(diag.span) {
                let (offset, lines) = match &self.probe {
                    Some(p) => (p.instrumented.to_original(range.start), &p.original),
                    None => (range.start, self.main.lines()),
                };
                if let Some((line, column)) = lines.byte_to_line_column(offset) {
                    result.line = Some(line);
                    result.column = Some(column);
                }
            }
            return result;
        }
        let path = id.get();
        result.file = Some(match path.root() {
            VirtualRoot::Package(spec) => format!("{spec}{}", path.vpath().get_with_slash()),
            VirtualRoot::Project => path.vpath().get_with_slash().to_string(),
        });
        if let (Some(source), Some(range)) = (self.sources.get(&id), self.range(diag.span))
            && let Some((line, column)) = source.lines().byte_to_line_column(range.start)
        {
            result.line = Some(line);
            result.column = Some(column);
        }
        result
    }

    fn not_found(&self, id: FileId) -> FileError {
        let path = id.get();
        match path.root() {
            VirtualRoot::Package(spec) if !self.packages.contains(spec) => {
                self.missing.lock().unwrap().insert(spec.to_string());
                FileError::Package(PackageError::NotFound(spec.clone()))
            }
            _ => FileError::NotFound(path.vpath().get_with_slash().into()),
        }
    }
}

/// Collects the probe metadata from a compiled document as a JSON array.
fn probes_json(doc: &PagedDocument) -> String {
    let label = Label::new(PicoStr::intern("__cetz-editor-probe")).unwrap();
    let values: Vec<_> = doc
        .introspector()
        .query(&Selector::Label(label))
        .iter()
        .filter_map(|c| c.to_packed::<MetadataElem>().map(|m| m.value.clone()))
        .collect();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".into())
}

impl World for EditorWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        if id == self.probe_id {
            return Ok(Source::new(id, include_str!("probe.typ").into()));
        }
        self.sources.get(&id).cloned().ok_or_else(|| self.not_found(id))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            return Ok(Bytes::from_string(self.main.text().to_owned()));
        }
        if id == self.probe_id {
            return Ok(Bytes::from_string(include_str!("probe.typ")));
        }
        self.files.get(&id).cloned().ok_or_else(|| self.not_found(id))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn compiles_plain_typst() {
        let mut world = EditorWorld::new();
        world.set_main("#set page(width: auto, height: auto)\nHello $x^2$");
        let out = world.compile();
        assert!(out.diagnostics.iter().all(|d| !d.error), "{:?}", out.diagnostics);
        assert!(out.svg.unwrap().starts_with("<svg"));
    }

    #[test]
    fn exports_each_format() {
        let mut world = EditorWorld::new();
        let text = "#set page(width: 20pt, height: 10pt)\nHi\n#pagebreak()\nThere";
        let pdf = world.export(text, Format::Pdf).data.unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let svg = world.export(text, Format::Svg).data.unwrap();
        assert!(svg.starts_with(b"<svg"));
        let png = world.export(text, Format::Png(2.0)).data.unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // Both pages stacked, at two pixels per point: 40 x 40.
        assert_eq!(&png[16..24], &[0, 0, 0, 40, 0, 0, 0, 40]);
    }

    #[test]
    fn export_reports_errors_in_the_source() {
        let mut world = EditorWorld::new();
        let out = world.export("ok\n#undefined-thing", Format::Pdf);
        assert!(out.data.is_none());
        assert_eq!((out.diagnostics[0].line, out.diagnostics[0].column), (Some(1), Some(1)));
    }

    #[test]
    fn reports_errors_with_location() {
        let mut world = EditorWorld::new();
        world.set_main("ok\n#undefined-thing");
        let out = world.compile();
        assert!(out.svg.is_none());
        let err = &out.diagnostics[0];
        assert!(err.error && err.file.is_none());
        assert_eq!((err.line, err.column), (Some(1), Some(1)));
    }

    #[test]
    fn reports_missing_packages() {
        let mut world = EditorWorld::new();
        world.set_main("#import \"@preview/cetz:0.5.2\": canvas");
        let out = world.compile();
        assert_eq!(out.missing_packages, ["@preview/cetz:0.5.2"]);
    }

    /// Compiles every `.typ` fixture and bundled example using packages from
    /// the local Typst cache (populated by `typst compile` / `just render`).
    #[test]
    fn compiles_fixtures_from_local_package_cache() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = [root.join("fixtures"), root.join("examples/cetz-gallery")];
        for entry in dirs.iter().flat_map(|dir| std::fs::read_dir(dir).unwrap()) {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "typ") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let mut world = EditorWorld::new();
            world.set_main(&source);
            let plain = compile_with_cache(&mut world, &cache);
            assert!(plain.diagnostics.iter().all(|d| !d.error), "{}: {:?}", path.display(), plain.diagnostics);

            // Instrumenting must not change the rendering.
            world.set_main_probed(&source);
            let probed = compile_with_cache(&mut world, &cache);
            assert!(probed.diagnostics.iter().all(|d| !d.error), "{}: {:?}", path.display(), probed.diagnostics);
            assert_eq!(plain.svg, probed.svg, "{}: probe changed the render", path.display());
            assert!(probed.probes.is_some());
        }
    }

    #[test]
    fn probes_report_geometry_per_call() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let source = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\
            #canvas({\n  import draw: *\n  rect((0, 0), (2, 1), name: \"r\", fill: blue)\n  \
            for x in (0, 1) { circle((x, 3), radius: 0.5) }\n})";
        let mut world = EditorWorld::new();
        world.set_main_probed(source);
        let out = compile_with_cache(&mut world, &cache);
        let probes: serde_json::Value = serde_json::from_str(&out.probes.unwrap()).unwrap();
        let probes = probes.as_array().unwrap();

        let rect_at = source.find("rect(").unwrap() as u64;
        let circle_at = source.find("circle(").unwrap() as u64;
        let ids: Vec<u64> = probes.iter().map(|p| p["id"].as_u64().unwrap()).collect();
        // One probe for the rect, one per loop iteration for the circle.
        assert_eq!(ids, [rect_at, circle_at, circle_at]);

        let rect = &probes[0];
        assert_eq!(rect["name"], "r");
        assert_point(&rect["anchors"]["north-east"], [2.0, 1.0]);
        assert_eq!(rect["drawables"][0]["type"], "path");
        assert_eq!(rect["drawables"][0]["fill"], "#0074d9");
        assert_eq!(probes[1]["drawables"][0]["fill"], serde_json::Value::Null);
        assert_point(&probes[2]["anchors"]["center"], [1.0, 3.0]);
    }

    /// A drawing function's calls report once per use, under their own
    /// offset, as well as under the use's.
    #[test]
    fn probes_calls_inside_drawing_functions() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let source = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\
            #import draw: *\n\
            #let plate(x) = {\n  rect((x, 0), (x + 1, 2))\n  content((x, 3), [+])\n}\n\
            #let pair(x) = group({ circle((x, 0)); circle((x + 1, 0)) })\n\
            #canvas({\n  plate(0)\n  plate(4)\n  pair(8)\n})";
        let mut world = EditorWorld::new();
        world.set_main_probed(source);
        let out = compile_with_cache(&mut world, &cache);
        assert!(out.diagnostics.iter().all(|d| !d.error), "{:?}", out.diagnostics);
        let probes: serde_json::Value = serde_json::from_str(&out.probes.unwrap()).unwrap();
        let probes = probes.as_array().unwrap();
        let count = |needle: &str| {
            let at = source.find(needle).unwrap() as u64;
            probes.iter().filter(|p| p["id"].as_u64() == Some(at)).collect::<Vec<_>>()
        };
        let rects = count("rect(");
        assert_eq!(rects.len(), 2);
        assert_point(&rects[0]["anchors"]["north-east"], [1.0, 2.0]);
        assert_point(&rects[1]["anchors"]["north-east"], [5.0, 2.0]);
        assert_eq!(count("content(").len(), 2);
        // Each use reports its rect and its content.
        assert_eq!(count("plate(0)").len(), 2);
        assert_eq!(count("circle((x, 0))").len(), 1);
        assert_eq!(count("group(").len(), 1);
    }

    /// An open arc has no border at some compass directions, and asking
    /// CeTZ for one panics.
    #[test]
    fn probes_open_arcs_without_compass_anchors() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let source = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\
            #canvas({\n  import draw: *\n  arc((0, 0), start: 0deg, stop: 60deg, anchor: \"origin\")\n  \
            arc((3, 0), start: 0deg, stop: 30deg, mode: \"PIE\")\n  group(name: \"g\", line((0, 3), (2, 4)))\n})";
        let mut world = EditorWorld::new();
        world.set_main_probed(source);
        let out = compile_with_cache(&mut world, &cache);
        assert!(out.diagnostics.iter().all(|d| !d.error), "{:?}", out.diagnostics);
        let probes: serde_json::Value = serde_json::from_str(&out.probes.unwrap()).unwrap();
        let [open, pie, group] = probes.as_array().unwrap().as_slice() else { panic!("expected three probes") };
        assert_point(&open["anchors"]["origin"], [0.0, 0.0]);
        assert!(open["anchors"].get("north").is_none());
        // Closed arcs and groups around open paths keep theirs.
        assert!(pie["anchors"].get("north").is_some());
        assert!(group["anchors"].get("north").is_some());
    }

    /// CeTZ 0.5.2 offers a centroid for open lines too, and divides by zero
    /// finding it when they enclose no area, as elbow connectors often do:
    /// points in a row, or a Z.
    #[test]
    fn probes_open_lines_without_a_centroid() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let source = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\
            #canvas({\n  import draw: *\n  line((0, 0), (0, -1), (0, -1), (0, -2), mark: (end: \">\"))\n  \
            line((6.5, 0), (6.5, -2), (5.5, -2), (5.5, -4), mark: (end: \">\"))\n  line((0, 0), (2, 0), (2, 1), close: true)\n})";
        let mut world = EditorWorld::new();
        world.set_main_probed(source);
        let out = compile_with_cache(&mut world, &cache);
        assert!(out.diagnostics.iter().all(|d| !d.error), "{:?}", out.diagnostics);
        let probes: serde_json::Value = serde_json::from_str(&out.probes.unwrap()).unwrap();
        let [flat, zed, closed] = probes.as_array().unwrap().as_slice() else { panic!("expected three probes") };
        assert!(flat["anchors"].get("centroid").is_none());
        assert_point(&flat["anchors"]["end"], [0.0, -2.0]);
        assert!(zed["anchors"].get("centroid").is_none());
        assert!(closed["anchors"].get("centroid").is_some());
    }

    /// CeTZ 0.5.2's n-star panics on its own corner and edge anchors, so the
    /// probe reads them off the outline, as it does for polygons.
    #[test]
    fn probes_corners_from_outlines() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let source = "#import \"@preview/cetz:0.5.2\": canvas, draw\n\
            #canvas({\n  import draw: *\n  polygon((0, 0), 4, radius: 1)\n  n-star((3, 0), 5, radius: 1)\n})";
        let mut world = EditorWorld::new();
        world.set_main_probed(source);
        let out = compile_with_cache(&mut world, &cache);
        assert!(out.diagnostics.iter().all(|d| !d.error), "{:?}", out.diagnostics);
        let probes: serde_json::Value = serde_json::from_str(&out.probes.unwrap()).unwrap();
        let [square, star] = probes.as_array().unwrap().as_slice() else { panic!("expected two probes") };
        assert_point(&square["anchors"]["corner-0"], [1.0, 0.0]);
        assert_point(&square["anchors"]["corner-1"], [0.0, 1.0]);
        assert_point(&square["anchors"]["edge-0"], [0.5, 0.5]);
        assert_point(&square["anchors"]["edge-3"], [0.5, -0.5]);
        // Ten corners, inner then outer, each edge between two.
        let point = |name: &str| star["anchors"][name].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        let (c0, c1) = (point("corner-0"), point("corner-1"));
        assert_point(&star["anchors"]["corner-0"], [3.5, 0.0]);
        assert!(((c1[0] - 3.0).hypot(c1[1]) - 1.0).abs() < 1e-6, "{c1:?}");
        assert_point(&star["anchors"]["edge-0"], [(c0[0] + c1[0]) / 2.0, (c0[1] + c1[1]) / 2.0]);
        assert!(star["anchors"].get("corner-9").is_some());
    }

    /// CeTZ computes border anchors by intersection, so allow float noise.
    fn assert_point(value: &serde_json::Value, expected: [f64; 2]) {
        let p: Vec<f64> = value.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(
            (p[0] - expected[0]).abs() < 1e-4 && (p[1] - expected[1]).abs() < 1e-4,
            "{p:?} != {expected:?}"
        );
    }

    #[test]
    fn probed_diagnostics_point_into_the_original() {
        let mut world = EditorWorld::new();
        world.set_main_probed("ok\n#undefined-thing");
        let out = world.compile();
        let err = &out.diagnostics[0];
        assert!(err.file.is_none());
        assert_eq!((err.line, err.column), (Some(1), Some(1)));
    }

    fn compile_with_cache(world: &mut EditorWorld, cache: &Path) -> Output {
        loop {
            let out = world.compile();
            if out.missing_packages.is_empty() {
                return out;
            }
            for spec in &out.missing_packages {
                let dir = package_dir(cache, spec);
                assert!(dir.exists(), "{spec} not in local cache; run `just render` first");
                world.add_package_files(spec, read_dir_files(&dir, &dir)).unwrap();
            }
        }
    }

    fn typst_package_cache() -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(if cfg!(target_os = "macos") {
            home.join("Library/Caches/typst/packages")
        } else {
            home.join(".cache/typst/packages")
        })
    }

    fn package_dir(cache: &Path, spec: &str) -> PathBuf {
        let spec: PackageSpec = spec.parse().unwrap();
        cache.join(spec.namespace.as_str()).join(spec.name.as_str()).join(spec.version.to_string())
    }

    fn read_dir_files(root: &Path, dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(read_dir_files(root, &path));
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                files.push((rel, std::fs::read(&path).unwrap()));
            }
        }
        files
    }
}
