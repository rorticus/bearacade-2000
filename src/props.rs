//! Everything scattered over the map: trees, rocks, barrels, fuel cans and
//! jump ramps, plus the tiny sphere-body physics used by anything that can be
//! punted (barrels, snapped trees, airborne bears).

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::game::{Fx, GameState, Sim};
use crate::meshgen::{rgb, MeshBuilder};
use crate::noise::Rng;
use crate::terrain::{chunk_of, Biome, Ramp, World, CHUNK_SIZE, WATER_Y};
use crate::truck::{Truck, GRAVITY};

pub const OBJECT_RADIUS: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObKind {
    Tree,
    Rock,
}

#[derive(Clone)]
pub struct Obstacle {
    /// Trees: base of the trunk. Rocks: sphere centre.
    pub pos: Vec3,
    pub radius: f32,
    pub height: f32,
    pub kind: ObKind,
    pub breakable: bool,
    pub entity: Entity,
}

#[derive(Resource, Default)]
pub struct Obstacles(pub HashMap<IVec2, Vec<Obstacle>>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Barrel,
    Debris,
    Bear,
}

/// A bouncy sphere. `pos` is the sphere centre; the visual origin sits
/// `offset` below it along the body's own up axis.
#[derive(Component)]
pub struct Body {
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    pub angvel: Vec3,
    pub prev_pos: Vec3,
    pub prev_rot: Quat,
    pub radius: f32,
    pub mass: f32,
    pub bounce: f32,
    pub offset: f32,
    pub kind: BodyKind,
    pub asleep: bool,
    pub rest_time: f32,
    pub hit_cd: f32,
    pub grounded: bool,
    pub age: f32,
    /// Where the truck last touched it: toss distances are measured from here.
    pub origin: Vec3,
}

impl Body {
    pub fn new(kind: BodyKind, pos: Vec3, rot: Quat, radius: f32, mass: f32, offset: f32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            rot,
            angvel: Vec3::ZERO,
            prev_pos: pos,
            prev_rot: rot,
            radius,
            mass,
            bounce: 0.45,
            offset,
            kind,
            asleep: false,
            rest_time: 0.0,
            hit_cd: 0.0,
            grounded: false,
            age: 0.0,
            origin: pos,
        }
    }
}

/// Belongs to a streamed chunk; despawned when the chunk unloads.
#[derive(Component)]
pub struct InChunk(pub IVec2);

#[derive(Component)]
pub struct FuelCan {
    base_y: f32,
    phase: f32,
}

#[derive(Component)]
pub struct Lifetime(pub f32);

/// Sent after a chunk's props exist, so bears can move in.
#[derive(Event)]
pub struct ChunkPopulated(pub IVec2);

#[derive(Resource)]
pub struct PropAssets {
    pub material: Handle<StandardMaterial>,
    beacon_material: Handle<StandardMaterial>,
    beacon: Handle<Mesh>,
    pines: Vec<Handle<Mesh>>,
    rounds: Vec<Handle<Mesh>>,
    cacti: Vec<Handle<Mesh>>,
    rocks: Vec<Handle<Mesh>>,
    barrels: Vec<Handle<Mesh>>,
    fuel: Handle<Mesh>,
}

#[derive(Resource, Default)]
struct Loaded(HashSet<IVec2>);

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Obstacles>()
            .init_resource::<Loaded>()
            .add_event::<ChunkPopulated>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, (truck_vs_bodies, body_physics).chain().in_set(Sim::Bodies))
            .add_systems(Update, (stream_objects, sync_bodies, fuel_cans, smash_trees, lifetimes));
    }
}

fn pine(rng: &mut Rng) -> Mesh {
    let mut mb = MeshBuilder::new();
    let green = rgb(0.13 + rng.f() * 0.06, 0.38 + rng.f() * 0.1, 0.2 + rng.f() * 0.05);
    mb.frustum(Vec3::ZERO, Vec3::Y * 1.6, 0.28, 0.2, 6, rgb(0.36, 0.24, 0.14), false);
    let tiers = 3 + (rng.f() * 2.0) as usize;
    let mut y = 1.1;
    for i in 0..tiers {
        let k = 1.0 - i as f32 / tiers as f32 * 0.55;
        let hgt = 2.1 * k + 0.5;
        let lean = Vec3::new(rng.signed() * 0.12, 0.0, rng.signed() * 0.12);
        mb.frustum(Vec3::Y * y, Vec3::Y * (y + hgt) + lean, 1.75 * k, 0.0, 7, green * (0.85 + i as f32 * 0.09), true);
        y += hgt * 0.55;
    }
    mb.build()
}

