//! The truck: a hand-rolled rigid body with four soft, bouncy suspension
//! rays and deliberately generous arcade tuning. Think Warthog, not Scania.

use bevy::prelude::*;

use crate::game::{Config, Fx, Game, GameState, Sim};
use crate::meshgen::{rgb, MeshBuilder};
use crate::props::{ObKind, Obstacles};
use crate::terrain::{chunk_of, World, WATER_Y};

pub const MASS: f32 = 1500.0;
pub const GRAVITY: f32 = 15.0;
const INERTIA: Vec3 = Vec3::new(5200.0, 5400.0, 2100.0);
/// Suspension hardpoints, relative to the centre of mass. Forward is -Z.
const HARD: [Vec3; 4] = [
    Vec3::new(-1.2, -0.35, -1.85),
    Vec3::new(1.2, -0.35, -1.85),
    Vec3::new(-1.2, -0.35, 1.55),
    Vec3::new(1.2, -0.35, 1.55),
];
const REST: f32 = 0.75;
pub const WHEEL_R: f32 = 0.66;
const SPRING: f32 = MASS * GRAVITY / 4.0 / 0.3;
const DAMP: f32 = 2600.0;
/// Collision box, relative to the centre of mass.
pub const BOX_C: Vec3 = Vec3::new(0.0, 0.78, 0.0);
pub const BOX_H: Vec3 = Vec3::new(1.2, 1.3, 3.0);
const TOP_SPEED: f32 = 40.0;
const BOOST_SPEED: f32 = 62.0;

#[derive(Clone, Copy, Default)]
pub struct WheelState {
    pub len: f32,
    pub contact: bool,
    pub spin: f32,
    pub point: Vec3,
    pub slip: f32,
}

#[derive(Component)]
pub struct Truck {
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub angvel: Vec3,
    pub prev_pos: Vec3,
    pub prev_rot: Quat,
    pub wheels: [WheelState; 4],
    pub steer: f32,
    pub throttle: f32,
    pub boosting: bool,
    pub grounded: u8,
    pub body_contact: bool,
    pub air_time: f32,
    pub spin_acc: Vec3,
    pub upside_timer: f32,
    pub in_water: bool,
    pub reset_request: bool,
    pub shake: f32,
}

