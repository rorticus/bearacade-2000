//! Endless procedurally generated polygonal terrain, streamed in chunks
//! around the truck. `World` is also the single source of truth for physics
//! ground queries, so what you see is exactly what you drive on.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::meshgen::{rgb, MeshBuilder};
use crate::noise::{fbm, gnoise, hash2, smoothstep};
use crate::truck::Truck;

pub const CELL: f32 = 4.0;
pub const CHUNK_CELLS: i32 = 16;
pub const CHUNK_SIZE: f32 = CELL * CHUNK_CELLS as f32;
pub const WATER_Y: f32 = -7.0;
pub const TERRAIN_RADIUS: i32 = 8;
const VERTS: usize = (CHUNK_CELLS + 1) as usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    Meadow,
    Forest,
    Desert,
}

/// A drivable wooden kicker. Part of the ground as far as physics cares.
#[derive(Clone)]
pub struct Ramp {
    pub chunk: IVec2,
    pub pos: Vec2,
    pub dir: Vec2,
    pub len: f32,
    pub width: f32,
    pub height: f32,
    pub base_y: f32,
}

impl Ramp {
    fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let d = Vec2::new(x, z) - self.pos;
        let u = d.dot(self.dir);
        let v = d.dot(self.dir.perp());
        if u < 0.0 || u > self.len || v.abs() > self.width * 0.5 {
            return None;
        }
        Some(self.base_y + self.height * (u / self.len))
    }
}

#[derive(Resource)]
pub struct World {
    pub seed: u32,
    pub ramps: Vec<Ramp>,
    heights: HashMap<IVec2, Box<[f32]>>,
}

impl World {
    pub fn new(seed: u32) -> Self {
        Self { seed, ramps: Vec::new(), heights: HashMap::default() }
    }

    /// The analytic height field (before triangulation).
    pub fn raw_height(&self, x: f32, z: f32) -> f32 {
        let s = self.seed;
        let cont = fbm(x / 1100.0, z / 1100.0, 3, s);
        let hills = fbm(x / 300.0, z / 300.0, 4, s.wrapping_add(11)) * 30.0;
        let mount_mask = smoothstep(0.12, 0.5, cont);
        let ridge = 1.0 - gnoise(x / 380.0, z / 380.0, s.wrapping_add(23)).abs();
        let mountains = ridge * ridge * ridge * 75.0 * mount_mask;
        let detail = fbm(x / 60.0, z / 60.0, 3, s.wrapping_add(37)) * 4.5;
        // Knolls: rounded humps you can launch off at speed.
        let k = gnoise(x / 85.0, z / 85.0, s.wrapping_add(47));
        let knolls = (k.max(0.0)).powi(2) * 22.0;
        let chatter = gnoise(x / 13.0, z / 13.0, s.wrapping_add(41)) * 0.45;
        // Rolling dunes / whoops in dry areas: nature's jump ramps.
        let dry = smoothstep(-0.05, -0.3, self.moisture(x, z));
        let whoops = (gnoise(x / 34.0, z / 34.0, s.wrapping_add(53)).abs() * 2.0 - 0.6).max(0.0) * 5.0 * dry;
        let h = cont * 24.0 + hills + mountains + detail + knolls + chatter + whoops + 5.0;
        // Gentle plateau to start on.
        let t = smoothstep(30.0, 150.0, (x * x + z * z).sqrt());
        (4.0 + detail * 0.2 + chatter) * (1.0 - t) + h * t
    }

    fn moisture(&self, x: f32, z: f32) -> f32 {
        fbm(x / 700.0 + 31.7, z / 700.0 - 12.3, 2, self.seed.wrapping_add(71))
    }

    pub fn biome(&self, x: f32, z: f32) -> Biome {
        let m = self.moisture(x, z) + gnoise(x / 90.0, z / 90.0, self.seed.wrapping_add(83)) * 0.05;
        if m < -0.16 {
            Biome::Desert
        } else if m > 0.12 {
            Biome::Forest
        } else {
            Biome::Meadow
        }
    }

