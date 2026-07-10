//! Lobby (design §6): press-to-join, per-player name & car color, ready-up.
//!
//! The game boots into the lobby with the arena as a live backdrop. Keyboard
//! and each gamepad join independently; names cycle from a preset list
//! (typing with a pad is party-game poison). When everyone is ready, a short
//! countdown starts the match. Tab / Select returns here mid-game.

use bevy::prelude::*;

use crate::vehicle::{InputSource, PlayerSlot, Roster, PLAYER_COLORS};

/// Arcade-style preset driver names, cycled with left/right.
pub const NAMES: [&str; 12] = [
    "Ace", "Blaze", "Crash", "Dizzy", "Enzo", "Flash", "Gonzo", "Hex", "Ivy", "Jinx", "Koto",
    "Loco",
];

const START_DELAY: f32 = 2.0;

#[derive(States, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum GameState {
    #[default]
    Lobby,
    InGame,
}

/// Ticks down once every joined player is ready; `None` while waiting.
#[derive(Resource, Default)]
struct StartCountdown(Option<f32>);

#[derive(Component)]
struct PanelRow;

#[derive(Component)]
struct PlayerPanel;

#[derive(Component)]
struct StatusText;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_resource::<StartCountdown>()
            .add_systems(OnEnter(GameState::Lobby), enter_lobby)
            .add_systems(
                Update,
                (
                    drop_disconnected_pads,
                    keyboard_lobby_input,
                    gamepad_lobby_input,
                    refresh_panels,
                    tick_countdown,
                )
                    .chain()
                    .run_if(in_state(GameState::Lobby)),
            )
            .add_systems(
                Update,
                back_to_lobby.run_if(in_state(GameState::InGame)),
            );
    }
}

// --- UI ---

