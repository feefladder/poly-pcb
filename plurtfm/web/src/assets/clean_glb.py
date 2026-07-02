#!/usr/bin/env -S flatpak run org.blender.Blender --background --python
# run with
# blender --python clean_glb.py
# or if blender is flatpak:
# flatpak run org.blender.Blender --background --python "$PWD/clean_glb.py"


import os

import bpy

MERGE_DISTANCE = 0.0001

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))

for filename in os.listdir(SCRIPT_DIR):
    if not filename.lower().endswith(".glb"):
        continue

    filepath = os.path.join(SCRIPT_DIR, filename)
    print(f"Processing {filename}")

    # Clear scene
    bpy.ops.wm.read_factory_settings(use_empty=True)

    # Import GLB
    bpy.ops.import_scene.gltf(filepath=filepath)

    # Merge by Distance on every mesh
    for obj in bpy.data.objects:
        if obj.type != "MESH":
            continue

        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj

        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.remove_doubles(
            threshold=MERGE_DISTANCE,
            use_sharp_edge_from_normals=True,
        )
        bpy.ops.object.mode_set(mode="OBJECT")

    # Overwrite original file
    bpy.ops.export_scene.gltf(
        filepath=filepath,
        export_format="GLB",
    )

print("Done.")