fn round_tree(rng: &mut Rng) -> Mesh {
    let mut mb = MeshBuilder::new();
    mb.frustum(Vec3::ZERO, Vec3::Y * 2.6, 0.3, 0.2, 6, rgb(0.42, 0.29, 0.17), false);
    let green = rgb(0.3 + rng.f() * 0.1, 0.58 + rng.f() * 0.1, 0.2);
    mb.blob(Vec3::Y * 3.6, Vec3::new(1.7, 1.5, 1.7), 0.22, rng, green, 0.1);
    mb.blob(Vec3::new(0.8, 4.5, 0.3), Vec3::splat(1.0), 0.25, rng, green * 1.1, 0.1);
    mb.build()
}

fn cactus(rng: &mut Rng) -> Mesh {
    let mut mb = MeshBuilder::new();
    let g = rgb(0.25, 0.55, 0.3);
    let h = 3.0 + rng.f() * 1.2;
    mb.frustum(Vec3::ZERO, Vec3::Y * h, 0.38, 0.32, 6, g, true);
    for s in [-1.0f32, 1.0] {
        if rng.chance(0.8) {
            let y = 1.0 + rng.f() * 1.0;
            let x = s * (0.9 + rng.f() * 0.3);
            mb.frustum(Vec3::new(s * 0.2, y, 0.0), Vec3::new(x, y + 0.15, 0.0), 0.22, 0.22, 5, g * 0.95, true);
            mb.frustum(Vec3::new(x, y, 0.0), Vec3::new(x, y + 1.0 + rng.f(), 0.0), 0.22, 0.18, 5, g * 1.05, true);
        }
    }
    mb.build()
}

fn rock(rng: &mut Rng) -> Mesh {
    let mut mb = MeshBuilder::new();
    let grey = 0.42 + rng.f() * 0.12;
    mb.blob(Vec3::ZERO, Vec3::new(1.0, 0.8, 1.0), 0.28, rng, rgb(grey, grey * 0.97, grey * 0.94), 0.12);
    mb.build()
}

fn barrel(color: Vec3) -> Mesh {
    let mut mb = MeshBuilder::new();
    mb.frustum(Vec3::Y * -0.58, Vec3::Y * 0.58, 0.42, 0.42, 10, color, true);
    for y in [-0.3f32, 0.3] {
        mb.frustum(Vec3::Y * (y - 0.05), Vec3::Y * (y + 0.05), 0.45, 0.45, 10, color * 0.6, false);
    }
    mb.build()
}

fn fuel_can() -> Mesh {
    let mut mb = MeshBuilder::new();
    let red = rgb(0.9, 0.16, 0.1);
    mb.cuboid(Vec3::ZERO, Vec3::new(0.42, 0.55, 0.24), red);
    mb.cuboid(Vec3::new(0.0, 0.0, 0.25), Vec3::new(0.3, 0.08, 0.02), rgb(1.0, 0.85, 0.2));
    mb.cuboid(Vec3::new(0.0, 0.0, -0.25), Vec3::new(0.3, 0.08, 0.02), rgb(1.0, 0.85, 0.2));
    mb.cuboid(Vec3::new(-0.1, 0.68, 0.0), Vec3::new(0.24, 0.05, 0.08), rgb(0.15, 0.15, 0.15));
    mb.cuboid(Vec3::new(-0.32, 0.61, 0.0), Vec3::new(0.04, 0.08, 0.08), rgb(0.15, 0.15, 0.15));
    mb.frustum(Vec3::new(0.25, 0.55, 0.0), Vec3::new(0.48, 0.85, 0.0), 0.08, 0.06, 6, rgb(0.15, 0.15, 0.15), true);
    mb.build()
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut rng = Rng::new(0xbea2);
    commands.insert_resource(PropAssets {
        material: materials.add(StandardMaterial { perceptual_roughness: 0.9, reflectance: 0.2, ..default() }),
        beacon_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.62, 0.1, 0.38),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
        beacon: meshes.add(Cylinder::new(0.55, 26.0).mesh().resolution(6)),
        pines: (0..4).map(|_| meshes.add(pine(&mut rng))).collect(),
        rounds: (0..3).map(|_| meshes.add(round_tree(&mut rng))).collect(),
        cacti: (0..3).map(|_| meshes.add(cactus(&mut rng))).collect(),
        rocks: (0..5).map(|_| meshes.add(rock(&mut rng))).collect(),
        barrels: vec![meshes.add(barrel(rgb(0.8, 0.2, 0.15))), meshes.add(barrel(rgb(0.2, 0.4, 0.75))), meshes.add(barrel(rgb(0.9, 0.7, 0.15)))],
        fuel: meshes.add(fuel_can()),
    });
}

