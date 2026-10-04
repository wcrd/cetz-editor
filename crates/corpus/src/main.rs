//! Runs the editor's own layer over a corpus of real CeTZ diagrams and writes
//! `crates/corpus/report.md`. Typst and CeTZ render these fine on their own;
//! what's checked is what the editor adds on top:
//!
//! - **Probe**: the instrumented compile the editor runs on open must
//!   succeed and render exactly like the plain one.
//! - **Coverage**: which shapes you can drag, and why the rest can't be.
//! - **Edits**: moving, restyling, duplicating, grouping and rotating a sample
//!   of shapes must compile, land where expected, and reverse cleanly.
//!
//! Usage: `cetz-corpus [dir ...]` (default: `examples/` and `corpus/`).
//! Packages come from the local Typst cache; `just corpus-fetch` fills it.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use cetz_compile::{EditorWorld, Output};
use cetz_scene::{Call, Edit, EditResult, Scene, Value, VariableKind};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Shapes per diagram that get the edit round-trips.
const SAMPLE: usize = 6;
/// The move every round-trip uses, in the call's own units.
const DX: f64 = 0.5;
const DY: f64 = 0.25;
/// Shapes whose style the edits set: those CeTZ gives a `stroke`.
const STROKED: &[&str] = &[
    "rect", "circle", "line", "arc", "bezier", "bezier-through", "catmull", "hobby", "content", "polygon",
    "n-star", "merge-path", "grid", "rect-around",
];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cache) = package_cache().filter(|p| p.exists()) else {
        eprintln!("no local Typst package cache; run `just corpus-fetch` first");
        std::process::exit(1);
    };

    // A worker: check one diagram and print its report as JSON.
    if let [flag, path, name] = args.as_slice()
        && flag == "--one"
    {
        // Panics are recorded as outcomes, not printed.
        std::panic::set_hook(Box::new(|_| {}));
        let mut compiler = Compiler { world: EditorWorld::new(), cache };
        let source = std::fs::read_to_string(path).unwrap();
        println!("{}", serde_json::to_string(&check(&mut compiler, name.clone(), source)).unwrap());
        return;
    }

    let dirs: Vec<PathBuf> =
        if args.is_empty() { vec![root.join("examples"), root.join("corpus")] } else { args.iter().map(PathBuf::from).collect() };
    let mut files = Vec::new();
    for dir in &dirs {
        collect(dir, &mut files);
    }
    files.sort();
    if files.is_empty() {
        eprintln!("no .typ files found; run `just corpus-fetch` first");
        std::process::exit(1);
    }

    // One process per diagram: Typst's memoization cache is global, and
    // threads compiling in one process mostly wait on each other.
    let started = Instant::now();
    let exe = std::env::current_exe().unwrap();
    let next = AtomicUsize::new(0);
    let reports = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let name = path.strip_prefix(&root).unwrap_or(path).display().to_string();
                    let out = Command::new(&exe).arg("--one").arg(path).arg(&name).output().unwrap();
                    let report = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| Report::crashed(name, &out.stderr));
                    let mut reports = reports.lock().unwrap();
                    reports.push(report);
                    eprint!("\r{}/{}", reports.len(), files.len());
                }
            });
        }
    });
    eprintln!();
    let mut reports = reports.into_inner().unwrap();
    reports.sort_by(|a, b| a.name.cmp(&b.name));

    let text = render(&reports);
    let out = root.join("crates/corpus/report.md");
    std::fs::write(&out, &text).unwrap();
    print!("{}", text.split("\n## ").next().unwrap_or(""));
    println!("\nwrote {} in {}s", out.display(), started.elapsed().as_secs());
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        // Symlinked directories (janosh's site/static) would count diagrams twice.
        if entry.file_type().unwrap().is_dir() && path.file_name().is_some_and(|n| n != ".git") {
            collect(&path, files);
        } else if path.extension().is_some_and(|e| e == "typ") {
            files.push(path);
        }
    }
}

// --- Compiling -------------------------------------------------------------

struct Compiler {
    world: EditorWorld,
    cache: PathBuf,
}

