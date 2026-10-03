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

#let __cetz_probe_compass = ("east", "north-east", "north", "north-west", "west", "south-west", "south", "south-east")

#let __cetz_probe(id, elements) = {
  if type(elements) != array { return elements }
  elements.map(el => if type(el) != function { el } else { ctx => {
    let (ctx, ..element) = el(ctx)
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
      for name in names {
        if open-arc and name in __cetz_probe_compass { continue }
        anchors.insert(name, anchor-fn(name))
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
    )
    let probe = (
      type: "content",
      pos: (0.0, 0.0, 0.0),
      width: 0.0,
      height: 0.0,
      segments: (),
      tags: ("no-bounds", "cetz-editor-probe"),
      // The probe sits at canvas coordinate (0, 0), so its position on the
      // page is where the canvas origin landed.
      body: context {
        let p = here().position()
        [#metadata((..info, origin: (page: p.page, x: p.x / 1pt, y: p.y / 1pt)))<__cetz-editor-probe>]
      },
    )
    element.drawables = drawables + (probe,)
    (ctx: ctx, ..element)
  }})
}
