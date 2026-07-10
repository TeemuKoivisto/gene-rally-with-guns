//! In-game pause menu: Esc / Start opens it, freezing the match; players can
//! resume, return to the lobby, or quit.
//!
//! Pausing works by stopping `Time<Virtual>`: `FixedUpdate` stops running
//! (driving, weapons, physics, AI all halt) and every `Res<Time>` delta reads
//! zero, so round countdowns and lifetimes freeze without any per-system
//! gating. Menu input runs in `Update`, which keeps ticking.

use bevy::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::lobby::GameState;

/// Whether the running match is paused; only exists while in game.
#[derive(SubStates, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[source(GameState = GameState::InGame)]
pub enum PlayState {
    #[default]
    Running,
    Paused,
}

const MENU_ITEMS: [&str; 3] = ["Resume", "Return to Lobby", "Quit Game"];
const SELECTED_COLOR: Color = Color::srgb(1.0, 0.85, 0.2);
const UNSELECTED_COLOR: Color = Color::srgb(0.75, 0.75, 0.75);

/// Currently highlighted menu row.
#[derive(Resource, Default)]
struct MenuIndex(usize);

#[derive(Component)]
struct MenuItem(usize);

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_sub_state::<PlayState>()
            .init_resource::<MenuIndex>()
            .add_systems(Update, open_menu.run_if(in_state(PlayState::Running)))
            .add_systems(OnEnter(PlayState::Paused), (spawn_menu, pause_time))
            .add_systems(OnExit(PlayState::Paused), resume_time)
            .add_systems(
                Update,
                (menu_input, highlight_selection)
                    .chain()
                    .run_if(in_state(PlayState::Paused)),
            );
    }
}

fn open_menu(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<PlayState>>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let pad_start = pads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    if keys.just_pressed(KeyCode::Escape) || pad_start {
        next.set(PlayState::Paused);
        sfx.write(PlaySfx {
            kind: SfxKind::UiClick,
            position: None,
        });
    }
}

fn pause_time(mut time: ResMut<Time<Virtual>>) {
    time.pause();
}

/// Runs both on resume and when the menu exits to the lobby.
fn resume_time(mut time: ResMut<Time<Virtual>>) {
    time.unpause();
}

fn spawn_menu(mut commands: Commands, mut index: ResMut<MenuIndex>) {
    index.0 = 0;
    commands
        .spawn((
            Name::new("Pause menu"),
            DespawnOnExit(PlayState::Paused),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("PAUSED"),
                TextFont {
                    font_size: FontSize::Px(48.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            for (i, label) in MENU_ITEMS.iter().enumerate() {
                parent.spawn((
                    MenuItem(i),
                    Text::new(*label),
                    TextFont {
                        font_size: FontSize::Px(30.0),
                        ..default()
                    },
                    TextColor(if i == 0 {
                        SELECTED_COLOR
                    } else {
                        UNSELECTED_COLOR
                    }),
                ));
            }
            parent.spawn((
                Text::new("Up/Down or D-pad to choose - Enter / A to confirm - Esc / Start resumes"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
            ));
        });
}

fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut index: ResMut<MenuIndex>,
    mut next_play: ResMut<NextState<PlayState>>,
    mut next_game: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let close = keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| {
            p.just_pressed(GamepadButton::Start) || p.just_pressed(GamepadButton::East)
        });
    if close {
        next_play.set(PlayState::Running);
        sfx.write(PlaySfx {
            kind: SfxKind::UiClick,
            position: None,
        });
        return;
    }

    let down = keys.just_pressed(KeyCode::ArrowDown)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadDown));
    let up = keys.just_pressed(KeyCode::ArrowUp)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadUp));
    let step = down as i32 - up as i32;
    if step != 0 {
        let n = MENU_ITEMS.len() as i32;
        index.0 = ((index.0 as i32 + step).rem_euclid(n)) as usize;
        sfx.write(PlaySfx {
            kind: SfxKind::UiClick,
            position: None,
        });
    }

    let confirm = keys.just_pressed(KeyCode::Enter)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::South));
    if confirm {
        sfx.write(PlaySfx {
            kind: SfxKind::UiSelect,
            position: None,
        });
        match index.0 {
            0 => next_play.set(PlayState::Running),
            1 => next_game.set(GameState::Lobby),
            _ => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Recolor rows when the selection moves.
fn highlight_selection(index: Res<MenuIndex>, mut items: Query<(&MenuItem, &mut TextColor)>) {
    if !index.is_changed() {
        return;
    }
    for (item, mut color) in &mut items {
        color.0 = if item.0 == index.0 {
            SELECTED_COLOR
        } else {
            UNSELECTED_COLOR
        };
    }
}