impl Compiler {
    /// Compiles `text`, adding packages from the local cache as needed.
    fn compile(&mut self, text: &str, probed: bool) -> Result<Output, String> {
        if probed {
            self.world.set_main_probed(text);
        } else {
            self.world.set_main(text);
        }
        loop {
            let out = self.world.compile();
            if out.missing_packages.is_empty() {
                return match out.diagnostics.iter().find(|d| d.error) {
                    Some(d) => {
                        let file = d.file.as_deref().unwrap_or("main");
                        Err(format!("{} ({file}{})", d.message, d.line.map_or(String::new(), |l| format!(":{}", l + 1))))
                    }
                    None => Ok(out),
                };
            }
            for spec in &out.missing_packages {
                let dir = package_dir(&self.cache, spec);
                if !dir.exists() {
                    return Err(format!("{spec} isn't in the local package cache"));
                }
                self.world.add_package_files(spec, read_dir_files(&dir, &dir))?;
            }
        }
    }
}

fn package_cache() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") { home.join("Library/Caches/typst/packages") } else { home.join(".cache/typst/packages") })
}

/// `@preview/cetz:0.5.2` → `<cache>/preview/cetz/0.5.2`.
fn package_dir(cache: &Path, spec: &str) -> PathBuf {
    let (namespace, rest) = spec.trim_start_matches('@').split_once('/').unwrap_or(("", spec));
    let (name, version) = rest.split_once(':').unwrap_or((rest, ""));
    cache.join(namespace).join(name).join(version)
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

// --- Checking one diagram ----------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Report {
    name: String,
    load: Load,
    /// Probes whose id isn't a call the scene parser found.
    orphan_probes: usize,
    /// Calls that draw something.
    shapes: usize,
    draggable: usize,
    /// Why the other shapes can't be dragged, with an example of each.
    blocked: BTreeMap<String, (usize, String)>,
    edits: Vec<Trial>,
}

#[derive(Serialize, Deserialize)]
enum Load {
    /// Fails to compile without the editor too: not the editor's problem.
    Broken(String),
    ProbeBreaks(String),
    ProbeChangesRender,
    Ok,
}

#[derive(Serialize, Deserialize)]
struct Trial {
    edit: String,
    line: usize,
    call: String,
    outcome: Outcome,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Outcome {
    Ok,
    /// The edit declined, as it's allowed to.
    Refused,
    /// Reversing it gives different text but the same drawing.
    InexactUndo,
    BrokeCompile,
    WrongGeometry,
    ChangedRender,
    NotReversible,
    Panicked,
}

impl Outcome {
    fn is_failure(self) -> bool {
        self > Outcome::InexactUndo
    }

    fn label(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
            Outcome::InexactUndo => "inexact undo",
            Outcome::BrokeCompile => "broke compile",
            Outcome::WrongGeometry => "wrong geometry",
            Outcome::ChangedRender => "changed render",
            Outcome::NotReversible => "not reversible",
            Outcome::Panicked => "panicked",
        }
    }
}

impl Report {
    /// A worker that died without a report (a Typst panic, say).
    fn crashed(name: String, stderr: &[u8]) -> Self {
        let message = String::from_utf8_lossy(stderr).lines().last().unwrap_or("no output").to_string();
        Report {
            name,
            load: Load::ProbeBreaks(format!("the check crashed: {message}")),
            orphan_probes: 0,
            shapes: 0,
            draggable: 0,
            blocked: BTreeMap::new(),
            edits: Vec::new(),
        }
    }
}

