//! Round & match flow: party scoring over a lobby-chosen number of rounds.
//!
//! Each round is 1 life per player, last car running wins. Points per round:
//! one per opponent you outlived, plus a win bonus for the survivor. After
//! the final round a match banner shows and everyone returns to the lobby.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::cop::{self, CopAssets, CopCar};
use crate::lobby::{GameState, MatchConfig, NAMES};
use crate::vehicle::{
    self, Car, CarAssets, Health, HealthBar, InputSource, Player, Roster, PLAYER_COLORS,
};
use crate::weapon::{Lifetime, Projectile};

const RESET_SECONDS: f32 = 3.0;
/// Longer linger on the final banner so the match result sinks in.
const MATCH_END_SECONDS: f32 = 6.0;
const DEBRIS_PIECES: usize = 10;

#[derive(Resource, Default)]
enum RoundPhase {
    #[default]
    Active,
    /// Round decided; respawn everyone when the countdown expires.
    Over {
        countdown: f32,
    },
    /// Final round decided; back to the lobby when the countdown expires.
    MatchOver {
        countdown: f32,
    },
}

/// Progress through the current match.
#[derive(Resource)]
struct MatchState {
    round: u32,
    /// Players eliminated so far this round (drives outlive points).
    eliminated: usize,
}

impl Default for MatchState {
    fn default() -> Self {
        Self {
            round: 1,
            eliminated: 0,
        }
    }
}

/// Marker for the round-status UI text.
#[derive(Component)]
struct Banner;

/// Root node of the in-game score HUD (top-right).
#[derive(Component)]
struct Scoreboard;

/// One rebuilt-on-change line inside the scoreboard.
#[derive(Component)]
struct ScoreRow;

pub struct RoundPlugin;

impl Plugin for RoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RoundPhase>()
            .init_resource::<MatchState>()
            .add_systems(Startup, spawn_banner)
            .add_systems(OnEnter(GameState::InGame), (enter_game, spawn_scoreboard))
            .add_systems(OnExit(GameState::InGame), cleanup_game)
            .add_systems(
                Update,
                (
                    eliminate_dead_cars,
                    watch_for_winner,
                    restart_on_key,
                    reset_round,
                    refresh_scoreboard,
                )
                    .chain()
                    .run_if(in_state(GameState::InGame)),
            );
    }
}

/// Entering from the lobby: fresh scores, round 1 for the current roster.
fn enter_game(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
    mut state: ResMut<MatchState>,
    assets: Res<CarAssets>,
    cop_assets: Res<CopAssets>,
    mut roster: ResMut<Roster>,
    leftovers: Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: Single<&mut Text, With<Banner>>,
) {
    for slot in &mut roster.players {
        slot.score = 0;
    }
    state.round = 1;
    restart_round(
        &mut commands,
        &time,
        &mut phase,
        &mut state,
        &assets,
        &cop_assets,
        &roster,
        &leftovers,
        &mut *banner.into_inner(),
    );
}

/// Returning to the lobby: clear the battlefield (bars self-clean via their
/// car check, but sweep them too in case their car is already gone).
fn cleanup_game(
    mut commands: Commands,
    mut phase: ResMut<RoundPhase>,
    leftovers: Query<
        Entity,
        Or<(
            With<Car>,
            With<Projectile>,
            With<CopCar>,
            With<Lifetime>,
            With<HealthBar>,
        )>,
    >,
    banner: Single<&mut Text, With<Banner>>,
) {
    for entity in &leftovers {
        commands.entity(entity).try_despawn();
    }
    banner.into_inner().0 = String::new();
    *phase = RoundPhase::Active;
}

fn spawn_banner(mut commands: Commands) {
    commands.spawn((
        Banner,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(40.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(24.0),
            left: Val::Px(24.0),
            ..default()
        },
    ));
}

