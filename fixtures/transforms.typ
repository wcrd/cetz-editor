#import "@preview/cetz:0.5.2": canvas, draw
#set page(width: auto, height: auto, margin: 4pt)

#canvas({
  import draw: *
  set-style(stroke: 0.8pt)
  group(name: "g", {
    rotate(30deg)
    rect((0, 0), (2, 1), name: "r")
    circle((3, 0), radius: 0.5)
  })
  scale(2)
  circle((0, -2), radius: 0.3, name: "c")
  line("g.r.north-east", "c.east", mark: (end: ">"))
  bezier((0, -3), (3, -3), (1, -2), (2, -4))
  content((4, -3), $x^2 + y^2$, name: "math")
})
