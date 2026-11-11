//! Flat-shaded, vertex-coloured mesh building. Everything procedural in the
//! game (terrain, trees, rocks, ramps, wheels...) goes through `MeshBuilder`
//! so it all shares the same faceted look.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;

use crate::noise::Rng;

#[derive(Default)]
pub struct MeshBuilder {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
}

pub fn rgb(r: f32, g: f32, b: f32) -> Vec3 {
    Vec3::new(r, g, b)
}

fn lin(c: Vec3) -> [f32; 4] {
    Color::srgb(c.x, c.y, c.z).to_linear().to_f32_array()
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counter-clockwise triangle (seen from the outside), colour in sRGB.
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let col = lin(color);
        for p in [a, b, c] {
            self.pos.push(p.to_array());
            self.nor.push(n.to_array());
            self.col.push(col);
        }
    }

    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// Quad whose winding is fixed up so it faces roughly along `toward`.
    pub fn quad_facing(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3, toward: Vec3) {
        if (b - a).cross(c - a).dot(toward) >= 0.0 {
            self.quad(a, b, c, d, color);
        } else {
            self.quad(d, c, b, a, color);
        }
    }

    /// Axis-aligned box.
    pub fn cuboid(&mut self, center: Vec3, half: Vec3, color: Vec3) {
        self.obox(center, Quat::IDENTITY, half, color);
    }

    /// Oriented box.
    pub fn obox(&mut self, center: Vec3, rot: Quat, h: Vec3, color: Vec3) {
        let p = |x: f32, y: f32, z: f32| center + rot * Vec3::new(x * h.x, y * h.y, z * h.z);
        let (a, b, c, d) = (p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.));
        let (e, f, g, i) = (p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.));
        self.quad(a, b, c, d, color); // +z
        self.quad(f, e, i, g, color); // -z
        self.quad(b, f, g, c, color); // +x
        self.quad(e, a, d, i, color); // -x
        self.quad(d, c, g, i, color * 1.06); // +y
        self.quad(e, f, b, a, color * 0.8); // -y
    }

    /// Tapered n-gon prism around an arbitrary axis (`from` -> `to`).
    pub fn frustum(&mut self, from: Vec3, to: Vec3, r0: f32, r1: f32, sides: usize, color: Vec3, caps: bool) {
        let axis = (to - from).normalize_or_zero();
        let side = axis.any_orthonormal_vector();
        let fwd = axis.cross(side);
        let ring = |c: Vec3, r: f32, i: usize| {
            let a = i as f32 / sides as f32 * std::f32::consts::TAU;
            c + (side * a.cos() + fwd * a.sin()) * r
        };
        for i in 0..sides {
            let j = (i + 1) % sides;
            let shade = 0.88 + 0.12 * ((i * 7) % 5) as f32 / 4.0;
            let (a, b, c, d) = (ring(from, r0, i), ring(from, r0, j), ring(to, r1, j), ring(to, r1, i));
            if r1 < 1e-4 {
                self.tri(a, b, to, color * shade);
            } else {
                self.quad(a, b, c, d, color * shade);
            }
            if caps {
                if r1 >= 1e-4 {
                    self.tri(to, d, c, color * 1.05);
                }
                self.tri(from, b, a, color * 0.75);
            }
        }
    }

    /// Lumpy low-poly blob (subdivided octahedron with jitter): rocks, foliage.
    pub fn blob(&mut self, center: Vec3, radii: Vec3, jitter: f32, rng: &mut Rng, color: Vec3, shade_var: f32) {
        let base = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        let faces = [[0, 2, 4], [4, 2, 1], [1, 2, 5], [5, 2, 0], [4, 3, 0], [1, 3, 4], [5, 3, 1], [0, 3, 5]];
        // Jitter keyed on quantised direction so shared verts stay welded.
        let seed = rng.next_u32();
        let disp = |d: Vec3| {
            let q = (d * 8.0).round();
            let h = crate::noise::hash2(q.x as i32 * 31 + q.y as i32, q.z as i32 * 17 - q.y as i32, seed);
            let k = 1.0 + ((h & 0xffff) as f32 / 65535.0 - 0.5) * 2.0 * jitter;
            center + d * radii * k
        };
        for f in faces {
            let (a, b, c) = (base[f[0]], base[f[1]], base[f[2]]);
            let (ab, bc, ca) = ((a + b).normalize(), (b + c).normalize(), (c + a).normalize());
            for t in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
                let s = 1.0 + rng.signed() * shade_var;
                self.tri(disp(t[0]), disp(t[1]), disp(t[2]), color * s);
            }
        }
    }

    pub fn build(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
    }
}