/// Despawn cars at zero health, leaving a burst of body-colored debris.
/// Dying scores you one point per player eliminated before you (deaths in
/// the same frame count as simultaneous, so they tie).
fn eliminate_dead_cars(
    mut commands: Commands,
    assets: Res<CarAssets>,
    mut sfx: MessageWriter<PlaySfx>,
    mut roster: ResMut<Roster>,
    mut state: ResMut<MatchState>,
    cars: Query<(Entity, &Health, &Player, &Transform), With<Car>>,
) {
    let outlived = state.eliminated;
    let mut deaths = 0;
    for (entity, health, player, transform) in &cars {
        if health.current > 0.0 {
            continue;
        }
        info!("Player {} was wrecked!", player.id + 1);
        sfx.write(PlaySfx {
            kind: SfxKind::Wreck,
            position: Some(transform.translation),
        });
        deaths += 1;
        if let Some(slot) = roster.players.iter_mut().find(|s| s.id == player.id) {
            slot.score += outlived as u32;
        }
        commands.entity(entity).try_despawn();

        // Debris burst: deterministic golden-angle spread, no rand needed.
        let body = assets.body_materials[player.color % PLAYER_COLORS.len()].clone();
        for i in 0..DEBRIS_PIECES {
            let angle = i as f32 * 2.399963 + player.id as f32;
            let speed = 3.0 + (i % 3) as f32 * 2.0;
            let velocity = Vec3::new(
                angle.cos() * speed,
                4.0 + (i % 4) as f32 * 1.5,
                angle.sin() * speed,
            );
            commands.spawn((
                Name::new("Debris"),
                Mesh3d(assets.debris.clone()),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(transform.translation + Vec3::Y * 0.3)
                    .with_rotation(Quat::from_rotation_y(angle)),
                RigidBody::Dynamic,
                Collider::cuboid(0.3, 0.3, 0.3),
                Mass(0.5),
                LinearVelocity(velocity),
                AngularVelocity(Vec3::new(angle.sin(), angle.cos(), 1.0) * 3.0),
                Lifetime(2.5),
            ));
        }
    }
    if deaths > 0 {
        state.eliminated = outlived + deaths;
    }
}

/// With 2+ players in the session, end the round when at most one car runs;
/// after the final round, end the match instead.
fn watch_for_winner(
    mut phase: ResMut<RoundPhase>,
    mut roster: ResMut<Roster>,
    config: Res<MatchConfig>,
    state: Res<MatchState>,
    cars: Query<&Player, With<Car>>,
    mut sfx: MessageWriter<PlaySfx>,
    banner: Single<(&mut Text, &mut TextColor), With<Banner>>,
) {
    if !matches!(*phase, RoundPhase::Active) || roster.players.len() < 2 {
        return;
    }
    let alive: Vec<&Player> = cars.iter().collect();
    if alive.len() > 1 {
        return;
    }

    let (mut text, mut color) = banner.into_inner();
    let round_line = match alive.first() {
        Some(player) => {
            // Winner outlived everyone (players - 1 points) plus 1 for the win.
            let bonus = roster.players.len() as u32;
            let name = roster
                .players
                .iter_mut()
                .find(|slot| slot.id == player.id)
                .map(|slot| {
                    slot.score += bonus;
                    NAMES[slot.name_index % NAMES.len()]
                })
                .unwrap_or("Player ?");
            color.0 = PLAYER_COLORS[player.color % PLAYER_COLORS.len()];
            info!("{name} wins the round!");
            sfx.write(PlaySfx {
                kind: SfxKind::RoundWin,
                position: None,
            });
            format!("{name} wins the round!")
        }
        None => {
            color.0 = Color::WHITE;
            info!("Round ends in a draw");
            "Everyone's wrecked — draw!".to_string()
        }
    };

    if state.round < config.rounds {
        text.0 = round_line;
        *phase = RoundPhase::Over {
            countdown: RESET_SECONDS,
        };
        return;
    }

    // That was the final round: crown the match champion (or tie).
    let top = roster.players.iter().map(|s| s.score).max().unwrap_or(0);
    let champions: Vec<_> = roster
        .players
        .iter()
        .filter(|s| s.score == top)
        .collect();
    if let [champion] = champions[..] {
        let name = NAMES[champion.name_index % NAMES.len()];
        text.0 = format!("{round_line}\n{name} WINS THE MATCH!");
        color.0 = PLAYER_COLORS[champion.color_index % PLAYER_COLORS.len()];
        info!("{name} wins the match with {top} points!");
    } else {
        text.0 = format!("{round_line}\nMatch tied at {top} points!");
        color.0 = Color::WHITE;
        info!("Match ends in a {top}-point tie");
    }
    *phase = RoundPhase::MatchOver {
        countdown: MATCH_END_SECONDS,
    };
}

