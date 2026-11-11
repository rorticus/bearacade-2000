//! Shared game state, events and the scoring rules carried over from the
//! original Bearacade (350 a bear, chain combos, MULTIBEAR / ULTRABEAR /
//! UNBEARLEVABLE, fuel as the clock).

use bevy::prelude::*;

pub const COMBO_TIMEOUT: f32 = 3.0;
pub const BEAR_SCORE: i64 = 350;
pub const FUEL_SCORE_COST: i64 = 8000;
pub const FUEL_SCORE_BOOST: f32 = 35.0;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameState {
    #[default]
    Title,
    Playing,
    GameOver,
}

/// Fixed-step ordering: truck first, then bears get hit, then loose bodies.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sim {
    Truck,
    Bears,
    Bodies,
}

#[derive(Resource, Clone)]
pub struct Config {
    pub seed: u32,
    /// Self-driving smoke test: drives, takes screenshots, exits.
    pub autopilot: bool,
    /// Autopilot variant: floor it in a straight line (terrain / jump test).
    pub straight: bool,
    /// Fuel in the tank at the start of a run (debug: `--fuel 10`).
    pub start_fuel: f32,
}

#[derive(Resource, Default)]
pub struct Game {
    pub score: i64,
    pub combo: u32,
    pub combo_timer: f32,
    pub fuel: f32,
    pub bears: u32,
    pub best_toss: f32,
    pub best_air: f32,
    pub best_combo: u32,
    pub muted: bool,
}

impl Game {
    pub fn reset(&mut self, fuel: f32) {
        let muted = self.muted;
        *self = Game { fuel, muted, ..default() };
    }
}

/// Things that happen in the simulation. Scoring, audio and particles all
/// listen to the same stream.
#[derive(Event, Clone, Debug)]
pub enum Fx {
    BearHit { pos: Vec3, vel: Vec3, juggle: bool },
    BearLanded { pos: Vec3, dist: f32 },
    Crash { pos: Vec3, strength: f32 },
    TreeSmash { entity: Entity, pos: Vec3, vel: Vec3 },
    BarrelHit { pos: Vec3, strength: f32 },
    Landed { air_time: f32, flips: i32, rolls: i32, spins: i32, impact: f32 },
    Splash { pos: Vec3 },
    FuelPickup { pos: Vec3 },
    BoostStart,
    Flipped,
    BoughtFuel,
    NoScore,
    Horn,
}

#[derive(Event, Clone)]
pub struct Popup {
    pub text: String,
    pub color: Color,
    pub size: f32,
}

impl Popup {
    pub fn new(text: impl Into<String>, color: Color, size: f32) -> Self {
        Self { text: text.into(), color, size }
    }
}

/// Announcer tiers: 0 = multibear, 1 = ultrabear, 2 = unbearlevable.
#[derive(Event, Clone, Copy)]
pub struct Announce(pub u8);

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .insert_resource(Game { fuel: 100.0, ..default() })
            .add_event::<Fx>()
            .add_event::<Popup>()
            .add_event::<Announce>()
            .configure_sets(FixedUpdate, (Sim::Truck, Sim::Bears, Sim::Bodies).chain())
            .add_systems(Update, (score_events, tick_game).run_if(in_state(GameState::Playing)));
    }
}

const GOLD: Color = Color::srgb(1.0, 0.85, 0.2);
const PINK: Color = Color::srgb(1.0, 0.35, 0.75);
const CYAN: Color = Color::srgb(0.4, 0.95, 1.0);
const WHITE: Color = Color::srgb(1.0, 1.0, 1.0);

fn score_events(mut game: ResMut<Game>, mut fx: EventReader<Fx>, mut popups: EventWriter<Popup>, mut announce: EventWriter<Announce>) {
    for ev in fx.read() {
        match ev {
            Fx::BearHit { juggle, .. } => {
                game.combo = if game.combo_timer > 0.0 { game.combo + 1 } else { 1 };
                game.combo_timer = COMBO_TIMEOUT;
                game.bears += 1;
                game.best_combo = game.best_combo.max(game.combo);
                let mut pts = BEAR_SCORE + (game.combo as i64 - 3).max(0) * 100;
                if *juggle {
                    pts *= 2;
                    popups.write(Popup::new(format!("AIR BEAR! +{pts}"), CYAN, 34.0));
                } else {
                    popups.write(Popup::new(format!("+{pts}"), WHITE, 28.0));
                }
                game.score += pts;
                game.fuel = (game.fuel + 2.0).min(100.0);
                match game.combo {
                    3 => {
                        announce.write(Announce(0));
                        popups.write(Popup::new("MULTIBEAR!", GOLD, 72.0));
                    }
                    6 => {
                        announce.write(Announce(1));
                        popups.write(Popup::new("ULTRABEAR!!", PINK, 84.0));
                    }
                    9 => {
                        announce.write(Announce(2));
                        popups.write(Popup::new("UNBEARLEVABLE!!!", CYAN, 96.0));
                    }
                    n if n > 9 && n % 3 == 0 => {
                        popups.write(Popup::new(format!("{n} BEAR CHAIN!"), PINK, 64.0));
                    }
                    _ => {}
                }
            }
            Fx::BearLanded { dist, .. } => {
                game.best_toss = game.best_toss.max(*dist);
                if *dist > 35.0 {
                    let pts = (*dist as i64) * 10;
                    game.score += pts;
                    popups.write(Popup::new(format!("BEAR TOSS {:.0}m  +{pts}", dist), GOLD, 30.0));
                }
            }
            Fx::Landed { air_time, flips, rolls, spins, .. } => {
                game.best_air = game.best_air.max(*air_time);
                let tricks = flips.abs() + rolls.abs() + spins.abs();
                if *air_time > 1.2 || tricks > 0 {
                    let mut pts = (*air_time * 150.0) as i64;
                    let mut label = format!("BIG AIR {:.1}s", air_time);
                    if *flips != 0 {
                        label = format!("{}{} FLIP", if flips.abs() > 1 { format!("{}x ", flips.abs()) } else { String::new() }, if *flips > 0 { "FRONT" } else { "BACK" });
                    } else if *rolls != 0 {
                        label = format!("{}BARREL ROLL", if rolls.abs() > 1 { format!("{}x ", rolls.abs()) } else { String::new() });
                    } else if *spins != 0 {
                        label = format!("{} SPIN", spins.abs() * 360);
                    }
                    pts += tricks as i64 * 1000;
                    game.score += pts;
                    popups.write(Popup::new(format!("{label}  +{pts}"), if tricks > 0 { PINK } else { WHITE }, if tricks > 0 { 48.0 } else { 30.0 }));
                }
            }
            Fx::TreeSmash { .. } => {
                game.score += 50;
                popups.write(Popup::new("TIMBER +50", Color::srgb(0.6, 0.95, 0.5), 24.0));
            }
            Fx::BarrelHit { strength, .. } if *strength > 6.0 => {
                game.score += 25;
            }
            Fx::FuelPickup { .. } => {
                game.fuel = (game.fuel + 25.0).min(100.0);
                popups.write(Popup::new("FUEL +25", Color::srgb(1.0, 0.5, 0.3), 34.0));
            }
            _ => {}
        }
    }
}

fn tick_game(time: Res<Time>, mut game: ResMut<Game>) {
    if game.combo_timer > 0.0 {
        game.combo_timer -= time.delta_secs();
        if game.combo_timer <= 0.0 {
            game.combo = 0;
        }
    }
}
