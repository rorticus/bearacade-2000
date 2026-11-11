//! Bears. They idle, wander, dance, flee, occasionally charge the truck -
//! and when they meet the bull bar they go a very long way.

use std::time::Duration;

use bevy::prelude::*;

use crate::game::{Fx, GameState, Sim};
use crate::noise::{hash2, Rng};
use crate::props::{Body, BodyKind, ChunkPopulated, InChunk};
use crate::terrain::{Biome, World, CHUNK_SIZE, WATER_Y};
use crate::truck::{AutopilotTarget, Truck};

const IDLE: usize = 0;
const WALK: usize = 1;
const RUN: usize = 2;
const FALL: usize = 3;
const DANCE: usize = 4;
/// The generated model is 1 unit tall and centred on its middle.
const MODEL_HEIGHT: f32 = 2.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Idle,
    Wander,
    Dance,
    Flee,
    Charge,
    Flying,
    Dazed,
}

#[derive(Component)]
pub struct Bear {
    mode: Mode,
    timer: f32,
    heading: f32,
    target_heading: f32,
    home: Vec2,
    brave: bool,
    raver: bool,
    scale: f32,
    player: Option<Entity>,
    anim: usize,
}

#[derive(Resource)]
struct BearAssets {
    scene: Handle<Scene>,
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
}

pub struct BearsPlugin;

impl Plugin for BearsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(FixedUpdate, truck_vs_bears.in_set(Sim::Bears))
            .add_systems(Update, (spawn_bears, link_players, bear_ai, bear_landing, bear_anim, autopilot_target));
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    let path = "models/bear.glb";
    let clips: Vec<Handle<AnimationClip>> = (0..5).map(|i| assets.load(GltfAssetLabel::Animation(i).from_asset(path))).collect();
    let (graph, nodes) = AnimationGraph::from_clips(clips);
    commands.insert_resource(BearAssets {
        scene: assets.load(GltfAssetLabel::Scene(0).from_asset(path)),
        graph: graphs.add(graph),
        nodes,
    });
}

fn spawn_bears(mut commands: Commands, mut events: EventReader<ChunkPopulated>, world: Res<World>, assets: Res<BearAssets>) {
    for ChunkPopulated(c) in events.read() {
        let mut rng = Rng::for_cell(c.x, c.y, world.seed, 2);
        let origin = Vec2::new(c.x as f32, c.y as f32) * CHUNK_SIZE;
        let mid = origin + Vec2::splat(CHUNK_SIZE * 0.5);
        let chance = match world.biome(mid.x, mid.y) {
            Biome::Forest => 0.62,
            Biome::Meadow => 0.55,
            Biome::Desert => 0.42,
        };
        if !rng.chance(chance) {
            continue;
        }
        let centre = origin + Vec2::new(rng.range(10.0, CHUNK_SIZE - 10.0), rng.range(10.0, CHUNK_SIZE - 10.0));
        let rave = rng.chance(0.1);
        let count = if rave { rng.int(6, 10) } else { rng.int(1, 5) };
        for i in 0..count {
            let a = rng.f() * std::f32::consts::TAU;
            let r = if rave { 4.0 + (i % 2) as f32 * 2.5 } else { rng.range(1.5, 10.0) };
            let a = if rave { i as f32 / count as f32 * std::f32::consts::TAU } else { a };
            let p = centre + Vec2::new(a.cos(), a.sin()) * r;
            let h = world.ground(p.x, p.y);
            if h < WATER_Y + 0.5 || p.length() < 22.0 {
                continue;
            }
            let scale = if rng.chance(0.07) { rng.range(1.6, 2.1) } else { rng.range(0.85, 1.25) };
            let heading = if rave { a + std::f32::consts::PI } else { rng.f() * std::f32::consts::TAU };
            commands
                .spawn((
                    InChunk(*c),
                    Bear {
                        mode: if rave { Mode::Dance } else { Mode::Idle },
                        timer: rng.range(1.0, 5.0) + if rave { 1e6 } else { 0.0 },
                        heading,
                        target_heading: heading,
                        home: centre,
                        brave: rng.chance(0.3),
                        raver: rave,
                        scale,
                        player: None,
                        anim: usize::MAX,
                    },
                    Transform::from_xyz(p.x, h, p.y).with_rotation(Quat::from_rotation_y(heading)),
                    Visibility::default(),
                ))
                .with_children(|ch| {
                    let s = MODEL_HEIGHT * scale;
                    ch.spawn((SceneRoot(assets.scene.clone()), Transform::from_xyz(0.0, 0.5 * s, 0.0).with_scale(Vec3::splat(s))));
                });
        }
    }
}