fn check(compiler: &mut Compiler, name: String, source: String) -> Report {
    let mut report = Report {
        name,
        load: Load::Ok,
        orphan_probes: 0,
        shapes: 0,
        draggable: 0,
        blocked: BTreeMap::new(),
        edits: Vec::new(),
    };
    let plain = match compiler.compile(&source, false) {
        Ok(out) => out,
        Err(e) => {
            report.load = Load::Broken(e);
            return report;
        }
    };
    let probed = match compiler.compile(&source, true) {
        Ok(out) => out,
        Err(e) => {
            report.load = Load::ProbeBreaks(e);
            return report;
        }
    };
    if probed.svg != plain.svg {
        report.load = Load::ProbeChangesRender;
    }
    let probes = probes_by_call(&probed);
    let Ok(scene) = catch_unwind(|| cetz_scene::parse(&source)) else {
        report.load = Load::ProbeBreaks("the scene parser panicked".into());
        return report;
    };
    let calls = all_calls(&scene);
    report.orphan_probes = probes.keys().filter(|id| !calls.iter().any(|c| c.id == **id)).count();

    // Coverage: every shape is draggable, or blocked for a reason.
    let mut draggable = Vec::new();
    for (ordinal, call) in calls.iter().enumerate() {
        let draws = probes.get(&call.id).is_some_and(|ps| ps.iter().any(|p| p["drawables"].as_array().is_some_and(|d| !d.is_empty())));
        if !draws {
            continue;
        }
        report.shapes += 1;
        match blocked(&source, &scene, call) {
            None => draggable.push(ordinal),
            Some(reason) => {
                let entry = report.blocked.entry(reason).or_insert_with(|| (0, snippet(&source, call)));
                entry.0 += 1;
            }
        }
    }
    report.draggable = draggable.len();

    // Edits on an even spread of draggable shapes.
    let mut cx = Cx { compiler, source: &source, plain_svg: plain.svg.as_deref().unwrap_or(""), probes: &probes };
    let step = (draggable.len() as f64 / SAMPLE as f64).max(1.0);
    let sample: Vec<usize> = (0..SAMPLE.min(draggable.len())).map(|i| draggable[(i as f64 * step) as usize]).collect();
    for ordinal in sample {
        let call = calls[ordinal];
        for edit in ["move", "restyle", "duplicate", "group", "rotate"] {
            if let Some((outcome, detail)) = cx.trial(edit, &scene, ordinal) {
                report.edits.push(Trial { edit: edit.into(), line: line_of(&source, call.id), call: snippet(&source, call), outcome, detail });
            }
        }
    }
    if let Some((outcome, detail)) = cx.round_trip(Edit::GatherAnchors, true, |_| None, |_, _| None) {
        report.edits.push(Trial { edit: "gather anchors".into(), line: 0, call: String::new(), outcome, detail });
    }
    report
}

/// Calls in canvases and in the drawing functions they call.
fn all_calls(scene: &Scene) -> Vec<&Call> {
    scene.canvases.iter().flat_map(|c| &c.calls).chain(scene.functions.iter().flat_map(|f| &f.calls)).collect()
}

fn probes_by_call(out: &Output) -> HashMap<usize, Vec<Json>> {
    let mut by_call: HashMap<usize, Vec<Json>> = HashMap::new();
    let probes: Json = out.probes.as_deref().and_then(|p| serde_json::from_str(p).ok()).unwrap_or(Json::Null);
    for p in probes.as_array().into_iter().flatten() {
        if let Some(id) = p["id"].as_u64() {
            by_call.entry(id as usize).or_default().push(p.clone());
        }
    }
    by_call
}