    fn vertex_height(&self, ix: i32, iz: i32) -> f32 {
        let c = IVec2::new(ix.div_euclid(CHUNK_CELLS), iz.div_euclid(CHUNK_CELLS));
        if let Some(h) = self.heights.get(&c) {
            let (lx, lz) = (ix.rem_euclid(CHUNK_CELLS) as usize, iz.rem_euclid(CHUNK_CELLS) as usize);
            return h[lz * VERTS + lx];
        }
        self.raw_height(ix as f32 * CELL, iz as f32 * CELL)
    }

    /// Height of the triangulated terrain mesh at (x, z).
    pub fn terrain_height(&self, x: f32, z: f32) -> f32 {
        let (gx, gz) = (x / CELL, z / CELL);
        let (ix, iz) = (gx.floor() as i32, gz.floor() as i32);
        let (fx, fz) = (gx - ix as f32, gz - iz as f32);
        let h00 = self.vertex_height(ix, iz);
        let h11 = self.vertex_height(ix + 1, iz + 1);
        if fx > fz {
            let h10 = self.vertex_height(ix + 1, iz);
            h00 + (h10 - h00) * fx + (h11 - h10) * fz
        } else {
            let h01 = self.vertex_height(ix, iz + 1);
            h00 + (h11 - h01) * fx + (h01 - h00) * fz
        }
    }

    /// Drivable ground height: terrain plus ramps.
    pub fn ground(&self, x: f32, z: f32) -> f32 {
        let mut h = self.terrain_height(x, z);
        for r in &self.ramps {
            if (r.pos.x - x).abs() < 16.0 && (r.pos.y - z).abs() < 16.0 {
                if let Some(rh) = r.height_at(x, z) {
                    h = h.max(rh);
                }
            }
        }
        h
    }

    pub fn ground_normal(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.35;
        let dx = self.ground(x + e, z) - self.ground(x - e, z);
        let dz = self.ground(x, z + e) - self.ground(x, z - e);
        // Clamp so ramp lips / steps do not produce horizontal normals.
        Vec3::new(-dx.clamp(-1.4, 1.4), 2.0 * e, -dz.clamp(-1.4, 1.4)).normalize()
    }

    pub fn slope(&self, x: f32, z: f32) -> f32 {
        let e = CELL;
        let dx = self.raw_height(x + e, z) - self.raw_height(x - e, z);
        let dz = self.raw_height(x, z + e) - self.raw_height(x, z - e);
        (dx * dx + dz * dz).sqrt() / (2.0 * e)
    }
}

pub fn chunk_of(p: Vec3) -> IVec2 {
    IVec2::new((p.x / CHUNK_SIZE).floor() as i32, (p.z / CHUNK_SIZE).floor() as i32)
}

#[derive(Component)]
pub struct TerrainChunk(pub IVec2);

#[derive(Component)]
struct Water;

#[derive(Resource)]
struct TerrainAssets {
    material: Handle<StandardMaterial>,
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, (stream_chunks, follow_water));
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(TerrainAssets {
        material: materials.add(StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.15, ..default() }),
    });
    commands.spawn((
        Water,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(3000.0, 3000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.16, 0.5, 0.72, 0.78),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.25,
            reflectance: 0.6,
            ..default()
        })),
        Transform::from_xyz(0.0, WATER_Y, 0.0),
    ));
}

fn follow_water(truck: Query<&Transform, With<Truck>>, mut water: Query<&mut Transform, (With<Water>, Without<Truck>)>) {
    let (Ok(t), Ok(mut w)) = (truck.single(), water.single_mut()) else { return };
    w.translation.x = (t.translation.x / 64.0).round() * 64.0;
    w.translation.z = (t.translation.z / 64.0).round() * 64.0;
}

fn surface_color(world: &World, x: f32, z: f32, y: f32, normal_y: f32, jitter: f32) -> Vec3 {
    let biome = world.biome(x, z);
    let mut c = match biome {
        Biome::Meadow => rgb(0.44, 0.68, 0.28),
        Biome::Forest => rgb(0.27, 0.5, 0.24),
        Biome::Desert => rgb(0.88, 0.68, 0.4),
    };
    // Patchy two-tone ground.
    let patch = gnoise(x / 45.0, z / 45.0, world.seed.wrapping_add(97));
    c *= 1.0 + patch * 0.12;
    if normal_y < 0.8 {
        let rock = if biome == Biome::Desert { rgb(0.7, 0.45, 0.3) } else { rgb(0.5, 0.48, 0.47) };
        c = c.lerp(rock, smoothstep(0.8, 0.68, normal_y));
    }
    let snow_line = 58.0 + patch * 6.0;
    if y > snow_line {
        c = c.lerp(rgb(0.95, 0.97, 1.0), smoothstep(snow_line, snow_line + 7.0, y));
    }
    if y < WATER_Y + 1.6 {
        c = c.lerp(rgb(0.9, 0.82, 0.58), smoothstep(WATER_Y + 1.6, WATER_Y + 0.6, y));
    }
    if y < WATER_Y - 0.5 {
        c = c.lerp(rgb(0.45, 0.55, 0.5), smoothstep(WATER_Y - 0.5, WATER_Y - 4.0, y));
    }
    c * (1.0 + jitter)
}

