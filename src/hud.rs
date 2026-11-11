//! HUD, title / game-over screens and the fuel clock.

use bevy::prelude::*;

use crate::game::{Config, Fx, Game, GameState, Popup, FUEL_SCORE_BOOST, FUEL_SCORE_COST};
use crate::truck::Truck;

#[derive(Component)]
struct ScoreText;
#[derive(Component)]
struct ComboText;
#[derive(Component)]
struct SpeedText;
#[derive(Component)]
struct FuelFill;
#[derive(Component)]
struct FuelHint;
#[derive(Component)]
struct PopupStack;
#[derive(Component)]
struct PopupItem {
    life: f32,
    color: Color,
}
#[derive(Component)]
struct TitleScreen;
#[derive(Component)]
struct GameOverScreen;
#[derive(Component)]
struct GameOverStats;
#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct Blink;

const GREEN: Color = Color::srgb(0.35, 1.0, 0.3);
const PINK: Color = Color::srgb(1.0, 0.3, 0.75);
const YELLOW: Color = Color::srgb(1.0, 0.9, 0.2);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (update_hud, popups, blink, screens))
            .add_systems(Update, (fuel_clock, buy_fuel).run_if(in_state(GameState::Playing)))
            .add_systems(Update, start_input.run_if(not(in_state(GameState::Playing))));
    }
}

fn text(s: &str, size: f32, color: Color) -> (Text, TextFont, TextColor, TextShadow) {
    (
        Text::new(s),
        TextFont { font_size: size, ..default() },
        TextColor(color),
        TextShadow { offset: Vec2::splat((size / 14.0).max(2.0)), color: Color::srgba(0.0, 0.0, 0.0, 0.75) },
    )
}

fn setup(mut commands: Commands, config: Res<Config>) {
    // In-game HUD
    commands
        .spawn((HudRoot, Node { width: Val::Percent(100.0), height: Val::Percent(100.0), position_type: PositionType::Absolute, ..default() }, Visibility::Hidden))
        .with_children(|root| {
            root.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(24.0), top: Val::Px(16.0), flex_direction: FlexDirection::Column, ..default() })
                .with_children(|p| {
                    p.spawn((ScoreText, text("0", 54.0, Color::WHITE)));
                    p.spawn((ComboText, text("", 28.0, YELLOW)));
                });
            root.spawn(Node {
                position_type: PositionType::Absolute,
                right: Val::Px(24.0),
                top: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|p| {
                p.spawn(text("FUEL", 22.0, Color::WHITE));
                p.spawn((
                    Node { width: Val::Px(280.0), height: Val::Px(26.0), border: UiRect::all(Val::Px(3.0)), ..default() },
                    BorderColor(Color::WHITE),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                ))
                .with_children(|bar| {
                    bar.spawn((FuelFill, Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(GREEN)));
                });
                p.spawn((FuelHint, text("", 16.0, YELLOW)));
            });
            root.spawn(Node { position_type: PositionType::Absolute, right: Val::Px(24.0), bottom: Val::Px(16.0), ..default() })
                .with_children(|p| {
                    p.spawn((SpeedText, text("0 km/h", 40.0, Color::WHITE)));
                });
            root.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(24.0), bottom: Val::Px(14.0), ..default() }).with_children(|p| {
                p.spawn(text(
                    "WASD drive   SPACE handbrake   SHIFT boost   Q/E roll   R flip   F buy fuel   H horn   C look back   M mute",
                    14.0,
                    Color::srgba(1.0, 1.0, 1.0, 0.7),
                ));
            });
            root.spawn((
                PopupStack,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Percent(14.0),
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::ColumnReverse,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
        });

    // Title
    commands
        .spawn((
            TitleScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.0, 0.15, 0.45)),
        ))
        .with_children(|p| {
            p.spawn(text("BEARACADE", 120.0, GREEN));
            p.spawn(text("2 0 0 0", 64.0, PINK));
            p.spawn(Node { height: Val::Px(18.0), ..default() });
            p.spawn(text("AN OPEN WORLD FULL OF BEARS. YOU HAVE A TRUCK.", 24.0, YELLOW));
            p.spawn(text("Hit as many bears as you can before you run out of fuel.", 20.0, Color::WHITE));
            p.spawn(text("Chain hits for MULTIBEAR combos. Big air and flips score too.", 20.0, Color::WHITE));
            p.spawn(Node { height: Val::Px(10.0), ..default() });
            p.spawn(text("WASD / arrows: drive    SPACE: handbrake    SHIFT: boost", 18.0, Color::srgba(1.0, 1.0, 1.0, 0.85)));
            p.spawn(text("In the air: W/S pitch, A/D spin, Q/E roll    R: flip upright    F: trade 8000 points for fuel", 18.0, Color::srgba(1.0, 1.0, 1.0, 0.85)));
            p.spawn(Node { height: Val::Px(18.0), ..default() });
            p.spawn((Blink, text("PRESS ENTER TO START", 36.0, YELLOW)));
            p.spawn(text(&format!("world seed {}", config.seed), 14.0, Color::srgba(1.0, 1.0, 1.0, 0.6)));
        });

    // Game over
    commands
        .spawn((
            GameOverScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.0, 0.15, 0.6)),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn(text("OUT OF FUEL", 96.0, PINK));
            p.spawn((GameOverStats, text("", 28.0, Color::WHITE), TextLayout::new_with_justify(JustifyText::Center)));
            p.spawn((Blink, text("PRESS ENTER TO GO AGAIN", 32.0, YELLOW)));
        });
}

