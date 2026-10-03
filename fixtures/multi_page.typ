#import "@preview/cetz:0.5.2": canvas, draw
#set page(width: 10cm, height: 6cm, margin: 1cm)

= Two canvases on two pages

#canvas({
  import draw: *
  rect((0, 0), (3, 2), name: "a", fill: luma(92%))
  circle("a.north-east", radius: 0.4)
})

#pagebreak()

#canvas(length: 0.8cm, {
  import draw: *
  line((0, 0), (4, 1), mark: (end: ">"))
  content((2, -0.5), [On page two], name: "label")
})
