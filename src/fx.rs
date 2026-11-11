//! Cheap cube particles: dust, fur, splinters, splashes, boost flames.

use bevy::prelude::*;

use crate::game::Fx;
use crate::noise::Rng;
use crate::terrain::{Biome, World, WATER_Y};
use crate::truck::Truck;

#[derive(Component)]
struct Particle {
    vel: Vec3,
    spin: Vec3,
    life: f32,
    max: f32,
    gravity: f32,
    size: f32,
    grow: f32,
}

#[derive(Clone, Copy)]
enum Kind {
    Dust = 0,
    Smoke,
    Fur,
    Leaf,
    Spark,
    Water,
    Wood,
    Flame,
    Grass,
}

#[derive(Resource)]
struct FxAssets {
    cube: Handle<Mesh>,
    mats: Vec<Handle<StandardMaterial>>,
    rng: Rng,
    live: usize,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, (bursts, truck_trails, update));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut mat = |r: f32, g: f32, b: f32, glow: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            emissive: LinearRgba::new(r * glow, g * glow, b * glow, 1.0),
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    let mats = vec![
        mat(0.78, 0.68, 0.5, 0.0),
        mat(0.3, 0.3, 0.32, 0.0),
        mat(0.45, 0.28, 0.15, 0.0),
        mat(0.25, 0.55, 0.22, 0.0),
        mat(1.0, 0.8, 0.3, 3.0),
        mat(0.75, 0.9, 1.0, 0.2),
        mat(0.6, 0.42, 0.24, 0.0),
        mat(1.0, 0.45, 0.1, 5.0),
        mat(0.36, 0.5, 0.2, 0.0),
    ];
    commands.insert_resource(FxAssets { cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)), mats, rng: Rng::new(99), live: 0 });
}

#[allow(clippy::too_many_arguments)]
fn emit(commands: &mut Commands, a: &mut FxAssets, kind: Kind, pos: Vec3, vel: Vec3, spread: f32, size: f32, life: f32, n: usize) {
    for _ in 0..n {
        if a.live > 600 {
            return;
        }
        a.live += 1;
        let r = &mut a.rng;
        let jitter = Vec3::new(r.signed(), r.signed(), r.signed());
        let (gravity, grow) = match kind {
            Kind::Dust => (-1.5, 1.6),
            Kind::Grass => (9.0, 0.0),
            Kind::Smoke => (-3.0, 1.0),
            Kind::Flame => (-2.0, -0.6),
            Kind::Water => (16.0, 0.0),
            _ => (14.0, 0.0),
        };
        let s = size * (0.6 + r.f() * 0.8);
        let life = life * (0.6 + r.f() * 0.8);
        commands.spawn((
            Particle { vel: vel + jitter * spread, spin: jitter * 6.0, life, max: life, gravity, size: s, grow },
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(a.mats[kind as usize].clone()),
            Transform::from_translation(pos + jitter * 0.3).with_scale(Vec3::splat(s)),
            bevy::pbr::NotShadowCaster,
        ));
    }
}