/// Hook each freshly spawned skeleton's AnimationPlayer up to its bear.
fn link_players(
    mut commands: Commands,
    assets: Res<BearAssets>,
    new_players: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    mut bears: Query<&mut Bear>,
) {
    for player in &new_players {
        for ancestor in parents.iter_ancestors(player) {
            if let Ok(mut bear) = bears.get_mut(ancestor) {
                bear.player = Some(player);
                commands.entity(player).insert((AnimationGraphHandle(assets.graph.clone()), AnimationTransitions::new()));
                break;
            }
        }
    }
}

fn angle_towards(from: f32, to: f32, max: f32) -> f32 {
    let d = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    from + d.clamp(-max, max)
}

fn bear_ai(
    time: Res<Time>,
    state: Res<State<GameState>>,
    world: Res<World>,
    truck: Query<&Truck>,
    mut bears: Query<(Entity, &mut Bear, &mut Transform), Without<Body>>,
) {
    let Ok(truck) = truck.single() else { return };
    let dt = time.delta_secs().min(0.05);
    let playing = *state.get() == GameState::Playing;
    let tick = (time.elapsed_secs() * 10.0) as i32;
    for (entity, mut b, mut tf) in &mut bears {
        let pos = tf.translation;
        let to_truck = Vec2::new(truck.pos.x - pos.x, truck.pos.z - pos.z);
        let dist = to_truck.length();
        if dist > 230.0 {
            continue; // too far to matter; save the CPU
        }
        let mut rng = Rng::new(hash2(entity.index() as i32, tick, 77) as u64);
        let threatened = playing && dist < 34.0 && truck.speed() > 5.0;
        let toward = to_truck.x.atan2(to_truck.y);

        b.timer -= dt;
        match b.mode {
            Mode::Idle | Mode::Wander | Mode::Dance => {
                if threatened {
                    b.mode = if b.brave { Mode::Charge } else { Mode::Flee };
                    b.timer = rng.range(3.0, 6.0);
                } else if b.timer <= 0.0 {
                    let roll = rng.f();
                    if roll < 0.4 {
                        b.mode = Mode::Idle;
                        b.timer = rng.range(2.0, 5.0);
                    } else if roll < 0.85 {
                        b.mode = Mode::Wander;
                        b.timer = rng.range(3.0, 7.0);
                        let home = b.home - Vec2::new(pos.x, pos.z);
                        b.target_heading = if home.length() > 18.0 { home.x.atan2(home.y) } else { rng.f() * std::f32::consts::TAU };
                    } else {
                        b.mode = Mode::Dance;
                        b.timer = rng.range(3.0, 8.0);
                    }
                } else if b.mode == Mode::Idle && dist < 20.0 {
                    b.target_heading = toward; // stare at the truck. Judgingly.
                }
            }
            Mode::Flee => {
                b.target_heading = toward + std::f32::consts::PI + (time.elapsed_secs() * 1.7 + entity.index() as f32).sin() * 0.5;
                if dist > 60.0 || (b.timer <= 0.0 && !threatened) {
                    b.mode = if b.raver { Mode::Dance } else { Mode::Idle };
                    b.timer = rng.range(1.0, 3.0) + if b.raver { 1e6 } else { 0.0 };
                }
            }
            Mode::Charge => {
                b.target_heading = toward;
                if dist > 70.0 || !playing {
                    b.mode = Mode::Idle;
                    b.timer = 2.0;
                }
            }
            Mode::Dazed => {
                if b.timer <= 0.0 {
                    b.mode = if threatened { Mode::Flee } else { Mode::Idle };
                    b.timer = rng.range(2.0, 4.0);
                }
            }
            Mode::Flying => {}
        }

        let speed = match b.mode {
            Mode::Wander => 1.7,
            Mode::Flee => 7.0,
            Mode::Charge => 8.0,
            _ => 0.0,
        } * b.scale.sqrt();
        let turn = if speed > 3.0 { 5.0 } else { 2.5 };
        b.heading = angle_towards(b.heading, b.target_heading, turn * dt);

        let mut p = Vec2::new(pos.x, pos.z);
        if speed > 0.0 {
            let step = Vec2::new(b.heading.sin(), b.heading.cos()) * speed * dt;
            let next = p + step;
            let wet_now = world.terrain_height(p.x, p.y) < WATER_Y + 0.3;
            let wet_next = world.terrain_height(next.x, next.y) < WATER_Y + 0.3;
            if wet_next && !wet_now {
                b.target_heading += std::f32::consts::PI * 0.6; // bears here do not swim voluntarily
                b.heading = b.target_heading;
            } else {
                p = next;
            }
        }
        // Wading depth: keep the head above water.
        let y = world.ground(p.x, p.y).max(WATER_Y - 1.5 * b.scale);
        if b.mode == Mode::Dazed {
            tf.translation = Vec3::new(p.x, y + 0.55 * b.scale, p.y);
            tf.rotation = Quat::from_rotation_y(b.heading) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        } else {
            tf.translation = Vec3::new(p.x, y, p.y);
            tf.rotation = Quat::from_rotation_y(b.heading);
        }
    }
}

