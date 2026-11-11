//! BEARACADE 2000 - an open-world, funny-physics take on Bearacade.

mod audio;
mod bears;
mod camera;
mod fx;
mod game;
mod hud;
mod meshgen;
mod noise;
mod props;
mod terrain;
mod truck;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use game::Config;

fn main() {
    let args = launch_args();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let seed = arg("--seed").and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        (now_secs() % 100_000) as u32
    });
    let autopilot = args.iter().any(|a| a == "--autopilot" || a == "--autopilot-straight");
    let straight = args.iter().any(|a| a == "--autopilot-straight");
    println!("BEARACADE 2000 - world seed {seed} (replay it with --seed {seed})");
    let shots_dir = arg("--shots").unwrap_or_else(|| "shots".into());

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "BEARACADE 2000".into(),
                        resolution: (1280.0_f32, 720.0_f32).into(),
                        // Web: render into the page's canvas and track its size.
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                })
                // No .meta files are shipped; do not 404 looking for them.
                .set(AssetPlugin { meta_check: bevy::asset::AssetMetaCheck::Never, ..default() }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(Time::<Fixed>::from_hz(120.0))
        .insert_resource(ShotsDir(shots_dir))
        .insert_resource(Config { seed, autopilot, straight, start_fuel: arg("--fuel").and_then(|s| s.parse().ok()).unwrap_or(100.0) })
        .insert_resource(terrain::World::new(seed))
        .add_plugins((
            game::GamePlugin,
            terrain::TerrainPlugin,
            truck::TruckPlugin,
            props::PropsPlugin,
            bears::BearsPlugin,
            camera::CameraPlugin,
            fx::FxPlugin,
            audio::AudioPlugin,
            hud::HudPlugin,
        ))
        .add_systems(Update, (screenshot_key, autopilot_shots, page_ready))
        .run();
}

#[derive(Resource)]
struct ShotsDir(String);

/// Native: the process arguments. Web: the page's query string, so
/// `?seed=42&autopilot` behaves like `--seed 42 --autopilot`.
#[cfg(not(target_arch = "wasm32"))]
fn launch_args() -> Vec<String> {
    std::env::args().collect()
}

#[cfg(target_arch = "wasm32")]
fn launch_args() -> Vec<String> {
    let query = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    let mut args = vec!["bearacade2000".to_string()];
    for pair in query.trim_start_matches('?').split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        args.push(format!("--{key}"));
        if !value.is_empty() {
            args.push(value.to_string());
        }
    }
    args
}

#[cfg(not(target_arch = "wasm32"))]
fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(2000)
}

#[cfg(target_arch = "wasm32")]
fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Web: tell the host page the world is on screen so it can drop its splash.
fn page_ready(mut frames: Local<u32>) {
    *frames += 1;
    #[cfg(target_arch = "wasm32")]
    if *frames == 4 {
        if let Some(w) = web_sys::window() {
            let _ = js_sys::Reflect::set(&w, &"__bearacadeReady".into(), &true.into());
        }
    }
}

fn screenshot_key(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>, mut n: Local<u32>) {
    if keys.just_pressed(KeyCode::F12) && cfg!(not(target_arch = "wasm32")) {
        *n += 1;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("screenshot_{:03}.png", *n)));
    }
}

/// `--autopilot [--shots dir]`: drive for a while, save screenshots, quit.
fn autopilot_shots(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<Config>,
    shots: Res<ShotsDir>,
    game: Res<game::Game>,
    truck: Query<&truck::Truck>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: EventWriter<AppExit>,
    mut next: Local<u32>,
) {
    if !config.autopilot {
        return;
    }
    let times = [0.8, 4.0, 7.0, 10.0, 13.0, 16.0, 19.0, 22.0];
    let t = time.elapsed_secs();
    if (*next as usize) < times.len() && t > times[*next as usize] {
        if cfg!(not(target_arch = "wasm32")) {
            let dir = &shots.0;
            let _ = std::fs::create_dir_all(dir);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("{dir}/shot_{}.png", *next)));
        }
        if let Ok(tr) = truck.single() {
            info!(
                "[autopilot t={t:.1}] pos=({:.0},{:.1},{:.0}) speed={:.1} grounded={} score={} bears={} fuel={:.0} best_air={:.1}s fps={:.0}",
                tr.pos.x, tr.pos.y, tr.pos.z, tr.speed(), tr.grounded, game.score, game.bears, game.fuel, game.best_air,
                diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0)
            );
        }
        *next += 1;
    }
    if t > times[times.len() - 1] + 1.0 && cfg!(not(target_arch = "wasm32")) {
        exit.write(AppExit::Success);
    }
}
