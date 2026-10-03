#import "@preview/cetz:0.5.2": canvas, draw, decorations, angle
#set page(width: auto, height: auto, margin: 4pt)

// Shapes the editor's tools and handles write, and the probe has to survive.
#canvas({
  import draw: *
  // Arcs short of a quarter turn have no border at some compass directions.
  arc((0, 0), start: 0deg, stop: 60deg, radius: 1, anchor: "origin")
  arc((2.5, 0), start: 30deg, stop: -60deg, radius: 1, anchor: "origin", mark: (end: ">"))
  arc((5, 0), start: 0deg, delta: 45deg, radius: 1, mode: "PIE", fill: luma(90%))
  polygon((8, 0), 6, radius: 1, angle: 30deg)
  n-star((10.5, 0), 5, radius: 1, inner-radius: 40%)
  circle((13, 0), radius: (1.4, 0.8))

  // The rotation and scale handles' wrapper scopes, and what ungroup splits into.
  scope({
    rotate(30deg, origin: (1, -3))
    scale(1.5, origin: (1, -3))
    rect((0, -3.5), (2, -2.5), name: "turned")
  })
  scope({
    rotate(-30deg, origin: (5, -3))
    circle((4.5, -3), radius: 0.4)
  })
  scope({
    rotate(-30deg, origin: (5, -3))
    rect((5.2, -3.4), (6, -2.6), name: "split")
  })
  line("turned.north-east", "split.west", mark: (end: ">"))
  rect-around("turned", "split", padding: 0.2, stroke: (dash: "dashed"))

  // Curve, braces, angle marks and path decorations.
  catmull((8, -4), (9, -2.5), (10.5, -4), (12, -2.5))
  decorations.brace((0, -6), (4, -6))
  decorations.flat-brace((5, -6), (9, -6), flip: true)
  line((10, -7), (12, -7), name: "a")
  line((10, -7), (11.5, -5.5), name: "b")
  angle.angle("a.start", "a.end", "b.end", label: $alpha$)
  angle.right-angle((13, -7), (14, -7), (13, -6))
  decorations.zigzag(line((0, -8.5), (4, -8.5)), amplitude: 0.3)
  decorations.coil(line((5, -8.5), (9, -8.5)), amplitude: 0.4)
  compound-path({
    rect((10, -9), (12, -8))
    circle((11, -8.5), radius: 0.3)
  }, fill: luma(80%), fill-rule: "even-odd")
})
