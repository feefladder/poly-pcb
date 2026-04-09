# Johnson solid lamps!

![animation of triangular hebesphenorotunda lamp](./anim.webp)

So I thought it'd be superduper cool to make lamps as like johnson solids, so here's some kicad files for that.

There's two versions for each polygon:
1. solder mask only.
2. Solder mask and edge cuts

So you can create directional lamps with soft light in the facing direction using the pcb as diffuser adn stronger light in the backfacing direction.

More will be added once a lamp is actually made

## Exporting for jlpcb

Great thanks to [FunDeckHermit's](https://github.com/FunDeckHermit/NURD-Template) pcb exporter. I've slightly modified it so I can run it from the top-level on any file.

upload zip file to jlpcb. It can be that edge cuts don't work. fix in inkscape.

## Making mask and edge cuts

mask:

1. Plot svgs. Set F.Cu, B.Cu to all layers
2. open F.Cu in inkscape
3. select all, 2x`Ctrl-Shift-G`, `Ctrl-Shift-C`, `Ctrl-Alt-C`, `Ctrl-Shift-+` `Ctrl-Shift-)`
4. There will be some specs, either:
   - have a great time with the node tool
   - `Ctrl-Shift-K` to unmerge, select desired polygons, `!` to invert selection and delete
4. save as...
5. in kicad, `Ctrl-Shift-L`, enable "place at position `0,0`

edge:

1. Plot svgs. Set F.Cu, B.Cu, Edge.Cuts to all layers
2. open F.Cu in inkscape
3. select all, 2x`Ctrl-Shift-G`, `Ctrl-Shift-C`, `Ctrl-Alt-C`, `Ctrl-Shift-+` 2x`Ctrl-Shift-)`
4. There will be specs and some tiny gaps/overlaps.
   - fixs specs as before
   - overlaps can be fixed with running `Ctrl-Shift-+`
   - tiny gaps are equally bad. These require some fun with the node tool
     1. `Shift-doubleclick` at the narrowest point to place nodes
     2. split
     3. overlapping nodes: box select one, shift-click to get the bottom, then select the other, connect with edge, then merge to one
4. save as...
5. in kicad, `Ctrl-Shift-L`, enable "place at position `0,0`
