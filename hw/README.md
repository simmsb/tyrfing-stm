# Hardware stuff

This folder contains the kicad schematic and layout for the driver, and
`boost.nbt` which is all the potential divider calculations, written in
[numbat](https://github.com/sharkdp/numbat).

# Changelog

- v0: Initial design: had the mistake of using the Rds(on) of a FET as a sense
  resistor... that's a bad idea, don't do it.
  
- v1: Second revision: uses a real sense resistor to measure the high end
  current, and an analog mux to switch between measuring before the low end
  sense resistor, or the high end sense resistor (such that we don't measure
  Rds(on) + Rsense).
  
  I also changed the LDO from 2.5v to 2.8v
  
- v2: Third revision: Adds more current return path vias, but makes the spring
  pad off center (I have no idea if this will work)
  
# Issues I think I still have

1. As of v1 I am not completely happy with the current return path after the
   sense resistors, currently there are vias spammed everywhere possible but I'm
   not sure there's enough. With v2 I added more vias and moved the spring pad
   off center, but that sounds dumb.

# Thanks

This video series:
https://www.youtube.com/playlist?list=PLYK5tmZIBWtEJjwAFE-49hSeELu9zoAmV was a
great help with getting started designing the current controlled boost circuit.