fn ramp_mesh(r: &Ramp) -> Mesh {
    let mut mb = MeshBuilder::new();
    let (d, p) = (Vec3::new(r.dir.x, 0.0, r.dir.y), Vec3::new(r.dir.perp().x, 0.0, r.dir.perp().y));
    let hw = r.width * 0.5;
    let planks = 6;
    let at = |u: f32, v: f32, y: f32| d * u + p * v + Vec3::Y * y;
    for i in 0..planks {
        let (u0, u1) = (r.len * i as f32 / planks as f32, r.len * (i + 1) as f32 / planks as f32);
        let (y0, y1) = (r.height * u0 / r.len, r.height * u1 / r.len);
        let wood = if i % 2 == 0 { rgb(0.72, 0.52, 0.3) } else { rgb(0.62, 0.43, 0.24) };
        mb.quad_facing(at(u0, -hw + 0.4, y0), at(u0, hw - 0.4, y0), at(u1, hw - 0.4, y1), at(u1, -hw + 0.4, y1), wood, Vec3::Y);
        // Hazard-striped edges.
        for s in [-1.0f32, 1.0] {
            let edge = if i % 2 == 0 { rgb(1.0, 0.75, 0.1) } else { rgb(0.12, 0.12, 0.12) };
            mb.quad_facing(at(u0, s * hw, y0), at(u0, s * (hw - 0.4), y0), at(u1, s * (hw - 0.4), y1), at(u1, s * hw, y1), edge, Vec3::Y);
        }
    }
    let side = rgb(0.5, 0.34, 0.2);
    let deep = -5.0;
    for s in [-1.0f32, 1.0] {
        mb.quad_facing(at(0.0, s * hw, deep), at(r.len, s * hw, deep), at(r.len, s * hw, r.height), at(0.0, s * hw, 0.0), side, p * s);
    }
    mb.quad_facing(at(r.len, -hw, deep), at(r.len, hw, deep), at(r.len, hw, r.height), at(r.len, -hw, r.height), side * 0.8, d);
    mb.build()
}