/// Press R to restart the round immediately (dev / playtest shortcut).
fn restart_on_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
    mut state: ResMut<MatchState>,
    assets: Res<CarAssets>,
    cop_assets: Res<CopAssets>,
    roster: Res<Roster>,
    leftovers: Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: Single<&mut Text, With<Banner>>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    restart_round(
        &mut commands,
        &time,
        &mut phase,
        &mut state,
        &assets,
        &cop_assets,
        &roster,
        &leftovers,
        &mut *banner.into_inner(),
    );
}

/// After the banner pause: next round, or back to the lobby at match end.
fn reset_round(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
    mut state: ResMut<MatchState>,
    mut next: ResMut<NextState<GameState>>,
    assets: Res<CarAssets>,
    cop_assets: Res<CopAssets>,
    roster: Res<Roster>,
    leftovers: Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: Single<&mut Text, With<Banner>>,
) {
    match &mut *phase {
        RoundPhase::Active => {}
        RoundPhase::Over { countdown } => {
            *countdown -= time.delta_secs();
            if *countdown > 0.0 {
                return;
            }
            state.round += 1;
            restart_round(
                &mut commands,
                &time,
                &mut phase,
                &mut state,
                &assets,
                &cop_assets,
                &roster,
                &leftovers,
                &mut *banner.into_inner(),
            );
        }
        RoundPhase::MatchOver { countdown } => {
            *countdown -= time.delta_secs();
            if *countdown > 0.0 {
                return;
            }
            *phase = RoundPhase::Active;
            next.set(GameState::Lobby);
        }
    }
}

/// Despawn round entities and respawn every roster player plus one cop.
fn restart_round(
    commands: &mut Commands,
    time: &Time,
    phase: &mut RoundPhase,
    state: &mut MatchState,
    assets: &CarAssets,
    cop_assets: &CopAssets,
    roster: &Roster,
    leftovers: &Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: &mut Text,
) {
    for entity in leftovers {
        commands.entity(entity).try_despawn();
    }
    for (position, slot) in roster.players.iter().enumerate() {
        vehicle::spawn_car(commands, assets, slot, position);
    }
    let pos = cop::pick_spawn_point(time.elapsed_secs(), 0);
    cop::spawn_cop(commands, cop_assets, assets, pos);
    banner.0 = String::new();
    state.eliminated = 0;
    *phase = RoundPhase::Active;
}

// --- Score HUD ---

fn spawn_scoreboard(mut commands: Commands) {
    commands.spawn((
        Name::new("Scoreboard"),
        Scoreboard,
        DespawnOnExit(GameState::InGame),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(24.0),
            right: Val::Px(24.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(2.0),
            padding: UiRect::all(Val::Px(10.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
    ));
}

/// Rebuild the score rows whenever scores or the round number change.
fn refresh_scoreboard(
    mut commands: Commands,
    roster: Res<Roster>,
    state: Res<MatchState>,
    config: Res<MatchConfig>,
    board: Single<Entity, With<Scoreboard>>,
    rows: Query<Entity, With<ScoreRow>>,
) {
    if !roster.is_changed() && !state.is_changed() {
        return;
    }
    for row in &rows {
        commands.entity(row).try_despawn();
    }
    let mut slots: Vec<_> = roster.players.iter().collect();
    slots.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
    commands.entity(*board).with_children(|parent| {
        parent.spawn((
            ScoreRow,
            Text::new(format!("Round {}/{}", state.round, config.rounds)),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
        for slot in slots {
            let name = NAMES[slot.name_index % NAMES.len()];
            let cpu = if matches!(slot.source, InputSource::Cpu) {
                " (CPU)"
            } else {
                ""
            };
            parent.spawn((
                ScoreRow,
                Text::new(format!("{name}{cpu}  {}", slot.score)),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(PLAYER_COLORS[slot.color_index % PLAYER_COLORS.len()]),
            ));
        }
    });
}