impl Truck {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            rot: Quat::IDENTITY,
            vel: Vec3::ZERO,
            angvel: Vec3::ZERO,
            prev_pos: pos,
            prev_rot: Quat::IDENTITY,
            wheels: [WheelState { len: REST, ..default() }; 4],
            steer: 0.0,
            throttle: 0.0,
            boosting: false,
            grounded: 0,
            body_contact: false,
            air_time: 0.0,
            spin_acc: Vec3::ZERO,
            upside_timer: 0.0,
            in_water: false,
            reset_request: false,
            shake: 0.0,
        }
    }

    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }

    pub fn up(&self) -> Vec3 {
        self.rot * Vec3::Y
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }

    fn inv_inertia(&self, v: Vec3) -> Vec3 {
        self.rot * ((self.rot.inverse() * v) / INERTIA)
    }

    pub fn point_vel(&self, p: Vec3) -> Vec3 {
        self.vel + self.angvel.cross(p - self.pos)
    }

    pub fn apply_impulse(&mut self, j: Vec3, p: Vec3) {
        self.vel += j / MASS;
        let dw = self.inv_inertia((p - self.pos).cross(j));
        self.angvel += dw;
    }

    /// Effective inverse mass at `p` along `n`.
    pub fn inv_mass_at(&self, p: Vec3, n: Vec3) -> f32 {
        let r = p - self.pos;
        1.0 / MASS + n.dot(self.inv_inertia(r.cross(n)).cross(r))
    }

    /// Sphere vs. the truck's collision box. Returns the contact point on the
    /// box, the normal (box -> sphere) and the penetration depth.
    pub fn sphere_contact(&self, c: Vec3, r: f32) -> Option<(Vec3, Vec3, f32)> {
        let local = self.rot.inverse() * (c - self.pos) - BOX_C;
        let q = local.clamp(-BOX_H, BOX_H);
        let d = local - q;
        let dist = d.length();
        if dist > r {
            return None;
        }
        let (n, pen) = if dist > 1e-4 {
            (d / dist, r - dist)
        } else {
            // Centre is inside the box: push out through the nearest face.
            let rem = BOX_H - local.abs();
            if rem.x < rem.y && rem.x < rem.z {
                (Vec3::X * local.x.signum(), rem.x + r)
            } else if rem.y < rem.z {
                (Vec3::Y * local.y.signum(), rem.y + r)
            } else {
                (Vec3::Z * local.z.signum(), rem.z + r)
            }
        };
        Some((self.pos + self.rot * (q + BOX_C), self.rot * n, pen))
    }

    /// Bounce the truck off something immovable at `p` (normal points from the
    /// truck into the obstacle). Returns the closing speed.
    fn bounce_static(&mut self, p: Vec3, n: Vec3, restitution: f32, friction: f32) -> f32 {
        let v = self.point_vel(p);
        let vn = v.dot(n);
        if vn <= 0.0 {
            return 0.0;
        }
        let j = (1.0 + restitution) * vn / self.inv_mass_at(p, n);
        self.apply_impulse(-n * j, p);
        let vt = v - n * vn;
        let vt_len = vt.length();
        if vt_len > 0.05 {
            let t = vt / vt_len;
            let jt = (vt_len / self.inv_mass_at(p, t)).min(friction * j);
            self.apply_impulse(-t * jt, p);
        }
        vn
    }

    /// Put the truck back on its wheels, somewhere it will not immediately
    /// get wedged on the same rock again.
    fn reset_upright(&mut self, world: &World, obstacles: &Obstacles) {
        let f = self.forward();
        let yaw = (-f.x).atan2(-f.z);
        let here = self.pos;
        let clear = |p: Vec3| {
            let c = chunk_of(p);
            (-1..=1).all(|dz| {
                (-1..=1).all(|dx| {
                    obstacles.0.get(&(c + IVec2::new(dx, dz))).is_none_or(|list| {
                        list.iter().all(|ob| Vec2::new(ob.pos.x - p.x, ob.pos.z - p.z).length() > ob.radius + 4.5)
                    })
                })
            })
        };
        'search: for ring in 0..5 {
            for k in 0..8 {
                let a = yaw + k as f32 * std::f32::consts::FRAC_PI_4;
                let p = here + Vec3::new(a.sin(), 0.0, a.cos()) * ring as f32 * 4.5;
                if clear(p) {
                    self.pos.x = p.x;
                    self.pos.z = p.z;
                    break 'search;
                }
                if ring == 0 {
                    break;
                }
            }
        }
        self.rot = Quat::from_rotation_y(yaw);
        self.pos.y = world.ground(self.pos.x, self.pos.z).max(WATER_Y) + 2.4;
        self.vel = Vec3::Y * 2.0;
        self.angvel = Vec3::ZERO;
        self.upside_timer = 0.0;
        self.prev_pos = self.pos;
        self.prev_rot = self.rot;
    }
}

/// Where the self-driving smoke test wants to go.
#[derive(Resource, Default)]
pub struct AutopilotTarget(pub Option<Vec3>);

#[derive(Component)]
pub struct WheelVisual(pub usize);

pub struct TruckPlugin;

impl Plugin for TruckPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutopilotTarget>()
            .add_systems(Startup, spawn_truck)
            .add_systems(FixedUpdate, truck_physics.in_set(Sim::Truck))
            .add_systems(Update, (truck_keys, sync_truck).chain());
    }
}