#[allow(clippy::too_many_arguments)]
fn populate(c: IVec2, commands: &mut Commands, world: &mut World, obstacles: &mut Obstacles, assets: &PropAssets, meshes: &mut Assets<Mesh>) {
    let mut rng = Rng::for_cell(c.x, c.y, world.seed, 1);
    let origin = Vec2::new(c.x as f32, c.y as f32) * CHUNK_SIZE;
    let mid = origin + Vec2::splat(CHUNK_SIZE * 0.5);
    let biome = world.biome(mid.x, mid.y);
    let mut list: Vec<Obstacle> = Vec::new();
    let spot = |rng: &mut Rng, world: &World, margin: f32, max_slope: f32| -> Option<Vec3> {
        let x = origin.x + rng.range(margin, CHUNK_SIZE - margin);
        let z = origin.y + rng.range(margin, CHUNK_SIZE - margin);
        let h = world.terrain_height(x, z);
        (h > WATER_Y + 0.7 && (x * x + z * z) > 38.0 * 38.0 && world.slope(x, z) < max_slope).then_some(Vec3::new(x, h, z))
    };

    // Trees
    let n_trees = match biome {
        Biome::Forest => rng.int(8, 14),
        Biome::Meadow => rng.int(1, 4),
        Biome::Desert => rng.int(0, 2),
    };
    for _ in 0..n_trees {
        let Some(p) = spot(&mut rng, world, 1.0, 0.65) else { continue };
        if p.y > 62.0 && rng.chance(0.7) {
            continue;
        }
        let scale = rng.range(0.8, 1.75);
        let (mesh, breakable, radius, height) = if biome == Biome::Desert {
            (assets.cacti[rng.int(0, 2) as usize].clone(), true, 0.5 * scale, 3.2 * scale)
        } else if rng.chance(0.72) {
            (assets.pines[rng.int(0, 3) as usize].clone(), scale < 1.5, 0.42 * scale, 5.0 * scale)
        } else {
            (assets.rounds[rng.int(0, 2) as usize].clone(), scale < 1.5, 0.42 * scale, 5.0 * scale)
        };
        let e = commands
            .spawn((
                InChunk(c),
                Mesh3d(mesh),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_translation(p - Vec3::Y * 0.25).with_rotation(Quat::from_rotation_y(rng.f() * 6.28)).with_scale(Vec3::splat(scale)),
            ))
            .id();
        list.push(Obstacle { pos: p, radius, height, kind: ObKind::Tree, breakable, entity: e });
    }

    // Rocks
    let n_rocks = match biome {
        Biome::Desert => rng.int(1, 4),
        _ => rng.int(0, 2),
    };
    for _ in 0..n_rocks {
        let Some(p) = spot(&mut rng, world, 2.0, 1.2) else { continue };
        let scale = rng.range(0.7, 2.6);
        let squash = rng.range(0.7, 1.15);
        let centre = p + Vec3::Y * 0.15 * scale;
        let e = commands
            .spawn((
                InChunk(c),
                Mesh3d(assets.rocks[rng.int(0, 4) as usize].clone()),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_translation(centre).with_rotation(Quat::from_rotation_y(rng.f() * 6.28)).with_scale(Vec3::new(scale, scale * squash, scale)),
            ))
            .id();
        list.push(Obstacle { pos: centre, radius: 0.92 * scale * squash.min(1.0), height: 0.0, kind: ObKind::Rock, breakable: false, entity: e });
    }

    // Barrel piles: free physics toys.
    if rng.chance(0.24) {
        if let Some(p) = spot(&mut rng, world, 8.0, 0.35) {
            let n = rng.int(3, 8);
            for i in 0..n {
                let a = i as f32 * 2.4;
                let r = 0.6 + i as f32 * 0.45;
                let (x, z) = (p.x + a.cos() * r, p.z + a.sin() * r);
                let pos = Vec3::new(x, world.terrain_height(x, z) + 0.58, z);
                let mut body = Body::new(BodyKind::Barrel, pos, Quat::IDENTITY, 0.56, 45.0, 0.0);
                body.asleep = true;
                commands.spawn((
                    InChunk(c),
                    body,
                    Mesh3d(assets.barrels[rng.int(0, 2) as usize].clone()),
                    MeshMaterial3d(assets.material.clone()),
                    Transform::from_translation(pos),
                ));
            }
        }
    }

    // Fuel
    if rng.chance(0.2) {
        if let Some(p) = spot(&mut rng, world, 4.0, 0.5) {
            commands
                .spawn((
                    InChunk(c),
                    FuelCan { base_y: p.y + 1.5, phase: rng.f() * 6.28 },
                    Mesh3d(assets.fuel.clone()),
                    MeshMaterial3d(assets.material.clone()),
                    Transform::from_translation(p + Vec3::Y * 1.5).with_scale(Vec3::splat(1.6)),
                ))
                .with_children(|ch| {
                    ch.spawn((Mesh3d(assets.beacon.clone()), MeshMaterial3d(assets.beacon_material.clone()), Transform::from_xyz(0.0, 8.5, 0.0), bevy::pbr::NotShadowCaster));
                });
        }
    }

    // Kickers
    if rng.chance(0.2) {
        if let Some(p) = spot(&mut rng, world, 6.0, 0.25) {
            let a = rng.f() * std::f32::consts::TAU;
            let dir = Vec2::new(a.cos(), a.sin());
            let big = rng.chance(0.3);
            let (len, width, height) = if big { (15.0, 9.0, 5.5) } else { (10.0, 7.0, 3.3) };
            let end = Vec2::new(p.x, p.z) + dir * len;
            let h_end = world.terrain_height(end.x, end.y);
            if (h_end - p.y).abs() < 1.8 && h_end > WATER_Y + 0.5 {
                let ramp = Ramp { chunk: c, pos: Vec2::new(p.x, p.z), dir, len, width, height, base_y: p.y - 0.12 };
                commands.spawn((
                    InChunk(c),
                    Mesh3d(meshes.add(ramp_mesh(&ramp))),
                    MeshMaterial3d(assets.material.clone()),
                    Transform::from_xyz(p.x, ramp.base_y, p.z),
                ));
                world.ramps.push(ramp);
            }
        }
    }

    obstacles.0.insert(c, list);
}

