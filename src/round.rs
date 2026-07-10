//! Round flow for M2: 1 life per player, last car running wins, then the
//! round resets after a short banner (design §4-5; level rotation lands in M5).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::cop::{self, CopAssets, CopCar};
use crate::lobby::{GameState, NAMES};
use crate::vehicle::{self, Car, CarAssets, Health, HealthBar, Player, Roster, PLAYER_COLORS};
use crate::weapon::{Lifetime, Projectile};

const RESET_SECONDS: f32 = 3.0;
const DEBRIS_PIECES: usize = 10;

#[derive(Resource, Default)]
enum RoundPhase {
    #[default]
    Active,
    Over {
        countdown: f32,
    },
}

/// Marker for the round-status UI text.
#[derive(Component)]
struct Banner;

pub struct RoundPlugin;

impl Plugin for RoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RoundPhase>()
            .add_systems(Startup, spawn_banner)
            .add_systems(OnEnter(GameState::InGame), enter_game)
            .add_systems(OnExit(GameState::InGame), cleanup_game)
            .add_systems(
                Update,
                (
                    eliminate_dead_cars,
                    watch_for_winner,
                    restart_on_key,
                    reset_round,
                )
                    .chain()
                    .run_if(in_state(GameState::InGame)),
            );
    }
}

/// Entering from the lobby: start the first round for the current roster.
fn enter_game(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
    assets: Res<CarAssets>,
    cop_assets: Res<CopAssets>,
    roster: Res<Roster>,
    leftovers: Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: Single<&mut Text, With<Banner>>,
) {
    restart_round(
        &mut commands,
        &time,
        &mut phase,
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
fn eliminate_dead_cars(
    mut commands: Commands,
    assets: Res<CarAssets>,
    mut sfx: MessageWriter<PlaySfx>,
    cars: Query<(Entity, &Health, &Player, &Transform), With<Car>>,
) {
    for (entity, health, player, transform) in &cars {
        if health.current > 0.0 {
            continue;
        }
        info!("Player {} was wrecked!", player.id + 1);
        sfx.write(PlaySfx {
            kind: SfxKind::Wreck,
            position: Some(transform.translation),
        });
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
}

/// With 2+ players in the session, end the round when at most one car runs.
fn watch_for_winner(
    mut phase: ResMut<RoundPhase>,
    roster: Res<Roster>,
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
    match alive.first() {
        Some(player) => {
            let name = roster
                .players
                .iter()
                .find(|slot| slot.id == player.id)
                .map(|slot| NAMES[slot.name_index % NAMES.len()])
                .unwrap_or("Player ?");
            text.0 = format!("{name} wins the round!");
            color.0 = PLAYER_COLORS[player.color % PLAYER_COLORS.len()];
            info!("{name} wins the round!");
            sfx.write(PlaySfx {
                kind: SfxKind::RoundWin,
                position: None,
            });
        }
        None => {
            text.0 = "Everyone's wrecked — draw!".to_string();
            color.0 = Color::WHITE;
            info!("Round ends in a draw");
        }
    }
    *phase = RoundPhase::Over {
        countdown: RESET_SECONDS,
    };
}

/// Press R to restart the round immediately (dev / playtest shortcut).
fn restart_on_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
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
        &assets,
        &cop_assets,
        &roster,
        &leftovers,
        &mut *banner.into_inner(),
    );
}

/// After the banner pause: clear leftovers and respawn every roster player.
fn reset_round(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<RoundPhase>,
    assets: Res<CarAssets>,
    cop_assets: Res<CopAssets>,
    roster: Res<Roster>,
    leftovers: Query<Entity, Or<(With<Car>, With<Projectile>, With<CopCar>, With<Lifetime>)>>,
    banner: Single<&mut Text, With<Banner>>,
) {
    let RoundPhase::Over { countdown } = &mut *phase else {
        return;
    };
    *countdown -= time.delta_secs();
    if *countdown > 0.0 {
        return;
    }

    restart_round(
        &mut commands,
        &time,
        &mut phase,
        &assets,
        &cop_assets,
        &roster,
        &leftovers,
        &mut *banner.into_inner(),
    );
}

/// Despawn round entities and respawn every roster player plus one cop.
fn restart_round(
    commands: &mut Commands,
    time: &Time,
    phase: &mut RoundPhase,
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
    *phase = RoundPhase::Active;
}