fn spawn_truck(mut commands: Commands, assets: Res<AssetServer>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, world: Res<World>) {
    let y = world.raw_height(0.0, 0.0) + 2.2;
    commands
        .spawn((Truck::new(Vec3::new(0.0, y, 0.0)), Transform::from_xyz(0.0, y, 0.0), Visibility::default()))
        .with_children(|p| {
            // Model is 1 unit long, centred, nose towards +Z.
            p.spawn((
                SceneRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("models/truck.glb"))),
                Transform::from_xyz(0.0, 0.78, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::PI)).with_scale(Vec3::splat(6.0)),
            ));
        });

    // Chunky off-road wheel, axle along X.
    let mut mb = MeshBuilder::new();
    let (w, r) = (0.29, WHEEL_R);
    let tyre = rgb(0.09, 0.09, 0.1);
    mb.frustum(Vec3::X * -w, Vec3::X * w, r, r, 12, tyre, false);
    mb.frustum(Vec3::X * w, Vec3::X * (w + 0.06), r, r * 0.62, 12, tyre * 1.4, false);
    mb.frustum(Vec3::X * -w, Vec3::X * -(w + 0.06), r, r * 0.62, 12, tyre * 1.4, false);
    mb.frustum(Vec3::X * (w - 0.08), Vec3::X * (w + 0.07), r * 0.62, r * 0.5, 8, rgb(0.92, 0.5, 0.12), true);
    mb.frustum(Vec3::X * -(w - 0.08), Vec3::X * -(w + 0.07), r * 0.62, r * 0.5, 8, rgb(0.92, 0.5, 0.12), true);
    // Tread blocks so the spin reads.
    for i in 0..6 {
        let a = i as f32 / 6.0 * std::f32::consts::TAU;
        let q = Quat::from_rotation_x(a);
        mb.obox(q * Vec3::new(0.0, r, 0.0), q, Vec3::new(w + 0.03, 0.05, 0.12), tyre * 0.6);
    }
    let mesh = meshes.add(mb.build());
    let mat = materials.add(StandardMaterial { perceptual_roughness: 0.85, ..default() });
    for i in 0..4 {
        commands.spawn((WheelVisual(i), Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
    }
}

/// Edge-triggered keys live in Update; held keys are read in the fixed step.
fn truck_keys(keys: Res<ButtonInput<KeyCode>>, state: Res<State<GameState>>, mut truck: Query<&mut Truck>, mut fx: EventWriter<Fx>) {
    let Ok(mut t) = truck.single_mut() else { return };
    if *state.get() != GameState::Playing {
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        t.reset_request = true;
    }
    if keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight) {
        fx.write(Fx::BoostStart);
    }
    if keys.just_pressed(KeyCode::KeyH) {
        fx.write(Fx::Horn);
    }
}