fn bursts(mut commands: Commands, mut a: ResMut<FxAssets>, mut fx: EventReader<Fx>, truck: Query<&Truck>) {
    let a = &mut *a;
    for ev in fx.read() {
        match ev {
            Fx::BearHit { pos, vel, .. } => {
                emit(&mut commands, a, Kind::Fur, *pos, *vel * 0.5 + Vec3::Y * 4.0, 5.0, 0.28, 1.1, 16);
                emit(&mut commands, a, Kind::Spark, *pos, *vel * 0.3 + Vec3::Y * 3.0, 7.0, 0.2, 0.5, 8);
            }
            Fx::BearLanded { pos, .. } => {
                let kind = if pos.y < WATER_Y + 0.5 { Kind::Water } else { Kind::Dust };
                emit(&mut commands, a, kind, *pos, Vec3::Y * 2.5, 3.0, 0.3, 0.9, 10);
            }
            Fx::Crash { pos, strength } => {
                let n = (*strength as usize).clamp(4, 14);
                emit(&mut commands, a, Kind::Spark, *pos, Vec3::Y * 4.0, 6.0, 0.16, 0.5, n);
                emit(&mut commands, a, Kind::Dust, *pos, Vec3::Y * 2.0, 3.0, 0.3, 0.9, n / 2);
            }
            Fx::TreeSmash { pos, vel, .. } => {
                emit(&mut commands, a, Kind::Wood, *pos + Vec3::Y, *vel * 0.6 + Vec3::Y * 5.0, 5.0, 0.3, 1.2, 12);
                emit(&mut commands, a, Kind::Leaf, *pos + Vec3::Y * 3.0, *vel * 0.4 + Vec3::Y * 4.0, 6.0, 0.3, 1.5, 18);
            }
            Fx::BarrelHit { pos, strength } if *strength > 4.0 => {
                emit(&mut commands, a, Kind::Spark, *pos, Vec3::Y * 3.0, 5.0, 0.14, 0.4, 6);
            }
            Fx::Landed { impact, .. } => {
                if let Ok(t) = truck.single() {
                    let n = (*impact as usize).clamp(6, 24);
                    emit(&mut commands, a, Kind::Dust, t.pos - Vec3::Y, t.vel * 0.3 + Vec3::Y * 2.0, 5.0, 0.4, 1.0, n);
                }
            }
            Fx::Splash { pos } => {
                emit(&mut commands, a, Kind::Water, Vec3::new(pos.x, WATER_Y, pos.z), Vec3::Y * 9.0, 6.0, 0.4, 1.3, 30);
            }
            Fx::FuelPickup { pos } => {
                emit(&mut commands, a, Kind::Flame, *pos, Vec3::Y * 5.0, 5.0, 0.3, 0.7, 16);
            }
            Fx::Flipped => {
                if let Ok(t) = truck.single() {
                    emit(&mut commands, a, Kind::Smoke, t.pos, Vec3::Y * 2.0, 4.0, 0.4, 0.8, 10);
                }
            }
            _ => {}
        }
    }
}

fn truck_trails(time: Res<Time>, mut commands: Commands, mut a: ResMut<FxAssets>, world: Res<World>, truck: Query<&Truck>, mut acc: Local<f32>) {
    let Ok(t) = truck.single() else { return };
    let a = &mut *a;
    *acc += time.delta_secs();
    if *acc < 1.0 / 30.0 {
        return;
    }
    *acc = 0.0;
    let speed = t.speed();
    for (i, w) in t.wheels.iter().enumerate() {
        if !w.contact {
            continue;
        }
        let wet = w.point.y < WATER_Y + 0.1;
        let amount = w.slip * 0.9 + if i >= 2 { (speed / 45.0).min(0.8) * 0.35 } else { 0.0 };
        if a.rng.f() < amount {
            let dusty = world.biome(w.point.x, w.point.z) == Biome::Desert;
            let (kind, size, up) = if wet {
                (Kind::Water, 0.25, 5.0)
            } else if dusty {
                (Kind::Dust, 0.2 + w.slip * 0.12, 1.2)
            } else {
                (Kind::Grass, 0.13, 3.5 + w.slip * 2.0)
            };
            emit(&mut commands, a, kind, w.point + Vec3::Y * 0.15, -t.vel * 0.1 + Vec3::Y * up, 1.4, size, 0.7, 1);
        }
    }
    if t.boosting {
        let back = t.pos + t.rot * Vec3::new(0.0, 0.1, 3.1);
        for s in [-0.7f32, 0.7] {
            emit(&mut commands, a, Kind::Flame, back + t.rot * Vec3::X * s, t.vel * 0.7 - t.forward() * 6.0, 1.2, 0.42, 0.3, 2);
        }
        emit(&mut commands, a, Kind::Smoke, back, t.vel * 0.4 + Vec3::Y, 1.5, 0.22, 0.6, 1);
    }
}

fn update(time: Res<Time>, mut commands: Commands, mut a: ResMut<FxAssets>, mut q: Query<(Entity, &mut Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut tf) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            a.live = a.live.saturating_sub(1);
            continue;
        }
        p.vel.y -= p.gravity * dt;
        p.vel *= 1.0 - (1.5 * dt).min(1.0);
        tf.translation += p.vel * dt;
        tf.rotate(Quat::from_scaled_axis(p.spin * dt));
        let k = p.life / p.max;
        let age = 1.0 - k;
        tf.scale = Vec3::splat((p.size * (1.0 + p.grow * age) * k.sqrt().min(1.0)).max(0.01));
    }
}
