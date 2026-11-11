#!/usr/bin/env python3
"""Merge animation-only GLBs (same skeleton, matched by node name) into a
rigged GLB so the game can load one file with every clip.

    python3 tools/merge_anims.py rigged.glb out.glb name=clip.glb [name=clip.glb ...]
"""
import json, struct, sys

def load(path):
    d = open(path, "rb").read()
    jl = struct.unpack("<I", d[12:16])[0]
    js = json.loads(d[20:20 + jl])
    off = 20 + jl
    bl = struct.unpack("<I", d[off:off + 4])[0]
    return js, bytearray(d[off + 8:off + 8 + bl])

def save(path, js, binary):
    j = json.dumps(js, separators=(",", ":")).encode()
    j += b" " * (-len(j) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(j) + 8 + len(binary)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(j), b"JSON")); f.write(j)
        f.write(struct.pack("<I4s", len(binary), b"BIN\0")); f.write(binary)

base, out = sys.argv[1], sys.argv[2]
js, binary = load(base)
by_name = {n["name"]: i for i, n in enumerate(js["nodes"])}
js["animations"] = []
for spec in sys.argv[3:]:
    name, path = spec.split("=", 1)
    a, abin = load(path)
    binary += b"\0" * (-len(binary) % 4)
    shift, bv0, acc0 = len(binary), len(js["bufferViews"]), len(js["accessors"])
    binary += abin
    for bv in a["bufferViews"]:
        bv = dict(bv); bv["byteOffset"] = bv.get("byteOffset", 0) + shift; bv["buffer"] = 0
        js["bufferViews"].append(bv)
    for acc in a["accessors"]:
        acc = dict(acc); acc["bufferView"] += bv0
        js["accessors"].append(acc)
    for anim in a["animations"]:
        samplers = [dict(s, input=s["input"] + acc0, output=s["output"] + acc0) for s in anim["samplers"]]
        channels = []
        for ch in anim["channels"]:
            node = by_name[a["nodes"][ch["target"]["node"]]["name"]]
            channels.append({"sampler": ch["sampler"], "target": {"node": node, "path": ch["target"]["path"]}})
        js["animations"].append({"name": name, "samplers": samplers, "channels": channels})
        dur = max(js["accessors"][s["input"]]["max"][0] for s in samplers)
        print(f"  {name}: {len(channels)} channels, {dur:.2f}s")
js["buffers"][0]["byteLength"] = len(binary) + (-len(binary) % 4)
save(out, js, binary)
print("wrote", out, len(binary) // 1024, "KiB")