fn update_hud(
    game: Res<Game>,
    truck: Query<&Truck>,
    mut score: Query<&mut Text, (With<ScoreText>, Without<ComboText>, Without<SpeedText>, Without<FuelHint>)>,
    mut combo: Query<&mut Text, (With<ComboText>, Without<SpeedText>, Without<FuelHint>)>,
    mut speed: Query<&mut Text, (With<SpeedText>, Without<FuelHint>)>,
    mut hint: Query<&mut Text, With<FuelHint>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), With<FuelFill>>,
) {
    let Ok(t) = truck.single() else { return };
    if let Ok(mut s) = score.single_mut() {
        s.0 = format!("{:06}", game.score);
    }
    if let Ok(mut c) = combo.single_mut() {
        c.0 = if game.combo >= 2 { format!("{} BEAR COMBO", game.combo) } else { format!("bears: {}", game.bears) };
    }
    if let Ok(mut s) = speed.single_mut() {
        s.0 = format!("{:.0} km/h", t.speed() * 3.6);
    }
    if let Ok(mut h) = hint.single_mut() {
        h.0 = if game.fuel < 35.0 && game.score >= FUEL_SCORE_COST { "PRESS F: BUY FUEL".into() } else { String::new() };
    }
    if let Ok((mut node, mut color)) = fill.single_mut() {
        node.width = Val::Percent(game.fuel.clamp(0.0, 100.0));
        color.0 = if game.fuel < 20.0 { Color::srgb(1.0, 0.2, 0.15) } else if game.fuel < 45.0 { YELLOW } else { GREEN };
    }
}

fn popups(
    time: Res<Time>,
    mut commands: Commands,
    mut events: EventReader<Popup>,
    stack: Query<(Entity, Option<&Children>), With<PopupStack>>,
    mut items: Query<(Entity, &mut PopupItem, &mut TextColor)>,
) {
    let Ok((stack, children)) = stack.single() else { return };
    let mut count = children.map(|c| c.len()).unwrap_or(0);
    for ev in events.read() {
        if count >= 7 {
            if let Some(oldest) = children.and_then(|c| c.first()) {
                commands.entity(*oldest).despawn();
            }
        }
        count += 1;
        let life = if ev.size > 60.0 { 2.4 } else { 1.6 };
        let child = commands.spawn((PopupItem { life, color: ev.color }, text(&ev.text, ev.size, ev.color))).id();
        commands.entity(stack).add_child(child);
    }
    for (e, mut item, mut color) in &mut items {
        item.life -= time.delta_secs();
        if item.life <= 0.0 {
            commands.entity(e).despawn();
        } else {
            color.0 = item.color.with_alpha((item.life * 2.5).min(1.0));
        }
    }
}