fn enter_lobby(mut commands: Commands, mut roster: ResMut<Roster>) {
    // Everyone re-readies each visit; roster (players/names/colors) persists.
    for slot in &mut roster.players {
        slot.ready = false;
    }

    commands
        .spawn((
            Name::new("Lobby UI"),
            DespawnOnExit(GameState::Lobby),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(24.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("GENE RALLY WITH GUNS"),
                TextFont {
                    font_size: FontSize::Px(52.0),
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
            ));
            parent.spawn((
                StatusText,
                Text::new("Press Enter (keyboard) or A (gamepad) to join"),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            parent.spawn((
                PanelRow,
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(16.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
            parent.spawn((
                Text::new(
                    "Keyboard: Enter join/ready - arrows left/right name, up/down color - Backspace leave\n\
                     Gamepad: A join/ready - D-pad left/right name, up/down color - B leave\n\
                     In game: Tab / Select returns to lobby",
                ),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ));
        });
}

/// Rebuild the per-player panels whenever the roster changes.
fn refresh_panels(
    mut commands: Commands,
    roster: Res<Roster>,
    row: Single<Entity, With<PanelRow>>,
    panels: Query<Entity, With<PlayerPanel>>,
) {
    if !roster.is_changed() {
        return;
    }
    for panel in &panels {
        commands.entity(panel).try_despawn();
    }
    for slot in &roster.players {
        let color = PLAYER_COLORS[slot.color_index % PLAYER_COLORS.len()];
        let source_label = match slot.source {
            InputSource::Keyboard => "Keyboard",
            InputSource::Gamepad(_) => "Gamepad",
        };
        commands.entity(*row).with_children(|parent| {
            parent
                .spawn((
                    PlayerPanel,
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(14.0)),
                        width: Val::Px(150.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.1, 0.1, 0.12, 0.9)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(NAMES[slot.name_index % NAMES.len()]),
                        TextFont {
                            font_size: FontSize::Px(26.0),
                            ..default()
                        },
                        TextColor(color),
                    ));
                    // Car color swatch.
                    panel.spawn((
                        Node {
                            width: Val::Px(56.0),
                            height: Val::Px(28.0),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                    panel.spawn((
                        Text::new(source_label),
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.6, 0.6, 0.6)),
                    ));
                    let (ready_text, ready_color) = if slot.ready {
                        ("READY!", Color::srgb(0.3, 0.95, 0.3))
                    } else {
                        ("not ready", Color::srgb(0.6, 0.6, 0.6))
                    };
                    panel.spawn((
                        Text::new(ready_text),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(ready_color),
                    ));
                });
        });
    }
}

// --- Roster editing ---

fn free_color(roster: &Roster, from: usize, direction: i32) -> usize {
    let taken: Vec<usize> = roster.players.iter().map(|p| p.color_index).collect();
    let n = PLAYER_COLORS.len() as i32;
    for step in 1..=n {
        let candidate = ((from as i32 + direction * step).rem_euclid(n)) as usize;
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    from
}

fn free_name(roster: &Roster, from: usize, direction: i32) -> usize {
    let taken: Vec<usize> = roster.players.iter().map(|p| p.name_index).collect();
    let n = NAMES.len() as i32;
    for step in 1..=n {
        let candidate = ((from as i32 + direction * step).rem_euclid(n)) as usize;
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    from
}

fn join(roster: &mut Roster, source: InputSource) {
    if roster.players.len() >= PLAYER_COLORS.len() {
        return;
    }
    let id = roster.players.iter().map(|p| p.id + 1).max().unwrap_or(0);
    let color_index = free_color(roster, id % PLAYER_COLORS.len(), 1);
    let name_index = free_name(roster, id % NAMES.len(), 1);
    roster.players.push(PlayerSlot {
        id,
        source,
        name_index,
        color_index,
        ready: false,
    });
}

/// Join / toggle-ready / leave / cycle, for one input source.
struct LobbyIntent {
    join_or_ready: bool,
    leave: bool,
    name_dir: i32,
    color_dir: i32,
}

fn apply_intent(roster: &mut Roster, source: InputSource, intent: LobbyIntent) {
    let index = roster.players.iter().position(|p| p.source == source);
    match index {
        None => {
            if intent.join_or_ready {
                join(roster, source);
            }
        }
        Some(index) => {
            if intent.leave {
                roster.players.remove(index);
                return;
            }
            if intent.join_or_ready {
                roster.players[index].ready = !roster.players[index].ready;
            }
            if intent.name_dir != 0 && !roster.players[index].ready {
                let from = roster.players[index].name_index;
                roster.players[index].name_index = free_name(roster, from, intent.name_dir);
            }
            if intent.color_dir != 0 && !roster.players[index].ready {
                let from = roster.players[index].color_index;
                roster.players[index].color_index = free_color(roster, from, intent.color_dir);
            }
        }
    }
}

/// A gamepad that turns off (Bluetooth sleep, cable pull) reconnects as a new
/// entity, so its old slot can never receive input again — drop it.
fn drop_disconnected_pads(pads: Query<(), With<Gamepad>>, mut roster: ResMut<Roster>) {
    let gone = |slot: &PlayerSlot| match slot.source {
        InputSource::Keyboard => false,
        InputSource::Gamepad(entity) => !pads.contains(entity),
    };
    // Only touch the resource when needed; refresh_panels reacts to changes.
    if roster.players.iter().any(gone) {
        roster.players.retain(|slot| !gone(slot));
    }
}

fn keyboard_lobby_input(keys: Res<ButtonInput<KeyCode>>, mut roster: ResMut<Roster>) {
    let intent = LobbyIntent {
        join_or_ready: keys.just_pressed(KeyCode::Enter),
        leave: keys.just_pressed(KeyCode::Backspace),
        name_dir: keys.just_pressed(KeyCode::ArrowRight) as i32
            - keys.just_pressed(KeyCode::ArrowLeft) as i32,
        color_dir: keys.just_pressed(KeyCode::ArrowDown) as i32
            - keys.just_pressed(KeyCode::ArrowUp) as i32,
    };
    if intent.join_or_ready || intent.leave || intent.name_dir != 0 || intent.color_dir != 0 {
        apply_intent(&mut roster, InputSource::Keyboard, intent);
    }
}

fn gamepad_lobby_input(pads: Query<(Entity, &Gamepad)>, mut roster: ResMut<Roster>) {
    for (entity, pad) in &pads {
        let intent = LobbyIntent {
            join_or_ready: pad.just_pressed(GamepadButton::South),
            leave: pad.just_pressed(GamepadButton::East),
            name_dir: pad.just_pressed(GamepadButton::DPadRight) as i32
                - pad.just_pressed(GamepadButton::DPadLeft) as i32,
            color_dir: pad.just_pressed(GamepadButton::DPadDown) as i32
                - pad.just_pressed(GamepadButton::DPadUp) as i32,
        };
        if intent.join_or_ready || intent.leave || intent.name_dir != 0 || intent.color_dir != 0 {
            apply_intent(&mut roster, InputSource::Gamepad(entity), intent);
        }
    }
}

// --- Match start / return ---

fn tick_countdown(
    time: Res<Time>,
    roster: Res<Roster>,
    mut countdown: ResMut<StartCountdown>,
    mut next: ResMut<NextState<GameState>>,
    status: Single<&mut Text, With<StatusText>>,
) {
    let all_ready = !roster.players.is_empty() && roster.players.iter().all(|p| p.ready);
    let mut status = status.into_inner();

    if !all_ready {
        countdown.0 = None;
        status.0 = if roster.players.is_empty() {
            "Press Enter (keyboard) or A (gamepad) to join".to_string()
        } else {
            "Waiting for everyone to ready up...".to_string()
        };
        return;
    }

    let remaining = countdown.0.get_or_insert(START_DELAY);
    *remaining -= time.delta_secs();
    status.0 = format!("Starting in {:.0}...", remaining.ceil().max(1.0));
    if *remaining <= 0.0 {
        countdown.0 = None;
        next.set(GameState::InGame);
    }
}

fn back_to_lobby(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<GameState>>,
) {
    let pad_select = pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    if keys.just_pressed(KeyCode::Tab) || pad_select {
        next.set(GameState::Lobby);
    }
}
