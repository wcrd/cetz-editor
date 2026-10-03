#import "@preview/cetz:0.5.2": canvas, draw
#set page(width: auto, height: auto, margin: 8pt)

// cetz-editor: grid 0.2
#canvas({
  import draw: *
  anchor("P1", (-16.6, 17))
  anchor("P2", (-6.2, 17))
  anchor("P5", (-3.2, 19.2))
  anchor("P7", (-13, 19.2))
  anchor("P4", (-3.2, 13.4))
  anchor("P8", (-13, 13.4))
  anchor("P3", (-6.2, 11.4))
  anchor("P6", (-16.6, 11.4))
  group(name: "group", {
    line("P5", "P7", "P8", "P4", close: true)
    line("P1", "P6", "P3", "P2", close: true)
    line("P6", "P8", "P4", "P3", close: true)
    line("P6", "P1", "P7", "P8", close: true)
    line("P1", "P7", "P5", "P2", close: true)
    line("P2", "P3", "P4", "P5", close: true)
  })
  content((-12.75, 20), text(size: 22pt)[Zone])
  rect((-0.8, 15.2), (2, 16), name: "rect")
  content("rect.center", [Adjacency])
  line("rect.west", (-4.8, 15.6), mark: (end: ">"))
})
