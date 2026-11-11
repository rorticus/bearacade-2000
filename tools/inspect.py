# Blender: print bounding box + render a quick preview of a GLB.
#   blender -b -P tools/inspect.py -- in.glb out.png
import bpy, sys, mathutils, math
src, out = sys.argv[sys.argv.index("--") + 1:][:2]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
lo = mathutils.Vector((1e9,) * 3); hi = mathutils.Vector((-1e9,) * 3)
for o in bpy.context.scene.objects:
    print("OBJ", o.type, o.name, "parent", o.parent.name if o.parent else None, "scale", tuple(o.scale))
    if o.type == "MESH":
        print("  tris", sum(len(p.vertices) - 2 for p in o.data.polygons), "verts", len(o.data.vertices))
        for c in o.bound_box:
            w = o.matrix_world @ mathutils.Vector(c)
            for i in range(3):
                lo[i] = min(lo[i], w[i]); hi[i] = max(hi[i], w[i])
    if o.type == "ARMATURE":
        print("  bones", [b.name for b in o.data.bones][:60])
print("BBOX(blender z-up) lo", tuple(round(v, 3) for v in lo), "hi", tuple(round(v, 3) for v in hi))
print("ACTIONS", [(a.name, tuple(a.frame_range)) for a in bpy.data.actions])
c = (lo + hi) / 2; size = max(hi - lo)
cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); bpy.context.scene.collection.objects.link(cam)
cam.location = c + mathutils.Vector((size * 1.3, -size * 1.6, size * 0.9))
cam.rotation_euler = (c - cam.location).to_track_quat("-Z", "Y").to_euler()
sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN")); sun.data.energy = 4
sun.rotation_euler = (0.8, 0.2, 0.6); bpy.context.scene.collection.objects.link(sun)
s = bpy.context.scene; s.camera = cam; s.render.engine = "BLENDER_EEVEE"
s.render.resolution_x = s.render.resolution_y = 600; s.render.filepath = out
w = bpy.data.worlds.new("w"); s.world = w; w.use_nodes = True
w.node_tree.nodes["Background"].inputs[0].default_value = (0.8, 0.85, 1, 1)
bpy.ops.render.render(write_still=True)
