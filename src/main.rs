//! Gene Rally with Guns — M0 scaffold.
//!
//! Iso camera + flat arena + arcade toy cars. Keyboard drives player 1;
//! gamepads press South (A / Cross) to join. See docs/game-design.md.

mod arena;
mod camera;
mod cop;
mod input;
mod nav;
mod pickup;
mod round;
mod vehicle;
mod weapon;

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::input::CarAction;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gene Rally with Guns".into(),
                    resolution: (1280, 720).into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(InputManagerPlugin::<CarAction>::default())
        .insert_resource(ClearColor(Color::srgb(0.13, 0.14, 0.17)))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 300.0,
            ..default()
        })
        .add_plugins((
            arena::ArenaPlugin,
            vehicle::VehiclePlugin,
            camera::CameraPlugin,
            weapon::WeaponPlugin,
            pickup::PickupPlugin,
            round::RoundPlugin,
            nav::NavPlugin,
            cop::CopPlugin,
        ))
        .add_systems(Update, quit_on_esc)
        .run();
}

fn quit_on_esc(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
