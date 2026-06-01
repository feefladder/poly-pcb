# Panelization

Based on [this hackaday post](https://hackaday.com/2020/04/25/kicad-panelization-made-easy/), the panelization with mouse-bites that I wanted (especially for triangles) was kikit. On debian, they don't like system-installing pip and there was no default repo, but [this here ppa](https://github.com/set-soft/debian) made life easy.

Sometimes it segfaults. Best is:

1. open project in kicad project manager
2. in same folder, terminal e.g. `pcbnew tr-panel`
3. in that file, open panelizer, set output to e.g. `tr-panels.kicad_pcb`
4. when panelization done, don't close, but open `tr-panels.kicad_pcb` in kicad to view output

Somehow, the inkscape-made holes make everything super slow, and since not-holed pcbs are the same (and even whole/half pcbs are the same), export panelization settings and load from json.

Now let's see what jlpcb says if a 100-piece panel is a single pcb lol