#[allow(clippy::too_many_arguments)]
fn stream_objects(
    mut commands: Commands,
    mut world: ResMut<World>,
    mut obstacles: ResMut<Obstacles>,
    mut loaded: ResMut<Loaded>,
    assets: Res<PropAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    truck: Query<&Truck>,
    owned: Query<(Entity, &InChunk)>,
    mut populated: EventWriter<ChunkPopulated>,
    mut started: Local<bool>,
) {
    let Ok(truck) = truck.single() else { return };
    let center = chunk_of(truck.pos);

    let stale: Vec<IVec2> = loaded.0.iter().copied().filter(|c| (*c - center).abs().max_element() > OBJECT_RADIUS + 1).collect();
    if !stale.is_empty() {
        for (e, ic) in &owned {
            if stale.contains(&ic.0) {
                commands.entity(e).despawn();
            }
        }
        for c in stale {
            loaded.0.remove(&c);
            obstacles.0.remove(&c);
            world.ramps.retain(|r| r.chunk != c);
        }
    }

    let mut missing: Vec<IVec2> = Vec::new();
    for dz in -OBJECT_RADIUS..=OBJECT_RADIUS {
        for dx in -OBJECT_RADIUS..=OBJECT_RADIUS {
            let c = center + IVec2::new(dx, dz);
            if !loaded.0.contains(&c) {
                missing.push(c);
            }
        }
    }
    missing.sort_by_key(|c| (*c - center).length_squared());
    let budget = if *started { 3 } else { usize::MAX };
    *started = true;
    for c in missing.into_iter().take(budget) {
        populate(c, &mut commands, &mut world, &mut obstacles, &assets, &mut meshes);
        loaded.0.insert(c);
        populated.write(ChunkPopulated(c));
    }
}

/// Truck shoves barrels, logs and already-airborne bears around.
fn truck_vs_bodies(mut truck: Query<&mut Truck>, mut bodies: Query<&mut Body>, mut fx: EventWriter<Fx>) {
    let Ok(mut t) = truck.single_mut() else { return };
    for mut b in &mut bodies {
        if (b.pos - t.pos).length_squared() > 30.0 {
            continue;
        }
        let Some((p, n, pen)) = t.sphere_contact(b.pos, b.radius) else { continue };
        b.pos += n * pen;
        let closing = (t.point_vel(p) - b.vel).dot(n);
        if closing <= 0.0 {
            continue;
        }
        let j = 1.6 * closing / (t.inv_mass_at(p, n) + 1.0 / b.mass);
        // The truck barely notices; the prop very much does.
        t.apply_impulse(-n * j * 0.5, p);
        let dv = j / b.mass;
        b.vel += n * dv + Vec3::Y * (1.5 + closing * 0.3);
        b.angvel += n.cross(Vec3::Y) * closing * 0.9 + Vec3::new(closing * 0.13, closing * 0.21, -closing * 0.17);
        b.asleep = false;
        b.rest_time = 0.0;
        b.origin = b.pos;
        if b.hit_cd <= 0.0 {
            b.hit_cd = 0.4;
            match b.kind {
                BodyKind::Bear if closing > 5.0 => {
                    fx.write(Fx::BearHit { pos: b.pos, vel: b.vel, juggle: !b.grounded });
                }
                BodyKind::Barrel | BodyKind::Debris => {
                    fx.write(Fx::BarrelHit { pos: b.pos, strength: closing });
                }
                _ => {}
            }
        }
    }
}

