#!/bin/bash

# Check if board file is provided
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <board_file.kicad_pcb>"
    exit 1
fi

BOARD="$1"
BOARD_DIR=$(dirname "$BOARD")
BOARD_BASENAME=$(basename "$BOARD" .kicad_pcb)
OUTPUT_DIR="$BOARD_DIR"


# Plot SVGs for mask (F.Cu, B.Cu)
kicad-cli pcb export svg "$BOARD" --output "$OUTPUT_DIR/plot.svg" --layers F.Cu,B.Cu --exclude-drawing-sheet --mode-single
kicad-cli pcb export svg "$BOARD" --output "$OUTPUT_DIR/edge.svg" --layers Edge.Cuts --exclude-drawing-sheet --mode-single --drill-shape-opt 0

# Create mask: single offset
/home/user/git/inkscape/build/install_dir/bin/inkscape --actions="select-all;
    selection-ungroup;
    selection-ungroup;
    selection-ungroup;
    object-to-path;
    object-stroke-to-path;
    path-offset;
    path-union;
    select-list;
    export-filename:$OUTPUT_DIR/mask.svg;
    export-overwrite;
    export-do" "$OUTPUT_DIR/plot.svg"

# Create cuts: double offset
/home/user/git/inkscape/build/install_dir/bin/inkscape --actions="select-all;
    selection-ungroup;
    selection-ungroup;
    selection-ungroup;
    object-to-path;
    object-stroke-to-path;
    path-offset;
    path-offset;
    path-union;
    select-list;
    export-filename:$OUTPUT_DIR/cuts.svg;
    export-overwrite;
    export-do" "$OUTPUT_DIR/plot.svg"

# Open all files in inkscape, because they need some visual checks and balances
inkscape "$OUTPUT_DIR/mask.svg" "$OUTPUT_DIR/cuts.svg" "$OUTPUT_DIR/edge.svg"