/// Walking (or dazed) bears vs. the truck. Fast hits launch them.
fn truck_vs_bears(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<GameState>>,
    mut truck: Query<&mut Truck>,
    mut bears: Query<(Entity, &mut Bear, &mut Transform), Without<Body>>,
    mut fx: EventWriter<Fx>,
) {
    let Ok(mut t) = truck.single_mut() else { return };
    let playing = *state.get() == GameState::Playing;
    for (entity, mut b, mut tf) in &mut bears {
        if (tf.translation - t.pos).length_squared() > 49.0 {
            continue;
        }
        let dazed = b.mode == Mode::Dazed;
        let centre = tf.translation + Vec3::Y * if dazed { 0.3 } else { 1.15 } * b.scale;
        let Some((p, n, pen)) = t.sphere_contact(centre, 0.95 * b.scale) else { continue };
        let v = t.point_vel(p);
        let closing = v.dot(n);
        if closing > 3.0 && playing {
            let mut rng = Rng::new(hash2(entity.index() as i32, (time.elapsed_secs() * 1000.0) as i32, 5) as u64);
            let mut body = Body::new(BodyKind::Bear, centre + n * pen, tf.rotation, 0.9 * b.scale, 90.0 * b.scale, 1.15 * b.scale);
            body.vel = v * 1.08 + n * 2.5 + Vec3::Y * (4.5 + closing * 0.42);
            body.angvel = Vec3::new(rng.signed(), rng.signed() * 0.6, rng.signed()).normalize_or_zero() * (5.0 + closing * 0.25);
            body.bounce = 0.5;
            body.hit_cd = 0.35;
            fx.write(Fx::BearHit { pos: centre, vel: body.vel, juggle: false });
            t.apply_impulse(-n * 200.0 * b.scale * closing * 0.6, p);
            t.shake = (t.shake + 0.22).min(1.0);
            b.mode = Mode::Flying;
            commands.entity(entity).insert(body);
        } else {
            // A gentle nudge just shoves the bear aside.
            let push = Vec3::new(n.x, 0.0, n.z) * pen;
            tf.translation += push;
        }
    }
}

/// A flying bear that has come to rest gets up again after a lie-down.
fn bear_landing(mut commands: Commands, mut bears: Query<(Entity, &mut Bear, &Body, &mut Transform)>, mut fx: EventWriter<Fx>) {
    for (entity, mut b, body, mut tf) in &mut bears {
        let slow = body.grounded && body.vel.length_squared() < 6.0;
        let floating = body.pos.y < WATER_Y + 0.3 && body.age > 2.5;
        if !(body.age > 0.5 && (slow || floating)) && body.age < 14.0 {
            continue;
        }
        let d = body.pos - body.origin;
        fx.write(Fx::BearLanded { pos: body.pos, dist: Vec2::new(d.x, d.z).length() });
        b.mode = Mode::Dazed;
        b.timer = 2.5 + (entity.index() % 5) as f32 * 0.4;
        let f = body.rot * Vec3::Z;
        b.heading = f.x.atan2(f.z);
        b.target_heading = b.heading;
        tf.translation = body.pos - Vec3::Y * body.radius;
        commands.entity(entity).remove::<Body>();
    }
}

fn bear_anim(assets: Res<BearAssets>, mut bears: Query<&mut Bear>, mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>) {
    for mut b in &mut bears {
        let Some(pe) = b.player else { continue };
        let (want, speed) = match b.mode {
            Mode::Idle => (IDLE, 1.0),
            Mode::Wander => (WALK, 1.0),
            Mode::Dance => (DANCE, 1.0),
            Mode::Flee => (RUN, 1.5),
            Mode::Charge => (RUN, 1.7),
            Mode::Flying => (FALL, 2.2),
            Mode::Dazed => (FALL, 0.45),
        };
        let key = ((b.mode as usize) << 4) | want;
        if b.anim == key {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(pe) else { continue };
        transitions.play(&mut player, assets.nodes[want], Duration::from_millis(180)).set_speed(speed).repeat();
        b.anim = key;
    }
}

fn autopilot_target(truck: Query<&Truck>, bears: Query<&Transform, (With<Bear>, Without<Body>)>, mut target: ResMut<AutopilotTarget>) {
    let Ok(t) = truck.single() else { return };
    target.0 = bears
        .iter()
        .map(|tf| tf.translation)
        .filter(|p| p.distance(t.pos) > 12.0)
        .min_by(|a, b| a.distance_squared(t.pos).total_cmp(&b.distance_squared(t.pos)));
}
