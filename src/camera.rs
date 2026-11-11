//! Chase camera, sun and sky.

use bevy::pbr::{CascadeShadowConfigBuilder, DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::game::GameState;
use crate::meshgen::{rgb, MeshBuilder};
use crate::noise::Rng;
use crate::terrain::{World, WATER_Y};
use crate::truck::Truck;

pub const SKY: Color = Color::srgb(0.56, 0.78, 0.96);

#[derive(Component)]
pub struct ChaseCam {
    yaw: f32,
    pos: Vec3,
    look: Vec3,
    ready: bool,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(AmbientLight { color: Color::srgb(0.8, 0.88, 1.0), brightness: 750.0, ..default() })
            .add_systems(Startup, (setup, spawn_clouds))
            .add_systems(Update, drift_clouds)
            .add_systems(PostUpdate, follow.before(TransformSystem::TransformPropagate));
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.3, far: 1400.0, ..default() }),
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgba(1.0, 0.9, 0.7, 0.4),
            directional_light_exponent: 12.0,
            falloff: FogFalloff::Linear { start: 170.0, end: 500.0 },
        },
        ChaseCam { yaw: 0.0, pos: Vec3::ZERO, look: Vec3::ZERO, ready: false },
        Transform::from_xyz(0.0, 20.0, 20.0),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 10_000.0, shadows_enabled: true, color: Color::srgb(1.0, 0.96, 0.88), ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.85, 0.0)),
        // WebGL2 only supports a single shadow cascade.
        if cfg!(target_arch = "wasm32") {
            CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 95.0, ..default() }.build()
        } else {
            CascadeShadowConfigBuilder { num_cascades: 3, first_cascade_far_bound: 28.0, maximum_distance: 190.0, ..default() }.build()
        },
    ));
}

fn angle_lerp(from: f32, to: f32, k: f32) -> f32 {
    let d = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    from + d * k
}

fn follow(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    world: Res<World>,
    truck: Query<(&Truck, &Transform), Without<ChaseCam>>,
    mut cam: Query<(&mut ChaseCam, &mut Transform, &mut Projection)>,
) {
    let (Ok((t, ttf)), Ok((mut c, mut tf, mut proj))) = (truck.single(), cam.single_mut()) else { return };
    let dt = time.delta_secs().min(0.05);
    let speed = t.speed();
    let flat_vel = Vec3::new(t.vel.x, 0.0, t.vel.z);
    let fwd = t.forward();
    let mut dir = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    if flat_vel.length() > 6.0 && flat_vel.dot(dir) > -2.0 {
        // Follow where the truck is going, not just where it points: drifts look great.
        dir = (dir + flat_vel.normalize() * 1.3).normalize_or(dir);
    }
    let want = (-dir.x).atan2(-dir.z);
    let title = *state.get() == GameState::Title;
    if title {
        c.yaw += dt * 0.3;
    } else {
        c.yaw = angle_lerp(c.yaw, want, 1.0 - (-3.2 * dt).exp());
    }
    let mut yaw = c.yaw;
    if keys.pressed(KeyCode::KeyC) {
        yaw += std::f32::consts::PI;
    }
    let back = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let (dist, height) = if title { (13.0, 3.2) } else { (11.5 + speed * 0.08, 4.4 + speed * 0.025) };
    let target = ttf.translation + Vec3::Y * 1.9;
    let mut desired = target + back * dist + Vec3::Y * height;
    desired.y = desired.y.max(world.ground(desired.x, desired.z) + 1.6).max(WATER_Y + 1.2);
    if !c.ready {
        c.ready = true;
        c.pos = desired;
        c.look = target;
        c.yaw = want;
    }
    let k = 1.0 - (-9.0 * dt).exp();
    c.pos = c.pos.lerp(desired, k);
    c.pos.y = c.pos.y.max(world.ground(c.pos.x, c.pos.z) + 1.2);
    c.look = c.look.lerp(target, 1.0 - (-16.0 * dt).exp());

    let e = time.elapsed_secs();
    let s = t.shake * t.shake * 0.55;
    let shake = Vec3::new((e * 71.0).sin(), (e * 93.0).cos(), (e * 57.0).sin()) * s;
    *tf = Transform::from_translation(c.pos + shake).looking_at(c.look + shake * 0.5, Vec3::Y);

    if let Projection::Perspective(p) = &mut *proj {
        let want_fov = (62.0 + speed * 0.5 + if t.boosting { 8.0 } else { 0.0 }).min(98.0f32).to_radians();
        p.fov += (want_fov - p.fov) * (dt * 4.0).min(1.0);
    }
}

#[derive(Component)]
struct Cloud(Vec3);

const CLOUD_SPAN: f32 = 700.0;

fn spawn_clouds(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut rng = Rng::new(4242);
    let mat = materials.add(StandardMaterial {
        emissive: LinearRgba::new(0.35, 0.37, 0.42, 1.0),
        perceptual_roughness: 1.0,
        fog_enabled: false,
        ..default()
    });
    let shapes: Vec<Handle<Mesh>> = (0..4)
        .map(|_| {
            let mut mb = MeshBuilder::new();
            for i in 0..4 {
                let c = Vec3::new(i as f32 * 11.0 - 16.0 + rng.signed() * 3.0, rng.signed() * 2.0, rng.signed() * 6.0);
                let r = rng.range(8.0, 15.0);
                mb.blob(c, Vec3::new(r, r * 0.5, r * 0.8), 0.18, &mut rng, rgb(1.0, 1.0, 1.0), 0.03);
            }
            meshes.add(mb.build())
        })
        .collect();
    for _ in 0..26 {
        let p = Vec3::new(rng.signed() * CLOUD_SPAN, rng.range(120.0, 190.0), rng.signed() * CLOUD_SPAN);
        commands.spawn((
            Cloud(p),
            Mesh3d(shapes[rng.int(0, 3) as usize].clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(p).with_rotation(Quat::from_rotation_y(rng.f() * 6.28)).with_scale(Vec3::splat(rng.range(0.8, 1.9))),
            bevy::pbr::NotShadowCaster,
        ));
    }
}

/// Clouds drift on the wind and wrap around the camera so the sky never empties.
fn drift_clouds(time: Res<Time>, cam: Query<&Transform, (With<ChaseCam>, Without<Cloud>)>, mut clouds: Query<(&mut Cloud, &mut Transform)>) {
    let Ok(cam) = cam.single() else { return };
    let wrap = |v: f32| (v + CLOUD_SPAN).rem_euclid(CLOUD_SPAN * 2.0) - CLOUD_SPAN;
    for (mut c, mut tf) in &mut clouds {
        c.0.x += time.delta_secs() * 4.0;
        tf.translation.x = cam.translation.x + wrap(c.0.x - cam.translation.x);
        tf.translation.z = cam.translation.z + wrap(c.0.z - cam.translation.z);
    }
}