fn base_name(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

/// Why dragging the shape does nothing, if it doesn't.
fn blocked(source: &str, scene: &Scene, call: &Call) -> Option<String> {
    if call.in_loop {
        return Some("in a loop".into());
    }
    let edit = Edit::Move { calls: vec![call.id], dx: DX, dy: DY, detach: false };
    match catch_unwind(|| cetz_scene::apply(source, &edit)) {
        Err(_) => return Some("move panicked".into()),
        Ok(Err(e)) => return Some(format!("move refused: {e}")),
        Ok(Ok(r)) if !r.patches.is_empty() => return None,
        Ok(Ok(_)) => {}
    }
    let base = base_name(&call.callee);
    if scene.variables.iter().any(|v| v.kind == VariableKind::Function && v.name == base) {
        return Some("calls a function the file defines".into());
    }
    let positional = positions(call);
    if positional.is_empty() {
        return Some("no coordinates".into());
    }
    if positional.iter().all(|a| matches!(a.value, Value::Str { .. })) {
        return Some("placed only by anchor names".into());
    }
    let text = positional.iter().find(|a| matches!(a.value, Value::Expr)).map_or("", |a| a.text.trim());
    Some(
        if text.starts_with("(rel") {
            "computed coordinates: relative `(rel: ..)`"
        } else if text.starts_with('(') && text.contains(':') {
            "computed coordinates: dictionary (polar, ...)"
        } else if text.starts_with('(') {
            "computed coordinates: tuple of expressions"
        } else if text.chars().all(|c| c.is_alphanumeric() || "_-.".contains(c)) {
            "computed coordinates: variable"
        } else if text.contains('(') {
            "computed coordinates: function call"
        } else {
            "computed coordinates: other expression"
        }
        .into(),
    )
}

/// The positional arguments that place the call: not text, and only the
/// first one or two of `content(..)`, whose last is its body.
fn positions(call: &Call) -> Vec<&cetz_scene::Arg> {
    let mut positional: Vec<_> = call.args.iter().filter(|a| a.key.is_none()).collect();
    if base_name(&call.callee) == "content" && !positional.is_empty() {
        positional.truncate(if positional.len() >= 3 { 2 } else { 1 });
    }
    positional
        .into_iter()
        .filter(|a| !matches!(a.value, Value::Content { .. } | Value::Number { .. }))
        .filter(|a| !(matches!(a.value, Value::Expr) && a.text.trim_start().starts_with(['$', '['])))
        .collect()
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

/// The call on one line, shortened.
fn snippet(source: &str, call: &Call) -> String {
    let text = source[call.range.clone()].split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 70 { format!("{}…", text.chars().take(70).collect::<String>()) } else { text.to_string() }
}

/// The context round-trips run in: the diagram as loaded.
struct Cx<'a> {
    compiler: &'a mut Compiler,
    source: &'a str,
    plain_svg: &'a str,
    probes: &'a HashMap<usize, Vec<Json>>,
}

impl Cx<'_> {
    /// One edit and its reverse on the call at `ordinal`, or `None` when the
    /// edit doesn't apply to it.
    fn trial(&mut self, edit: &str, scene: &Scene, ordinal: usize) -> Option<(Outcome, String)> {
        let calls = all_calls(scene);
        let call = calls[ordinal];
        let id = call.id;
        // The same call after an edit that keeps the calls' order.
        let after = move |r: &EditResult| -> Option<usize> { all_calls(&cetz_scene::parse(&r.source)).get(ordinal).map(|c| c.id) };
        match edit {
            "move" => {
                let rigid = rigid(scene, call);
                let before = self.probes.get(&id).cloned().unwrap_or_default();
                self.round_trip(
                    Edit::Move { calls: vec![id], dx: DX, dy: DY, detach: false },
                    false,
                    |r| after(r).map(|id| Edit::Move { calls: vec![id], dx: -DX, dy: -DY, detach: false }),
                    |r, out| if rigid { check_moved(&before, &probes_by_call(out), after(r)?) } else { None },
                )
            }
            "restyle" => {
                if !STROKED.contains(&base_name(&call.callee)) {
                    return None;
                }
                let old = call.args.iter().find(|a| a.key.as_deref() == Some("stroke")).map(|a| a.text.clone());
                let restore = move |r: &EditResult| after(r).map(|id| Edit::SetNamed { call: id, key: "stroke".into(), text: old.clone() });
                self.round_trip(Edit::SetNamed { call: id, key: "stroke".into(), text: Some("red".into()) }, false, restore, |_, _| None)
            }
            "duplicate" => self.round_trip(
                Edit::Duplicate { calls: vec![id], dx: DX, dy: DY },
                false,
                |r| Some(Edit::Delete { calls: r.created.clone() }),
                |_, _| None,
            ),
            "group" => self.round_trip(Edit::Group { calls: vec![id] }, true, |r| Some(Edit::Ungroup { calls: r.created.clone() }), |_, _| None),
            "rotate" => {
                let (x, y) = call.args.iter().find_map(|a| match a.value {
                    Value::Coord { x, y, .. } => Some((x, y)),
                    _ => None,
                })?;
                self.round_trip(
                    Edit::Rotate { call: id, angle: 30.0, x, y },
                    false,
                    // The new scope, or the shape when it was already in one.
                    |r| r.created.first().copied().or_else(|| after(r)).map(|call| Edit::Unrotate { call }),
                    |_, _| None,
                )
            }
            _ => unreachable!(),
        }
    }

    /// Applies `forward`, compiles the result (it must render like the
    /// original when `same_render`), runs `verify` on it, then applies the
    /// reverse edit and expects the original back.
    fn round_trip(
        &mut self,
        forward: Edit,
        same_render: bool,
        reverse: impl FnOnce(&EditResult) -> Option<Edit>,
        verify: impl FnOnce(&EditResult, &Output) -> Option<String>,
    ) -> Option<(Outcome, String)> {
        let fwd = match catch_unwind(AssertUnwindSafe(|| cetz_scene::apply(self.source, &forward))) {
            Err(e) => return Some((Outcome::Panicked, panic_message(e))),
            Ok(Err(e)) => return Some((Outcome::Refused, e)),
            Ok(Ok(r)) => r,
        };
        let out = match self.compiler.compile(&fwd.source, true) {
            Ok(out) => out,
            Err(e) => return Some((Outcome::BrokeCompile, e)),
        };
        if same_render && out.svg.as_deref() != Some(self.plain_svg) {
            return Some((Outcome::ChangedRender, String::new()));
        }
        if let Some(problem) = catch_unwind(AssertUnwindSafe(|| verify(&fwd, &out))).unwrap_or(Some("check panicked".into())) {
            return Some((Outcome::WrongGeometry, problem));
        }
        let Some(back) = catch_unwind(AssertUnwindSafe(|| reverse(&fwd))).ok().flatten() else {
            return Some((Outcome::Ok, String::new()));
        };
        let undone = match catch_unwind(AssertUnwindSafe(|| cetz_scene::apply(&fwd.source, &back))) {
            Err(e) => return Some((Outcome::Panicked, format!("reversing: {}", panic_message(e)))),
            Ok(Err(e)) => return Some((Outcome::NotReversible, format!("reverse refused: {e}"))),
            Ok(Ok(r)) => r.source,
        };
        if undone == self.source {
            return Some((Outcome::Ok, String::new()));
        }
        let diff = first_difference(self.source, &undone);
        match self.compiler.compile(&undone, false) {
            Ok(out) if out.svg.as_deref() == Some(self.plain_svg) => Some((Outcome::InexactUndo, diff)),
            Ok(_) => Some((Outcome::NotReversible, diff)),
            Err(e) => Some((Outcome::NotReversible, format!("reversed source doesn't compile: {e}"))),
        }
    }
}

