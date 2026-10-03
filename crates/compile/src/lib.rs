//! An in-memory Typst world: one main source, embedded fonts, and packages
//! supplied by the host (the browser fetches them; tests read the local cache).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::sync::Mutex;

use typst::diag::{FileError, FileResult, PackageError, Severity, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::Abs;
use typst_layout::PagedDocument;
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
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

pub struct EditorWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
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
            sources: HashMap::new(),
            files: HashMap::new(),
            packages: HashSet::new(),
            missing: Mutex::new(BTreeSet::new()),
        }
    }

    /// Replace the main source text. Reparses incrementally.
    pub fn set_main(&mut self, text: &str) {
        self.main.replace(text);
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
        self.missing.lock().unwrap().clear();
        let Warned { output, warnings } = typst::compile::<PagedDocument>(self);
        // Bound comemo's cache; recent results stay for incremental recompiles.
        typst::comemo::evict(10);

        let mut out = Output::default();
        match output {
            Ok(doc) => {
                out.svg = Some(typst_svg::svg_merged(&doc, &Default::default(), Abs::zero()));
            }
            Err(errors) => out.diagnostics.extend(errors.iter().map(|d| self.diagnostic(d))),
        }
        out.diagnostics.extend(warnings.iter().map(|d| self.diagnostic(d)));
        out.missing_packages = self.missing.lock().unwrap().iter().cloned().collect();
        out
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
        let source = if id == self.main.id() { Some(&self.main) } else { self.sources.get(&id) };
        if id != self.main.id() {
            let path = id.get();
            result.file = Some(match path.root() {
                VirtualRoot::Package(spec) => format!("{spec}{}", path.vpath().get_with_slash()),
                VirtualRoot::Project => path.vpath().get_with_slash().to_string(),
            });
        }
        if let (Some(source), Some(range)) = (source, self.range(diag.span))
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
        self.sources.get(&id).cloned().ok_or_else(|| self.not_found(id))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            return Ok(Bytes::from_string(self.main.text().to_owned()));
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

    /// Compiles every `.typ` fixture using packages from the local Typst
    /// cache (populated by `typst compile` / `just render`).
    #[test]
    fn compiles_fixtures_from_local_package_cache() {
        let Some(cache) = typst_package_cache().filter(|p| p.exists()) else {
            eprintln!("skipping: no local Typst package cache");
            return;
        };
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        for entry in std::fs::read_dir(fixtures).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "typ") {
                continue;
            }
            let mut world = EditorWorld::new();
            world.set_main(&std::fs::read_to_string(&path).unwrap());
            let out = loop {
                let out = world.compile();
                if out.missing_packages.is_empty() {
                    break out;
                }
                for spec in &out.missing_packages {
                    let dir = package_dir(&cache, spec);
                    assert!(dir.exists(), "{spec} not in local cache; run `just render` first");
                    world.add_package_files(spec, read_dir_files(&dir, &dir)).unwrap();
                }
            };
            assert!(out.diagnostics.iter().all(|d| !d.error), "{}: {:?}", path.display(), out.diagnostics);
            assert!(out.svg.is_some());
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
