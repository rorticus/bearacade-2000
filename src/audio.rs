//! Sound: the original Bearacade announcer and bear thump, plus new engine,
//! impacts and music.

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::game::{Announce, Fx, Game, GameState};
use crate::truck::Truck;

#[derive(Resource)]
struct Sounds {
    bear_hit: Handle<AudioSource>,
    bear_yelp: Handle<AudioSource>,
    crash: Handle<AudioSource>,
    crunch: Handle<AudioSource>,
    land: Handle<AudioSource>,
    boost: Handle<AudioSource>,
    barrel: Handle<AudioSource>,
    fuel: Handle<AudioSource>,
    horn: Handle<AudioSource>,
    noscore: Handle<AudioSource>,
    usefuel: Handle<AudioSource>,
    announcer: [Handle<AudioSource>; 3],
}

#[derive(Component)]
struct EngineSound;

#[derive(Component)]
struct Music;

#[derive(Resource, Default)]
struct Cooldowns {
    crash: f32,
    barrel: f32,
    yelp: f32,
    phase: f32,
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cooldowns>().add_systems(Startup, setup).add_systems(Update, (engine, one_shots, mute_toggle));
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let l = |name: &str| assets.load::<AudioSource>(format!("audio/{name}"));
    commands.insert_resource(Sounds {
        bear_hit: l("bear_hit.mp3"),
        bear_yelp: l("bear_yelp.mp3"),
        crash: l("crash.mp3"),
        crunch: l("crunch.mp3"),
        land: l("land.mp3"),
        boost: l("boost.mp3"),
        barrel: l("barrel.mp3"),
        fuel: l("fuel.mp3"),
        horn: l("horn.mp3"),
        noscore: l("noscore.mp3"),
        usefuel: l("usefuel.mp3"),
        announcer: [l("multibear.mp3"), l("ultrabear.mp3"), l("unbearlevable.mp3")],
    });
    commands.spawn((EngineSound, AudioPlayer::new(l("engine.wav")), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0))));
    commands.spawn((Music, AudioPlayer::new(l("music.ogg")), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.32))));
}

fn engine(time: Res<Time>, state: Res<State<GameState>>, truck: Query<&Truck>, mut sink: Query<&mut AudioSink, With<EngineSound>>, mut rev: Local<f32>) {
    let (Ok(t), Ok(mut sink)) = (truck.single(), sink.single_mut()) else { return };
    let playing = *state.get() == GameState::Playing;
    // Fake gearbox: revs climb within each "gear" band.
    let speed = t.speed();
    let gear_pos = (speed / 14.0).fract();
    let mut target = 0.55 + gear_pos * 0.55 + (speed / 60.0).min(1.0) * 0.5 + t.throttle.abs() * 0.18;
    if t.grounded == 0 && t.throttle > 0.0 {
        target += 0.35; // free-revving in the air
    }
    if t.boosting {
        target += 0.25;
    }
    if !playing {
        target = 0.5;
    }
    *rev += (target - *rev) * (time.delta_secs() * 6.0).min(1.0);
    sink.set_speed(rev.clamp(0.4, 2.4));
    let vol = if playing { 0.2 + t.throttle.abs() * 0.12 } else { 0.1 };
    sink.set_volume(Volume::Linear(vol));
}

fn play(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((AudioPlayer::new(sound.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)).with_speed(speed)));
}

fn one_shots(
    time: Res<Time>,
    mut commands: Commands,
    sounds: Res<Sounds>,
    mut cd: ResMut<Cooldowns>,
    mut fx: EventReader<Fx>,
    mut announce: EventReader<Announce>,
) {
    let dt = time.delta_secs();
    cd.crash -= dt;
    cd.barrel -= dt;
    cd.yelp -= dt;
    cd.phase = (cd.phase + 0.37) % 1.0;
    let wobble = 0.85 + cd.phase * 0.4;
    for ev in fx.read() {
        match ev {
            Fx::BearHit { .. } => {
                play(&mut commands, &sounds.bear_hit, 0.9, wobble);
                if cd.yelp <= 0.0 {
                    cd.yelp = 0.25;
                    play(&mut commands, &sounds.bear_yelp, 0.55, 0.8 + cd.phase * 0.6);
                }
            }
            Fx::Crash { strength, .. } if cd.crash <= 0.0 => {
                cd.crash = 0.35;
                let v = (*strength / 25.0).clamp(0.25, 0.9);
                play(&mut commands, if *strength > 16.0 { &sounds.crash } else { &sounds.crunch }, v, wobble);
            }
            Fx::TreeSmash { .. } => play(&mut commands, &sounds.crunch, 0.8, 0.9 + cd.phase * 0.3),
            Fx::BarrelHit { strength, .. } if cd.barrel <= 0.0 && *strength > 2.0 => {
                cd.barrel = 0.12;
                play(&mut commands, &sounds.barrel, (*strength / 20.0).clamp(0.2, 0.8), wobble);
            }
            Fx::Landed { impact, .. } if *impact > 5.0 => play(&mut commands, &sounds.land, (*impact / 22.0).clamp(0.3, 1.0), wobble),
            Fx::BearLanded { .. } if cd.barrel <= 0.0 => {
                cd.barrel = 0.1;
                play(&mut commands, &sounds.land, 0.25, 1.5);
            }
            Fx::Splash { .. } => play(&mut commands, &sounds.land, 0.6, 0.6),
            Fx::FuelPickup { .. } => play(&mut commands, &sounds.fuel, 0.9, 1.0),
            Fx::BoostStart => play(&mut commands, &sounds.boost, 0.6, 1.0),
            Fx::BoughtFuel => play(&mut commands, &sounds.usefuel, 1.0, 1.0),
            Fx::NoScore => play(&mut commands, &sounds.noscore, 1.0, 1.0),
            Fx::Horn => play(&mut commands, &sounds.horn, 0.8, 1.0),
            _ => {}
        }
    }
    for a in announce.read() {
        play(&mut commands, &sounds.announcer[a.0 as usize], 1.0, 1.0);
    }
}

fn mute_toggle(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>, mut volume: ResMut<GlobalVolume>) {
    if keys.just_pressed(KeyCode::KeyM) {
        game.muted = !game.muted;
        volume.volume = Volume::Linear(if game.muted { 0.0 } else { 1.0 });
    }
}