/// A failure's detail without what varies between instances: where it
/// happened and by how much.
fn cause(detail: &str) -> String {
    let first = detail.lines().next().unwrap_or("");
    if first.starts_with("moved (") {
        return "moved by the wrong amount".into();
    }
    if first.starts_with('`') {
        return "different source after reversing".into();
    }
    match first.rfind(" (") {
        Some(i) if first.ends_with(')') => first[..i].to_string(),
        _ => first.to_string(),
    }
}

fn panic_message(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// `was → now` around where two texts first differ.
fn first_difference(a: &str, b: &str) -> String {
    let start = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let start = (0..=start).rev().find(|&i| a.is_char_boundary(i) && b.is_char_boundary(i)).unwrap_or(0);
    let cut = |s: &str| s[start..].chars().take(40).collect::<String>().replace('\n', "⏎");
    format!("`{}` → `{}`", cut(a), cut(b))
}

/// Whether moving the call's literal coordinates moves its whole drawing
/// rigidly: it has some, nothing else positions it, and it has no children.
fn rigid(scene: &Scene, call: &Call) -> bool {
    let positional: Vec<_> = call.args.iter().filter(|a| a.key.is_none()).collect();
    positional.iter().any(|a| matches!(a.value, Value::Coord { .. }))
        && positional.iter().all(|a| matches!(a.value, Value::Coord { .. } | Value::Content { .. } | Value::Number { .. }))
        && !all_calls(scene).iter().any(|c| c.parent == Some(call.id))
}

/// Checks the call's drawables moved by `(DX, DY)` through its transform.
fn check_moved(before: &[Json], after: &HashMap<usize, Vec<Json>>, id: usize) -> Option<String> {
    let after = after.get(&id)?;
    if before.len() != after.len() {
        return Some(format!("{} probes became {}", before.len(), after.len()));
    }
    for (b, a) in before.iter().zip(after) {
        let m = &b["transform"];
        let t = |r: usize, c: usize| m[r][c].as_f64().unwrap_or(0.0);
        let want = [0, 1, 2].map(|r| t(r, 0) * DX + t(r, 1) * DY);
        let (pb, pa) = (points(b), points(a));
        if pb.len() != pa.len() {
            return Some(format!("{} points became {}", pb.len(), pa.len()));
        }
        for (p, q) in pb.iter().zip(&pa) {
            let got = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            if got.iter().zip(&want).any(|(g, w)| (g - w).abs() > 1e-3) {
                return Some(format!("moved ({:.3}, {:.3}), expected ({:.3}, {:.3})", got[0], got[1], want[0], want[1]));
            }
        }
    }
    None
}

/// Every point of a probe's drawables, in canvas units.
fn points(probe: &Json) -> Vec<[f64; 3]> {
    let vec = |v: &Json| {
        let a = v.as_array().map(Vec::as_slice).unwrap_or(&[]);
        [0, 1, 2].map(|i| a.get(i).and_then(Json::as_f64).unwrap_or(0.0))
    };
    let mut out = Vec::new();
    for d in probe["drawables"].as_array().into_iter().flatten() {
        if d["type"] == "path" {
            for sub in d["segments"].as_array().into_iter().flatten() {
                out.push(vec(&sub[0]));
                for seg in sub[2].as_array().into_iter().flatten() {
                    out.extend(seg.as_array().into_iter().flatten().skip(1).map(vec));
                }
            }
        } else {
            out.push(vec(&d["pos"]));
        }
    }
    out
}

// --- The report ----------------------------------------------------------------

fn render(reports: &[Report]) -> String {
    let mut s = String::new();
    let loaded: Vec<_> = reports.iter().filter(|r| !matches!(r.load, Load::Broken(_))).collect();
    let broken = reports.len() - loaded.len();
    let probe_failures: Vec<_> = loaded.iter().filter(|r| !matches!(r.load, Load::Ok) || r.orphan_probes > 0).collect();
    let shapes: usize = loaded.iter().map(|r| r.shapes).sum();
    let draggable: usize = loaded.iter().map(|r| r.draggable).sum();
    let trials: Vec<(&Report, &Trial)> = loaded.iter().flat_map(|r| r.edits.iter().map(move |t| (*r, t))).collect();
    let failures: Vec<_> = trials.iter().filter(|(_, t)| t.outcome.is_failure()).collect();
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };

    let _ = writeln!(s, "# Corpus report\n");
    let _ = writeln!(s, "Generated by `just corpus`. See `src/main.rs` for what each check means.\n");
    let _ = writeln!(s, "- **Diagrams:** {} ({} don't compile even without the editor and are skipped)", reports.len(), broken);
    let _ = writeln!(s, "- **Probe:** {} of {} load and render unchanged", loaded.len() - probe_failures.len(), loaded.len());
    let _ = writeln!(s, "- **Coverage:** {draggable} of {shapes} shapes draggable ({:.0}%)", pct(draggable, shapes));
    let _ = writeln!(s, "- **Edits:** {} round-trips, {} failures", trials.len(), failures.len());

    let _ = writeln!(s, "\n## Probe\n");
    if probe_failures.is_empty() {
        let _ = writeln!(s, "Every diagram loads with the probe and renders unchanged.");
    }
    for r in &probe_failures {
        let problem = match &r.load {
            Load::ProbeBreaks(e) => format!("probe breaks the compile: {e}"),
            Load::ProbeChangesRender => "probe changes the render".into(),
            _ => format!("{} probes for calls the scene doesn't have", r.orphan_probes),
        };
        let _ = writeln!(s, "- `{}`: {problem}", r.name);
    }
    if broken > 0 {
        let _ = writeln!(s, "\nSkipped (don't compile without the editor either):\n");
        for r in reports {
            if let Load::Broken(e) = &r.load {
                let _ = writeln!(s, "- `{}`: {}", r.name, e.lines().next().unwrap_or(""));
            }
        }
    }

    let _ = writeln!(s, "\n## Coverage\n");
    let _ = writeln!(s, "Why shapes can't be dragged, across all diagrams:\n");
    let mut reasons: BTreeMap<&str, (usize, usize, &str)> = BTreeMap::new();
    for r in &loaded {
        for (reason, (n, example)) in &r.blocked {
            let e = reasons.entry(reason).or_insert((0, 0, example));
            e.0 += n;
            e.1 += 1;
        }
    }
    let mut reasons: Vec<_> = reasons.into_iter().collect();
    reasons.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
    let _ = writeln!(s, "| Reason | Shapes | Diagrams | Example |\n|---|---:|---:|---|");
    for (reason, (n, files, example)) in reasons {
        let _ = writeln!(s, "| {reason} | {n} | {files} | `{}` |", example.replace('|', "\\|").replace('`', "'"));
    }
    let _ = writeln!(s, "\nPer diagram:\n\n| Diagram | Shapes | Draggable | Main blocker |\n|---|---:|---:|---|");
    for r in &loaded {
        let main = r.blocked.iter().max_by_key(|(_, (n, _))| *n).map_or(String::new(), |(reason, (n, _))| format!("{reason} ({n})"));
        let _ = writeln!(s, "| `{}` | {} | {} ({:.0}%) | {} |", r.name, r.shapes, r.draggable, pct(r.draggable, r.shapes), main);
    }

    let _ = writeln!(s, "\n## Edits\n");
    let _ = writeln!(
        s,
        "Up to {SAMPLE} draggable shapes per diagram get each edit and its reverse: move by ({DX}, {DY}) and back, \
         set `stroke` and restore it, duplicate and delete, group and ungroup (must render unchanged), rotate 30° and \
         unrotate. Gather anchors runs once per diagram and must render unchanged. Refusing is allowed.\n"
    );
    let outcomes = [
        Outcome::Ok,
        Outcome::Refused,
        Outcome::InexactUndo,
        Outcome::BrokeCompile,
        Outcome::WrongGeometry,
        Outcome::ChangedRender,
        Outcome::NotReversible,
        Outcome::Panicked,
    ];
    let _ = write!(s, "| Edit |");
    for o in outcomes {
        let _ = write!(s, " {} |", o.label());
    }
    let _ = write!(s, "\n|---|");
    for _ in outcomes {
        let _ = write!(s, "---:|");
    }
    let _ = writeln!(s);
    for edit in ["move", "restyle", "duplicate", "group", "rotate", "gather anchors"] {
        let _ = write!(s, "| {edit} |");
        for o in outcomes {
            let n = trials.iter().filter(|(_, t)| t.edit == edit && t.outcome == o).count();
            let _ = write!(s, " {} |", if n == 0 { String::new() } else { n.to_string() });
        }
        let _ = writeln!(s);
    }
    let detailed = |s: &mut String, list: &[&(&Report, &Trial)]| {
        for (r, t) in list {
            let at = if t.line > 0 { format!("{}:{}", r.name, t.line) } else { r.name.clone() };
            let call = if t.call.is_empty() { String::new() } else { format!(" `{}`", t.call.replace('`', "'")) };
            let detail = if t.detail.is_empty() { String::new() } else { format!(": {}", t.detail.lines().next().unwrap_or("")) };
            let _ = writeln!(s, "- **{}** {}, `{at}`{call}{detail}", t.edit, t.outcome.label(), );
        }
    };
    if !failures.is_empty() {
        let _ = writeln!(s, "\n### Failures by cause\n");
        let mut causes: BTreeMap<(String, &str, String), Vec<&&(&Report, &Trial)>> = BTreeMap::new();
        for f in &failures {
            causes.entry((f.1.edit.clone(), f.1.outcome.label(), cause(&f.1.detail))).or_default().push(f);
        }
        let mut causes: Vec<_> = causes.into_iter().collect();
        causes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        let _ = writeln!(s, "| Edit | Outcome | Cause | Times | Diagrams | Example |\n|---|---|---|---:|---:|---|");
        for ((edit, outcome, cause), list) in causes {
            let diagrams = list.iter().map(|f| &f.0.name).collect::<std::collections::BTreeSet<_>>().len();
            let (r, t) = list[0];
            let example = format!("`{}:{}` `{}`", r.name, t.line, t.call.replace('`', "'").replace('|', "\\|"));
            let _ = writeln!(s, "| {edit} | {outcome} | {} | {} | {diagrams} | {example} |", cause.replace('|', "\\|"), list.len());
        }
        let _ = writeln!(s, "\n<details><summary>All {} failures</summary>\n", failures.len());
        detailed(&mut s, &failures);
        let _ = writeln!(s, "\n</details>");
    }
    let inexact: Vec<_> = trials.iter().filter(|(_, t)| t.outcome == Outcome::InexactUndo).collect();
    if !inexact.is_empty() {
        let _ = writeln!(s, "\n### Inexact undos\n\nSame drawing, different text after reversing.\n");
        detailed(&mut s, &inexact);
    }
    s
}