fn body_physics(time: Res<Time>, world: Res<World>, obstacles: Res<Obstacles>, mut bodies: Query<&mut Body>) {
    let dt = time.delta_secs();
    for mut b in &mut bodies {
        b.prev_pos = b.pos;
        b.prev_rot = b.rot;
        b.hit_cd -= dt;
        if b.asleep {
            continue;
        }
        b.age += dt;
        b.vel.y -= GRAVITY * dt;
        b.vel *= 1.0 - 0.05 * dt;

        // Float.
        let depth = WATER_Y - (b.pos.y - b.radius * 0.3);
        if depth > 0.0 {
            b.vel.y += GRAVITY * 1.8 * depth.min(1.0) * dt;
            b.vel *= 1.0 - (2.5 * dt).min(1.0);
            b.angvel *= 1.0 - (2.0 * dt).min(1.0);
        }

        let step = b.vel * dt;
        b.pos += step;
        let spin = b.angvel * dt;
        b.rot = (Quat::from_scaled_axis(spin) * b.rot).normalize();

        b.grounded = false;
        let g = world.ground(b.pos.x, b.pos.z);
        if b.pos.y - b.radius < g {
            let n = world.ground_normal(b.pos.x, b.pos.z);
            b.pos.y = g + b.radius;
            b.grounded = true;
            let vn = b.vel.dot(n);
            if vn < 0.0 {
                let bounce = if vn < -2.5 { b.bounce } else { 0.0 };
                let dv = n * vn * (1.0 + bounce);
                b.vel -= dv;
            }
            let fr = (3.0 * dt).min(1.0);
            let vt = b.vel - n * b.vel.dot(n);
            b.vel -= vt * fr;
            // Roll with the ground.
            let roll = n.cross(b.vel) / b.radius;
            let k = (8.0 * dt).min(1.0);
            b.angvel = b.angvel.lerp(roll, k);
            if b.vel.length_squared() < 0.6 {
                b.rest_time += dt;
                if b.rest_time > 0.6 {
                    b.asleep = b.kind != BodyKind::Bear;
                    b.vel = Vec3::ZERO;
                    b.angvel = Vec3::ZERO;
                }
            } else {
                b.rest_time = 0.0;
            }
        }

        // Bonk off trees and rocks.
        if let Some(list) = obstacles.0.get(&chunk_of(b.pos)) {
            for ob in list {
                let c = match ob.kind {
                    ObKind::Rock => ob.pos,
                    ObKind::Tree => Vec3::new(ob.pos.x, b.pos.y.clamp(ob.pos.y, ob.pos.y + ob.height), ob.pos.z),
                };
                let d = b.pos - c;
                let rr = ob.radius + b.radius;
                let dist2 = d.length_squared();
                if dist2 < rr * rr && dist2 > 1e-5 {
                    let dist = dist2.sqrt();
                    let n = d / dist;
                    b.pos += n * (rr - dist);
                    let vn = b.vel.dot(n);
                    if vn < 0.0 {
                        b.vel -= n * vn * 1.5;
                    }
                }
            }
        }
    }
}

fn sync_bodies(fixed: Res<Time<Fixed>>, mut q: Query<(&Body, &mut Transform)>) {
    let a = fixed.overstep_fraction();
    for (b, mut tf) in &mut q {
        if b.asleep && tf.translation.distance_squared(b.pos) < 1e-6 && b.offset == 0.0 {
            continue;
        }
        let rot = b.prev_rot.slerp(b.rot, a);
        tf.rotation = rot;
        tf.translation = b.prev_pos.lerp(b.pos, a) - rot * Vec3::Y * b.offset;
    }
}

fn fuel_cans(
    time: Res<Time>,
    mut commands: Commands,
    state: Res<State<GameState>>,
    truck: Query<&Truck>,
    mut cans: Query<(Entity, &FuelCan, &mut Transform)>,
    mut fx: EventWriter<Fx>,
) {
    let Ok(truck) = truck.single() else { return };
    let e = time.elapsed_secs();
    for (entity, can, mut tf) in &mut cans {
        tf.rotation = Quat::from_rotation_y(e * 2.0 + can.phase);
        tf.translation.y = can.base_y + (e * 2.5 + can.phase).sin() * 0.3;
        if *state.get() == GameState::Playing && tf.translation.distance_squared(truck.pos + truck.up() * 0.6) < 4.2 * 4.2 {
            fx.write(Fx::FuelPickup { pos: tf.translation });
            commands.entity(entity).despawn();
        }
    }
}

/// A snapped tree becomes a free-flying log.
fn smash_trees(mut commands: Commands, mut fx: EventReader<Fx>, trees: Query<&Transform, Without<Body>>) {
    for ev in fx.read() {
        let Fx::TreeSmash { entity, vel, .. } = ev else { continue };
        let Ok(tf) = trees.get(*entity) else { continue };
        let offset = 2.2 * tf.scale.y;
        let mut body = Body::new(BodyKind::Debris, tf.translation + Vec3::Y * offset, tf.rotation, 0.9 * tf.scale.y, 120.0, offset);
        body.vel = *vel * 1.05 + Vec3::Y * (4.0 + vel.length() * 0.25);
        let side = Vec3::Y.cross(vel.normalize_or_zero());
        body.angvel = side * (2.5 + vel.length() * 0.12);
        body.bounce = 0.3;
        body.hit_cd = 0.3;
        commands.entity(*entity).insert((body, Lifetime(14.0)));
    }
}

fn lifetimes(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Lifetime, &mut Transform)>) {
    for (e, mut l, mut tf) in &mut q {
        l.0 -= time.delta_secs();
        if l.0 < 0.6 {
            tf.scale *= 1.0 - (time.delta_secs() * 5.0).min(0.9);
        }
        if l.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
