// Injected into an instrumented copy of the source (see cetz-scene's
// `instrument`). Wraps the CeTZ elements produced by one draw call and records
// the geometry CeTZ computed for them as invisible metadata, keyed by `id`
// (the call's byte offset in the original source).
//
// The probe drawable is tagged `no-bounds` so it never changes the canvas
// size, and has zero size so it never changes the rendering. Coupled to CeTZ
// 0.5's element/drawable structures.

// A fill as a CSS colour: gradients by their middle colour, tilings dropped.
#let __cetz_probe_fill(fill) = if type(fill) == color {
  rgb(fill).to-hex()
} else if type(fill) == gradient {
  rgb(fill.sample(50%)).to-hex()
} else { none }

// Whether the element's outline is an open path. CeTZ 0.5.2 lists a
// `centroid` anchor for every line, open or not (it checks `close != none`,
// and `close` is `false`), and works it out by dividing by the area the
// points enclose: zero for points in a row or a Z like an elbow connector.
// It only means anything for a closed line, so the probe skips it otherwise.
#let __cetz_probe_open(drawables) = {
  let path = drawables.find(d => d.type == "path")
  path != none and path.segments.len() > 0 and not path.segments.first().at(1)
}

#let __cetz_probe_compass = ("east", "north-east", "north", "north-west", "west", "south-west", "south", "south-east")

// How many repetitions of a call in a loop the probe records. Every probe
// costs memory, and a fractal can draw tens of thousands of shapes from one
// call: past this, the browser runs out.
#let __cetz_probe_limit = 500

// `looped`: the call is in a loop. Its unnamed shapes record only the
// anchors the editor uses for them, since asking CeTZ for every anchor of
// thousands of shapes takes seconds and gigabytes, and only the first
// `__cetz_probe_limit` are recorded at all (counted in the context, which
// CeTZ passes from shape to shape).
#let __cetz_probe(id, elements, looped: false) = {
  if type(elements) != array { return elements }
  elements.map(el => if type(el) != function { el } else { ctx => {
    let counts = ctx.at("cetz-editor-probes", default: (:))
    let count = counts.at(str(id), default: 0)
    if looped and count >= __cetz_probe_limit { return el(ctx) }
    let (ctx, ..element) = el(ctx)
    if looped { ctx.insert("cetz-editor-probes", counts + ((str(id)): count + 1)) }
    let drawables = element.at("drawables", default: ())
    if type(drawables) == dictionary { drawables = (drawables,) }

    let anchors = (:)
    let anchor-fn = element.at("anchors", default: none)
    if type(anchor-fn) == function {
      let names = anchor-fn(())
      // A compass anchor is where a ray from the centre meets the shape's
      // own path, and CeTZ panics when it misses. Only an open arc can miss
      // (one from 0° to 60° has no "north-west"); groups and the other
      // shapes measure a closed path or their bounds.
      let open-arc = "arc-start" in names and "chord-center" in names and drawables.any(d => d.type == "path" and d.segments.any(s => not s.at(1)))
      // A polygon's or star's corners and edge midpoints come from its
      // outline's vertices: CeTZ 0.5.2's n-star works them out from a
      // variable it never defines, so asking it panics.
      let corners = names.filter(n => n.starts-with("corner-")).len()
      let outline = drawables.find(d => d.type == "path")
      let ring = if corners > 0 and "edge-0" in names and outline != none {
        let (origin, _, segments) = outline.segments.first()
        if segments.all(s => s.first() == "l") {
          let ring = (origin,) + segments.map(s => s.at(1))
          // A closed outline ends with a segment back to its start.
          if ring.len() > 1 and ring.last() == origin { ring.slice(0, -1) } else { ring }
        }
      }
      let open-path = "centroid" in names and __cetz_probe_open(drawables)
      let outline-points = if outline != none and outline.segments.len() > 0 {
        let (origin, _, segments) = outline.segments.first()
        (origin,) + segments.map(s => s.slice(1)).join(default: ())
      } else { () }
      // A closed line's or path's centroid (also its default anchor) only
      // exists when its own points share one z, and CeTZ panics otherwise;
      // it also divides by the outline's area. Ask only when the canvas
      // points still tell us both are fine: under a 3D transform (`ortho`)
      // they might not.
      let no-centroid = "centroid" in names and {
        let zrow = ctx.transform.at(2)
        let z = outline-points.at(0, default: ()).at(2, default: 0)
        let planar = zrow.at(0) == 0 and zrow.at(1) == 0 and zrow.at(2) != 0 and outline-points.all(p => p.at(2, default: 0) == z)
        let n = outline-points.len()
        let area = range(n).map(i => {
          let (a, b) = (outline-points.at(i), outline-points.at(calc.rem(i + 1, n)))
          a.at(0) * b.at(1) - b.at(0) * a.at(1)
        }).sum(default: 0)
        not planar or calc.abs(area) < 1e-9
      }
      // A path's start, mid and end are points along it, and CeTZ fails an
      // assertion when it has no length (a line from a point to itself).
      let no-length = outline-points.all(p => p == outline-points.first())
      let few = looped and element.at("name", default: none) == none
      for name in names {
        if few and name not in ("default", "center") { continue }
        if open-arc and name in __cetz_probe_compass { continue }
        if open-path and name == "centroid" { continue }
        if no-centroid and name in ("centroid", "default") { continue }
        if no-length and name in ("start", "mid", "end") { continue }
        let corner = name.starts-with("corner-") and ring != none and ring.len() == corners
        let edge = name.starts-with("edge-") and ring != none and ring.len() == corners
        anchors.insert(name, if corner {
          ring.at(int(name.slice(7)))
        } else if edge {
          let i = int(name.slice(5))
          let (a, b) = (ring.at(i), ring.at(calc.rem(i + 1, ring.len())))
          a.zip(b).map(((p, q)) => (p + q) / 2)
        } else {
          anchor-fn(name)
        })
      }
    }

    let geometry = drawables
      .filter(d => {
        let tags = d.at("tags", default: ())
        "hidden" not in tags and "cetz-editor-probe" not in tags
      })
      .map(d => if d.type == "path" {
        (type: "path", segments: d.segments, fill: __cetz_probe_fill(d.at("fill", default: none)))
      } else {
        (type: d.type, pos: d.pos, width: d.width, height: d.height)
      })

    let info = (
      id: id,
      name: element.at("name", default: none),
      drawables: geometry,
      anchors: anchors,
      // Maps the call's own coordinates to canvas coordinates (4x4, rows).
      transform: ctx.transform,
      length: ctx.length / 1pt,
      // The last repetition recorded: later ones may have been drawn too.
      truncated: looped and count + 1 == __cetz_probe_limit,
    )
    let probe = (
      type: "content",
      pos: (0.0, 0.0, 0.0),
      width: 0.0,
      height: 0.0,
      // One point, like the border CeTZ gives its own content: `ortho` sorts
      // drawables by their segments' depth and fails on none. Tagged
      // `debug`, like CeTZ's own bounding boxes, so paths that merge their
      // children's segments (`merge-path`) leave it out.
      segments: (((0.0, 0.0, 0.0), false, ()),),
      tags: ("no-bounds", "debug", "cetz-editor-probe"),
      // The probe sits at canvas coordinate (0, 0), so where it lands on
      // the page is where the canvas origin did: the compiler reads that
      // from its position (asking with `context` here costs a lot of
      // memory in diagrams with thousands of shapes).
      body: [#metadata(info)<__cetz-editor-probe>],
    )
    element.drawables = drawables + (probe,)
    (ctx: ctx, ..element)
  }})
}
