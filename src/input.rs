//! Player actions and device bindings (leafwing-input-manager).

use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::vehicle::InputSource;

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum CarAction {
    /// -1.0 (reverse/brake) ..= 1.0 (full throttle)
    #[actionlike(Axis)]
    Throttle,
    /// -1.0 (left) ..= 1.0 (right)
    #[actionlike(Axis)]
    Steer,
    Handbrake,
    Fire,
}

pub fn map_for(source: InputSource) -> InputMap<CarAction> {
    match source {
        InputSource::Keyboard => keyboard_map(),
        InputSource::Gamepad(pad) => gamepad_map(pad),
    }
}

/// Keyboard bindings for the bench-test player (player 1 without a pad).
fn keyboard_map() -> InputMap<CarAction> {
    InputMap::default()
        .with_axis(CarAction::Throttle, VirtualAxis::ws())
        .with_axis(CarAction::Throttle, VirtualAxis::vertical_arrow_keys())
        .with_axis(CarAction::Steer, VirtualAxis::ad())
        .with_axis(CarAction::Steer, VirtualAxis::horizontal_arrow_keys())
        .with(CarAction::Handbrake, KeyCode::ShiftLeft)
        .with(CarAction::Handbrake, KeyCode::ShiftRight)
        .with(CarAction::Fire, KeyCode::Space)
}

/// Gamepad bindings, tied to one specific pad entity.
fn gamepad_map(gamepad: Entity) -> InputMap<CarAction> {
    InputMap::default()
        // Right trigger accelerates, left trigger brakes/reverses.
        .with_axis(
            CarAction::Throttle,
            VirtualAxis::new(GamepadButton::LeftTrigger2, GamepadButton::RightTrigger2),
        )
        // Left stick up/down also works (kart-style fallback).
        .with_axis(
            CarAction::Throttle,
            GamepadControlAxis::LEFT_Y.with_deadzone_symmetric(0.2),
        )
        .with_axis(
            CarAction::Steer,
            GamepadControlAxis::LEFT_X.with_deadzone_symmetric(0.15),
        )
        .with(CarAction::Handbrake, GamepadButton::South)
        .with(CarAction::Fire, GamepadButton::West)
        .with(CarAction::Fire, GamepadButton::RightTrigger)
        .with_gamepad(gamepad)
}
