#import "@preview/cetz:0.4.2": canvas, draw
#set page(width: auto, height: auto, margin: 2pt)
#canvas({
  import draw: *
  let pts = (A: (0,0), B: (5,0), C: (5,3), D: (0,3), E: (1.4,1), F: (6.4,1), G: (6.4,4), H: (1.4,4))
  for (k, p) in pts { anchor(k, p) }
  let face(..a) = line(..a.pos(), close: true, ..a.named())
  face("E","F","G","H", fill: luma(85%))
  face("D","C","G","H", fill: luma(90%), stroke: black)
  face("A","B","F","E", fill: luma(90%), stroke: black)
  face("A","B","C","D", stroke: 1pt)
  face("E","F","G","H", stroke: 1pt)
  for (a, b) in (("A","E"), ("B","F"), ("C","G"), ("D","H")) { line(a, b, stroke: 1pt) }
  content((1.84,4.38), strong[Zone])
  content((3.11,0.51), text(5pt)[Floor Surface])
  content((3.23,3.54), text(5pt)[Ceiling Surface])
  content((6.99,3.25), text(5pt)[Wall Surface])
  content((8.4,1.76), text(5pt)[type: .Pinned])
  content((11.88,2.26), [Equipment], name: "equip", frame: "rect", padding: .3, fill: orange.lighten(85%))
  face("B","F","G","C", fill: rgb("#f4c6cf"), stroke: black)
  content((8.4,2.25), text(6pt)[Adjacency], name: "equip2", frame: "rect", padding: .1, fill: orange.lighten(85%))
  line((5,1), (6.4,1), stroke: 1pt)
  line("equip2.west", (5.7,2.25), stroke: 1pt, mark: (end: ">", fill: black))
  line("equip2.east", "equip.west", stroke: 1pt, mark: (end: ">", fill: black))
})