#[allow(clippy::too_many_arguments)]
fn truck_physics(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    config: Res<Config>,
    config_target: Res<AutopilotTarget>,
    game: Res<Game>,
    world: Res<World>,
    mut obstacles: ResMut<Obstacles>,
    mut truck: Query<&mut Truck>,
    mut fx: EventWriter<Fx>,
) {
    let Ok(mut t) = truck.single_mut() else { return };
    let t = &mut *t;
    let dt = time.delta_secs();
    t.prev_pos = t.pos;
    t.prev_rot = t.rot;

    // ---- input ----------------------------------------------------------
    let playing = *state.get() == GameState::Playing && game.fuel > 0.0;
    let key = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    let (mut throttle, mut steer_in, mut handbrake, mut boost) = (0.0f32, 0.0f32, !playing, false);
    let (mut roll_in, mut pitch_in) = (0.0f32, 0.0f32);
    if playing {
        if key(KeyCode::KeyW, KeyCode::ArrowUp) {
            throttle += 1.0;
        }
        if key(KeyCode::KeyS, KeyCode::ArrowDown) {
            throttle -= 1.0;
        }
        if key(KeyCode::KeyA, KeyCode::ArrowLeft) {
            steer_in += 1.0;
        }
        if key(KeyCode::KeyD, KeyCode::ArrowRight) {
            steer_in -= 1.0;
        }
        if keys.pressed(KeyCode::KeyQ) {
            roll_in += 1.0;
        }
        if keys.pressed(KeyCode::KeyE) {
            roll_in -= 1.0;
        }
        handbrake = keys.pressed(KeyCode::Space);
        boost = key(KeyCode::ShiftLeft, KeyCode::ShiftRight);
        pitch_in = throttle;
        if config.autopilot {
            // Weave around so the smoke test covers steering too.
            // Hunt the nearest bear so the smoke test covers the fun part.
            let e = time.elapsed_secs();
            throttle = 1.0;
            steer_in = (e * 0.45).sin() * 0.4;
            if config.straight {
                steer_in = 0.0;
            } else if let Some(target) = config_target.0 {
                let d = target - t.pos;
                let f = t.forward();
                let diff = (-d.x).atan2(-d.z) - (-f.x).atan2(-f.z);
                let diff = (diff + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                steer_in = (diff * 2.5).clamp(-1.0, 1.0);
            }
            boost = (e % 9.0) > 6.5 || config.straight;
        }
    }
    if boost && throttle <= 0.0 {
        throttle = 1.0;
    }
    t.throttle = throttle;
    t.boosting = boost;

    if t.reset_request {
        t.reset_request = false;
        t.reset_upright(&world, &obstacles);
        fx.write(Fx::Flipped);
    }

    let fwd = t.forward();
    let up = t.up();
    let down = -up;
    let fwd_speed = t.vel.dot(fwd);
    let speed = t.speed();

    let max_steer = 0.6 / (1.0 + speed * 0.085);
    let target = steer_in * max_steer;
    t.steer += (target - t.steer) * (dt * 9.0).min(1.0);

    let mut force = Vec3::NEG_Y * GRAVITY * MASS;
    let mut torque = Vec3::ZERO;
    let mut add_force = |f: Vec3, at: Vec3, pos: Vec3| {
        force += f;
        torque += (at - pos).cross(f);
    };

    // ---- suspension + tyres ---------------------------------------------
    let (top, accel) = if boost { (BOOST_SPEED, 26.0) } else { (TOP_SPEED, 15.0) };
    let mut grounded = 0u8;
    for i in 0..4 {
        let hp = t.pos + t.rot * HARD[i];
        let mut w = t.wheels[i];
        w.contact = false;
        w.slip = 0.0;
        let mut len = REST;
        if down.y < -0.2 {
            // March the suspension ray down onto the height field.
            let mut tt = 0.0;
            for _ in 0..3 {
                let p = hp + down * tt;
                tt += (p.y - world.ground(p.x, p.z)) / (-down.y);
            }
            if tt < REST + WHEEL_R {
                let comp = REST + WHEEL_R - tt;
                len = (tt - WHEEL_R).clamp(0.08, REST);
                let cp = hp + down * tt.max(0.0);
                let n = world.ground_normal(cp.x, cp.z);
                let v = t.point_vel(cp);
                let mut fs = SPRING * comp - DAMP * v.dot(n);
                if comp > REST * 0.8 {
                    fs += SPRING * 5.0 * (comp - REST * 0.8); // bump stop
                }
                let fs = fs.clamp(0.0, MASS * GRAVITY * 3.5);
                add_force(n * fs, cp, t.pos);

                let steer_rot = if i < 2 { Quat::from_rotation_y(t.steer) } else { Quat::IDENTITY };
                let wf = t.rot * steer_rot * Vec3::NEG_Z;
                let wfwd = (wf - n * wf.dot(n)).normalize_or_zero();
                let side = wfwd.cross(n);
                let (v_long, v_lat) = (v.dot(wfwd), v.dot(side));
                let rear = i >= 2;
                let quarter = MASS / 4.0;
                let mut mu = 1.35;
                let mut f_lat = -v_lat * quarter * 13.0;
                let f_long;
                if handbrake && rear {
                    mu = 0.5;
                    f_lat *= 0.35;
                    f_long = -v_long * quarter * 5.0;
                } else if handbrake {
                    f_long = -v_long * quarter * if playing { 0.6 } else { 6.0 };
                } else if throttle > 0.0 {
                    f_long = quarter * accel * (1.0 - (fwd_speed / top).max(0.0).powi(2)).max(0.0);
                    w.spin += dt * 4.0 * (1.0 - (fwd_speed / 20.0).clamp(0.0, 1.0)); // wheelspin flourish
                } else if throttle < 0.0 {
                    if fwd_speed > 2.0 {
                        f_long = -quarter * 26.0;
                    } else {
                        f_long = -quarter * 11.0 * (1.0 - (fwd_speed / -16.0).max(0.0).powi(2)).max(0.0);
                    }
                } else {
                    f_long = -v_long * quarter * 0.4;
                }
                let fmax = mu * fs.max(quarter * GRAVITY * 0.55);
                let mag = (f_lat * f_lat + f_long * f_long).sqrt();
                let k = if mag > fmax { fmax / mag } else { 1.0 };
                w.slip = ((mag - fmax) / fmax).clamp(0.0, 2.0) * (speed / 10.0).min(1.0);
                // Tyre forces act close to the centre of mass: less tippy.
                let ap = cp + up * ((t.pos - cp).dot(up) - 0.15);
                add_force((wfwd * f_long + side * f_lat) * k, ap, t.pos);

                w.contact = true;
                w.point = cp;
                if !(handbrake && rear) {
                    w.spin += v_long / WHEEL_R * dt;
                }
                grounded += 1;
            }
        }
        if !w.contact {
            w.spin += (throttle * 30.0 + fwd_speed / WHEEL_R * 0.5) * dt;
        }
        w.len += (len - w.len) * (dt * 30.0).min(1.0);
        t.wheels[i] = w;
    }

    // ---- air control, drag, water -----------------------------------------
    let airborne = grounded == 0 && !t.body_contact;
    let mut ang_damp = 0.4;
    if airborne {
        let local = Vec3::new(-pitch_in * 3.2, steer_in * 2.6, roll_in * 4.5);
        t.angvel += t.rot * local * dt;
        if pitch_in == 0.0 && roll_in == 0.0 {
            // A little cat-like self-righting keeps landings fun.
            t.angvel += up.cross(Vec3::Y) * 2.2 * dt;
        }
        ang_damp = 0.7;
    } else {
        if roll_in != 0.0 {
            // On the ground Q/E rock the truck: handy for un-wedging.
            t.angvel += t.rot * Vec3::Z * roll_in * 3.0 * dt;
        } else if grounded >= 1 && up.y > 0.3 {
            // Invisible anti-roll bar: steering alone should slide, not capsize.
            let right = t.rot * Vec3::X;
            t.angvel += fwd * right.y * 7.0 * dt;
        }
    }
    force -= t.vel * (0.02 * MASS + 0.35 * speed);

    let depth = WATER_Y - (t.pos.y - 0.8);
    let was_in_water = t.in_water;
    t.in_water = depth > 0.0;
    if t.in_water {
        force += Vec3::Y * MASS * GRAVITY * (depth / 1.0).clamp(0.0, 1.9);
        force -= t.vel * MASS * 1.3 * depth.min(1.0);
        ang_damp = 2.5;
        if grounded == 0 {
            let flat = Vec3::new(fwd.x, 0.0, fwd.z);
            force += flat * throttle * MASS * 5.0; // paddle-steamer mode
            t.angvel.y += steer_in * 1.2 * dt;
        }
        if !was_in_water && speed > 7.0 {
            fx.write(Fx::Splash { pos: t.pos });
        }
    }

    // ---- integrate velocities ---------------------------------------------
    let pre_vy = t.vel.y;
    t.vel += force / MASS * dt;
    let dw = t.inv_inertia(torque) * dt;
    t.angvel += dw;
    t.angvel *= 1.0 - (ang_damp * dt).min(1.0);
    t.angvel = t.angvel.clamp_length_max(11.0);

    // ---- body vs ground -----------------------------------------------------
    let mut max_pen = 0.0f32;
    let mut body_contact = false;
    let mut worst = 0.0f32;
    let h = BOX_H;
    let pts = [
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(-h.x, h.y, -h.z),
        Vec3::new(h.x, h.y, -h.z),
        Vec3::new(-h.x, h.y, h.z),
        Vec3::new(h.x, h.y, h.z),
        Vec3::new(0.0, -h.y, 0.0),
        Vec3::new(0.0, h.y, 0.0),
        Vec3::new(0.0, 0.0, -h.z),
        Vec3::new(0.0, 0.0, h.z),
    ];
    for lp in pts {
        let p = t.pos + t.rot * (lp + BOX_C);
        let g = world.ground(p.x, p.z);
        if p.y < g {
            let n = world.ground_normal(p.x, p.z);
            max_pen = max_pen.max((g - p.y) * n.y);
            body_contact = true;
            worst = worst.max(t.bounce_static(p, -n, 0.32, 0.45));
        }
    }
    if max_pen > 0.0 {
        t.pos.y += max_pen.min(0.6) * 0.7;
    }
    if worst > 7.0 {
        fx.write(Fx::Crash { pos: t.pos, strength: worst });
        t.shake = (t.shake + worst * 0.04).min(1.0);
    }

    // ---- trees and rocks ----------------------------------------------------
    let c = chunk_of(t.pos);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let Some(list) = obstacles.0.get_mut(&(c + IVec2::new(dx, dz))) else { continue };
            let mut i = 0;
            while i < list.len() {
                let ob = list[i].clone();
                let reach = ob.radius + ob.height + 4.5;
                if (ob.pos - t.pos).length_squared() > reach * reach {
                    i += 1;
                    continue;
                }
                let centre = match ob.kind {
                    ObKind::Rock => ob.pos,
                    ObKind::Tree => Vec3::new(ob.pos.x, (t.pos.y + 0.4).clamp(ob.pos.y, ob.pos.y + ob.height), ob.pos.z),
                };
                if let Some((p, n, pen)) = t.sphere_contact(centre, ob.radius) {
                    let closing = t.point_vel(p).dot(n);
                    if ob.breakable && closing > 8.0 {
                        let pv = t.point_vel(p);
                        t.apply_impulse(-n * MASS * (closing * 0.12).min(2.5), p);
                        t.shake = (t.shake + 0.25).min(1.0);
                        fx.write(Fx::TreeSmash { entity: ob.entity, pos: ob.pos, vel: pv });
                        list.swap_remove(i);
                        continue;
                    }
                    t.pos -= n * pen * 0.9;
                    body_contact = true;
                    let hit = t.bounce_static(p, n, if ob.kind == ObKind::Tree { 0.2 } else { 0.45 }, 0.25);
                    if hit > 5.0 {
                        fx.write(Fx::Crash { pos: p, strength: hit });
                        t.shake = (t.shake + hit * 0.05).min(1.0);
                    }
                }
                i += 1;
            }
        }
    }

    t.body_contact = body_contact;

    // ---- integrate pose -------------------------------------------------------
    t.pos += t.vel * dt;
    t.rot = (Quat::from_scaled_axis(t.angvel * dt) * t.rot).normalize();
    t.shake = (t.shake - dt * 1.6).max(0.0);

    // ---- air time + tricks ----------------------------------------------------
    if airborne && !t.in_water {
        t.air_time += dt;
        let local_w = t.rot.inverse() * t.angvel;
        t.spin_acc += local_w * dt;
    } else if t.air_time > 0.0 {
        if grounded >= 2 {
            let impact = (-pre_vy).max(0.0);
            if t.air_time > 0.45 {
                let turns = |a: f32| (a / std::f32::consts::TAU).round() as i32;
                fx.write(Fx::Landed {
                    air_time: t.air_time,
                    flips: -turns(t.spin_acc.x),
                    rolls: turns(t.spin_acc.z),
                    spins: turns(t.spin_acc.y),
                    impact,
                });
                t.shake = (t.shake + impact * 0.03).min(0.8);
            }
            t.air_time = 0.0;
            t.spin_acc = Vec3::ZERO;
        } else if body_contact || t.in_water {
            // Landed on the roof / in a lake: no trick bonus for you.
            t.air_time = 0.0;
            t.spin_acc = Vec3::ZERO;
        }
    }
    t.grounded = grounded;

    // ---- turtle recovery --------------------------------------------------------
    let turtled = t.up().y < 0.25 && speed < 4.0 && !airborne;
    // Also catches being high-centred on a rock with the wheels dangling.
    let wedged = playing && grounded < 2 && speed < 1.0 && !t.in_water;
    if turtled || wedged {
        t.upside_timer += dt * if turtled { 1.0 } else { 0.5 };
        if t.upside_timer > 1.4 {
            t.reset_upright(&world, &obstacles);
            fx.write(Fx::Flipped);
        }
    } else {
        t.upside_timer = 0.0;
    }
    // Never fall out of the world.
    if t.pos.y < world.ground(t.pos.x, t.pos.z) - 6.0 {
        t.reset_upright(&world, &obstacles);
    }
}

fn sync_truck(
    fixed: Res<Time<Fixed>>,
    mut truck: Query<(&Truck, &mut Transform), Without<WheelVisual>>,
    mut wheels: Query<(&WheelVisual, &mut Transform), Without<Truck>>,
) {
    let Ok((t, mut tf)) = truck.single_mut() else { return };
    let a = fixed.overstep_fraction();
    tf.translation = t.prev_pos.lerp(t.pos, a);
    tf.rotation = t.prev_rot.slerp(t.rot, a);
    for (wv, mut wtf) in &mut wheels {
        let w = t.wheels[wv.0];
        let steer = if wv.0 < 2 { t.steer } else { 0.0 };
        wtf.translation = tf.translation + tf.rotation * (HARD[wv.0] + Vec3::NEG_Y * w.len);
        // Spin is negated because forward is -Z.
        wtf.rotation = tf.rotation * Quat::from_rotation_y(steer) * Quat::from_rotation_x(-w.spin);
    }
}
