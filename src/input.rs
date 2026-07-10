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
        // Bots have no device; their AI writes the ActionState directly.
        InputSource::Cpu => InputMap::default(),
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

/// Top-down racer layout: triggers (or A) for gas/brake, left-stick X for steer.
///
/// The left stick Y axis is intentionally *not* bound to throttle — mapping it
/// like WASD means gas and steer fight on the same stick and you cannot hold
/// a turn while accelerating.
fn gamepad_map(gamepad: Entity) -> InputMap<CarAction> {
    InputMap::default()
        // Analog triggers: LT brake/reverse, RT accelerate.
        .with_axis(
            CarAction::Throttle,
            VirtualAxis::new(GamepadButton::LeftTrigger2, GamepadButton::RightTrigger2)
                .with_deadzone_symmetric(0.05),
        )
        // Digital gas for party-game muscle memory (A / Cross).
        .with_axis(
            CarAction::Throttle,
            VirtualAxis::new(GamepadButton::Mode, GamepadButton::South),
        )
        // Steer on horizontal stick only — independent of throttle.
        .with_axis(
            CarAction::Steer,
            GamepadControlAxis::LEFT_X.with_deadzone_symmetric(0.12),
        )
        .with(CarAction::Handbrake, GamepadButton::East)
        .with(CarAction::Fire, GamepadButton::West)
        .with_gamepad(gamepad)
}