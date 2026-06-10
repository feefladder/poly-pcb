#!/usr/bin/env python3

from kikit.panelize import Panel
from kikit.units import mm, deg
from pcbnew import VECTOR2I, EDA_ANGLE, DEGREES_T, LoadBoard
from math import tan, radians, sin, cos
from shapely import LineString
from shapely.ops import unary_union
from shapely.linear import shortest_line
from shapely.affinity import rotate

BOARD = "pentagon.kicad_pcb"

panel = Panel("p-panel.kicad_pcb")
board = LoadBoard(BOARD)
bbox = board.GetBoardEdgesBoundingBox()

# Replace these with your packing numbers
WIDTH = bbox.GetWidth()
HEIGHT = bbox.GetHeight()
DX = WIDTH
DY = HEIGHT - WIDTH / 2 * tan(radians(36))

SPACING = 10*mm
VSPACE = SPACING*sin(radians(36))
HSPACE = SPACING*cos(radians(36))

ROWS = 6
COLS = 10

x = 0
y = 0

for row in range(ROWS):
    # pattern like:
    # 0
    # 1
    # 1
    # 0
    # 0
    # 1
    # 1
    # 0
    # 0
    # 1
    # 1
    if ((row-1) // 2) % 2 == 0:
       x = 0
    else:
       x = WIDTH/2 + HSPACE/2
    if (row-1)%2 == 0:
        y+=DY + VSPACE
    else:
        y+=HEIGHT + VSPACE
    for col in range(COLS):
        x += DX + HSPACE

        # Alternate rows
        if row % 2 == 0:
            rot = EDA_ANGLE(180, DEGREES_T)
        else:
            rot = EDA_ANGLE(0, DEGREES_T)

        panel.appendBoard(
            BOARD,
            VECTOR2I(int(x), int(y)),
            rotationAngle=rot
        )

# very ugly python to get partition lines between pentagosn
for row in range(1, ROWS):
    for col in range(COLS):
        if row % 2 == 1:
            xshift = ((row) // 2) % 2
            i_me = row*COLS+col
            i_prev = (row-1)*COLS+col-1+xshift
            i_next = (row-1)*COLS+col+xshift
            s_me = panel.substrates[i_me]
            s_prev = panel.substrates[i_prev]
            s_next = panel.substrates[i_next]

            p_prev = rotate(LineString([s_me.exterior().centroid,s_prev.exterior().centroid]), 90, "centroid")
            p_next = rotate(LineString([s_me.exterior().centroid,s_next.exterior().centroid]), 90, "centroid")
            s_prev.partitionLine = unary_union([s_prev.partitionLine, p_prev])
            s_next.partitionLine = unary_union([s_next.partitionLine, p_next])
            s_me.partitionLine = unary_union([s_me.partitionLine, p_prev, p_next])
        else:
            i_me = row*COLS+col
            i_other = (row-1)*COLS+col
            s_me = panel.substrates[i_me]
            s_other = panel.substrates[i_other]
            p = rotate(LineString([s_me.exterior().centroid,s_other.exterior().centroid]), 90, "centroid")
            s_me.partitionLine = unary_union([s_me.partitionLine, p])
            s_me.tab
            tabs = []
            cuts = []
            tab, cut = s_me.tab(
                origin = tuple(p.centroid.coords[:][0]),
                direction = (0,1),
                width = 5*mm,
                partitionLine = s_me.partitionLine
            )
            tabs.append(tab)
            cuts.append(cut)
            tab, cut = s_other.tab(
                origin = tuple(p.centroid.coords[:][0]),
                direction = (0,-1),
                width = 5*mm,
                partitionLine = s_other.partitionLine
            )
            tabs.append(tab)
            cuts.append(cut)

            print(panel.boardSubstrate.substrates)
            panel.forwardTabs.extend(tabs)
            panel.boardSubstrate.union(tabs)
            print(panel.boardSubstrate.substrates)
            # print(tab, cut, tab.area, tab.bounds, s_me.exterior().bounds)
            print(s_me.substrates)
            s_me.union(tab)
            print(s_me.substrates)
            # s_me.partitionLine = unary_union([s_me.partitionLine, tab.boundary, s_me.exterior().boundary])

# panel.buildPartitionLineFromBB(
#     boundarySubstrates=[],
#     safeMargin=0
# )
# panel.buildTabAnnotationsFixed(
#     hcount=2,
#     vcount=2,
#     hwidth=5 * mm,
#     vwidth=5 * mm,
#     minDistance=0,
#     ghostSubstrates=[]
# )
# p02 = rotate(shortest_line(panel.substrates[0].exterior().centroid, panel.substrates[2].exterior().centroid), 90, "centroid")
# panel.substrates[0].partitionLine = unary_union([panel.substrates[0].partitionLine, p02])
# panel.substrates[2].partitionLine = unary_union([panel.substrates[2].partitionLine, p02])
panel.buildTabsFromAnnotations(0)

panel.debugRenderPartitionLines()
panel.save("p-panels.kicad_pcb")