fn blink(time: Res<Time>, mut q: Query<&mut TextColor, With<Blink>>) {
    let a = 0.55 + 0.45 * (time.elapsed_secs() * 5.0).sin();
    for mut c in &mut q {
        c.0 = YELLOW.with_alpha(a);
    }
}

fn screens(
    state: Res<State<GameState>>,
    game: Res<Game>,
    mut hud: Query<&mut Visibility, (With<HudRoot>, Without<TitleScreen>, Without<GameOverScreen>)>,
    mut title: Query<&mut Visibility, (With<TitleScreen>, Without<GameOverScreen>)>,
    mut over: Query<&mut Visibility, With<GameOverScreen>>,
    mut stats: Query<&mut Text, With<GameOverStats>>,
) {
    let s = *state.get();
    let vis = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    if let Ok(mut v) = hud.single_mut() {
        *v = vis(s != GameState::Title);
    }
    if let Ok(mut v) = title.single_mut() {
        *v = vis(s == GameState::Title);
    }
    if let Ok(mut v) = over.single_mut() {
        *v = vis(s == GameState::GameOver);
    }
    if s == GameState::GameOver {
        if let Ok(mut t) = stats.single_mut() {
            t.0 = format!(
                "SCORE {}\n\nbears launched: {}     best chain: {}\nlongest bear toss: {:.0} m     biggest air: {:.1} s",
                game.score, game.bears, game.best_combo, game.best_toss, game.best_air
            );
        }
    }
}

fn fuel_clock(time: Res<Time>, mut game: ResMut<Game>, truck: Query<&Truck>, mut next: ResMut<NextState<GameState>>, mut dry: Local<f32>) {
    let Ok(t) = truck.single() else { return };
    let dt = time.delta_secs();
    let burn = 0.5 + t.throttle.abs() * 0.7 + if t.boosting { 2.3 } else { 0.0 };
    game.fuel = (game.fuel - burn * dt).max(0.0);
    if game.fuel <= 0.0 {
        // Let the truck coast (and any airborne bears land) before the curtain.
        *dry += dt;
        if *dry > 4.0 || (t.speed() < 2.0 && *dry > 1.0) {
            *dry = 0.0;
            next.set(GameState::GameOver);
        }
    } else {
        *dry = 0.0;
    }
}

fn buy_fuel(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>, mut fx: EventWriter<Fx>, mut popups: EventWriter<Popup>) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    if game.score >= FUEL_SCORE_COST {
        game.score -= FUEL_SCORE_COST;
        game.fuel = (game.fuel + FUEL_SCORE_BOOST).min(100.0);
        fx.write(Fx::BoughtFuel);
        popups.write(Popup::new(format!("FUEL BOUGHT  -{FUEL_SCORE_COST}"), YELLOW, 30.0));
    } else {
        fx.write(Fx::NoScore);
        popups.write(Popup::new(format!("NEED {FUEL_SCORE_COST} POINTS"), Color::srgb(1.0, 0.3, 0.3), 26.0));
    }
}

fn start_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<Config>,
    state: Res<State<GameState>>,
    mut game: ResMut<Game>,
    mut truck: Query<&mut Truck>,
    mut next: ResMut<NextState<GameState>>,
) {
    let auto = config.autopilot && time.elapsed_secs() > 1.5 && *state.get() == GameState::Title;
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) || auto {
        game.reset(config.start_fuel);
        if let Ok(mut t) = truck.single_mut() {
            t.reset_request = true;
        }
        next.set(GameState::Playing);
    }
}