fn build_chunk(world: &World, c: IVec2, heights: &[f32]) -> Mesh {
    let mut mb = MeshBuilder::new();
    let h = |x: usize, z: usize| heights[z * VERTS + x];
    let (ox, oz) = (c.x * CHUNK_CELLS, c.y * CHUNK_CELLS);
    for z in 0..CHUNK_CELLS as usize {
        for x in 0..CHUNK_CELLS as usize {
            let (x0, z0) = (x as f32 * CELL, z as f32 * CELL);
            let p00 = Vec3::new(x0, h(x, z), z0);
            let p10 = Vec3::new(x0 + CELL, h(x + 1, z), z0);
            let p01 = Vec3::new(x0, h(x, z + 1), z0 + CELL);
            let p11 = Vec3::new(x0 + CELL, h(x + 1, z + 1), z0 + CELL);
            for (i, t) in [[p00, p11, p10], [p00, p01, p11]].into_iter().enumerate() {
                let cen = (t[0] + t[1] + t[2]) / 3.0;
                let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
                let hsh = hash2(ox + x as i32, oz + z as i32, world.seed.wrapping_add(i as u32 * 7 + 5));
                let jitter = ((hsh & 0xff) as f32 / 255.0 - 0.5) * 0.09;
                let (wx, wz) = (cen.x + c.x as f32 * CHUNK_SIZE, cen.z + c.y as f32 * CHUNK_SIZE);
                mb.tri(t[0], t[1], t[2], surface_color(world, wx, wz, cen.y, n.y, jitter));
            }
        }
    }
    mb.build()
}

fn stream_chunks(
    mut commands: Commands,
    mut world: ResMut<World>,
    assets: Res<TerrainAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    truck: Query<&Truck>,
    chunks: Query<(Entity, &TerrainChunk)>,
    mut first: Local<bool>,
) {
    let Ok(truck) = truck.single() else { return };
    let center = chunk_of(truck.pos);

    for (e, ch) in &chunks {
        let d = ch.0 - center;
        if d.x.abs().max(d.y.abs()) > TERRAIN_RADIUS + 1 {
            commands.entity(e).despawn();
            world.heights.remove(&ch.0);
        }
    }

    // Nearest-first, a few per frame (everything on the very first frame).
    let mut missing: Vec<IVec2> = Vec::new();
    for dz in -TERRAIN_RADIUS..=TERRAIN_RADIUS {
        for dx in -TERRAIN_RADIUS..=TERRAIN_RADIUS {
            let c = center + IVec2::new(dx, dz);
            if !world.heights.contains_key(&c) {
                missing.push(c);
            }
        }
    }
    missing.sort_by_key(|c| (*c - center).length_squared());
    let budget = if *first { 6 } else { usize::MAX };
    *first = true;

    for c in missing.into_iter().take(budget) {
        let mut hs = vec![0.0f32; VERTS * VERTS];
        for z in 0..VERTS {
            for x in 0..VERTS {
                let (wx, wz) = ((c.x * CHUNK_CELLS + x as i32) as f32 * CELL, (c.y * CHUNK_CELLS + z as i32) as f32 * CELL);
                hs[z * VERTS + x] = world.raw_height(wx, wz);
            }
        }
        let mesh = build_chunk(&world, c, &hs);
        world.heights.insert(c, hs.into_boxed_slice());
        commands.spawn((
            TerrainChunk(c),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_xyz(c.x as f32 * CHUNK_SIZE, 0.0, c.y as f32 * CHUNK_SIZE),
        ));
    }
}
