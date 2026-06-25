import bpy
import sqlite3
import os

# very ugly python to make johnson solid

# --- CONFIG ---
# set to path in plurtfm
db_path = "/home/user/blender/GEOM/polydb.sqlite3"
target_longname = "hebesphenomegacorona"

# --- CONNECT TO DB ---
conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
cur = conn.cursor()

# --- GET POLYHEDRON ID ---
cur.execute("""
    SELECT id FROM Polyhedron WHERE longname = ?
""", (target_longname,))
row = cur.fetchone()

if not row:
    raise ValueError(f"Polyhedron '{target_longname}' not found")

poly_id = row[0]

# --- LOAD VERTICES ---
cur.execute("""
    SELECT id, x, y, z FROM Vertex
    WHERE poly = ?
    ORDER BY id
""", (poly_id,))

vertices_raw = cur.fetchall()

# Map DB vertex id -> Blender index
vertex_map = {}
vertices = []

for i, (vid, x, y, z) in enumerate(vertices_raw):
    vertex_map[vid] = i
    vertices.append((x, y, z))

# --- LOAD FACES ---
cur.execute("""
    SELECT face, vertex, idx FROM Polygon
    WHERE poly = ?
    ORDER BY face, idx
""", (poly_id,))

faces_dict = {}

for face_id, vertex_id, idx in cur.fetchall():
    faces_dict.setdefault(face_id, []).append((idx, vertex_id))

# Sort vertices inside each face by idx
faces = []
for face_id in sorted(faces_dict.keys()):
    ordered = sorted(faces_dict[face_id], key=lambda x: x[0])
    face = [vertex_map[v_id] for _, v_id in ordered]
    faces.append(face)

conn.close()

# --- CREATE MESH ---
mesh = bpy.data.meshes.new(target_longname)
mesh.from_pydata(vertices, [], faces)
mesh.update()

obj = bpy.data.objects.new(target_longname, mesh)
bpy.context.collection.objects.link(obj)

bpy.context.view_layer.objects.active = obj
bpy.ops.object.mode_set(mode='EDIT')
bpy.ops.mesh.normals_make_consistent(inside=False)  # outside-facing normals
bpy.ops.object.mode_set(mode='OBJECT')

print(f"Created polyhedron: {target_longname}")
