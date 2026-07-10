//! CPU players: lobby-added bots that drive regular player cars.
//!
//! A bot is a normal car (same chassis, weapons, health, pickups) whose
//! `ActionState<CarAction>` is written by AI instead of a device. Navigation
//! mirrors the cop brain — chase along A* waypoints, back out when stuck —
//! plus a trigger finger: burst-fire when an enemy sits in the aim cone.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::input::CarAction;
use crate::nav::NavGrid;
use crate::pickup::WeaponCrate;
use crate::vehicle::{self, Car, Player};
use crate::weapon::WeaponSlot;

/// Only shoot when the target is this close (world units)...
const FIRE_RANGE: f32 = 22.0;
/// ...and within this half-angle (rad) of our heading.
const AIM_CONE: f32 = 0.35;
/// Trigger cadence: hold fire for the burst, then let go. The release also
/// makes charged weapons (grenades) launch instead of charging forever.
const BURST_PERIOD: f32 = 1.3;
const BURST_LENGTH: f32 = 0.75;
/// Inside this range while aimed, ease off the gas and shoot it out instead
/// of trading paint.
const STANDOFF_RANGE: f32 = 8.0;

#[derive(Component, Default)]
pub struct BotDriver {
    stuck_time: f32,
    reversing_time: f32,
    /// Remaining A* waypoints toward the current target (world space).
    path: Vec<Vec3>,
    /// Countdown to the next repath.
    repath_time: f32,
    /// Time since this burst cycle started; wraps at BURST_PERIOD.
    burst_time: f32,
}

pub struct BotPlugin;

impl Plugin for BotPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, bot_ai.before(vehicle::drive_cars));
    }
}

/// Write throttle/steer/fire into each bot's ActionState.
fn bot_ai(
    time: Res<Time>,
    nav: Res<NavGrid>,
    mut bots: Query<
        (
            Entity,
            &mut BotDriver,
            &mut ActionState<CarAction>,
            &Transform,
            &LinearVelocity,
            &WeaponSlot,
        ),
        With<Car>,
    >,
    targets: Query<(Entity, &Transform), (With<Car>, With<Player>)>,
    crates: Query<&Transform, With<WeaponCrate>>,
) {
    let dt = time.delta_secs();
    for (me, mut bot, mut actions, transform, velocity, slot) in &mut bots {
        let pos = transform.translation;

        // Nearest other living player car, planar distance.
        let enemy = targets
            .iter()
            .filter(|(entity, _)| *entity != me)
            .min_by(|(_, a), (_, b)| {
                let da = (a.translation - pos).xz().length_squared();
                let db = (b.translation - pos).xz().length_squared();
                da.total_cmp(&db)
            })
            .map(|(_, t)| t.translation);

        // Unarmed (fresh spawn or gun ran dry): detour to the nearest crate.
        let needs_gun = slot.kind.is_none();
        let wanted_crate = needs_gun
            .then(|| {
                crates
                    .iter()
                    .min_by(|a, b| {
                        let da = (a.translation - pos).xz().length_squared();
                        let db = (b.translation - pos).xz().length_squared();
                        da.total_cmp(&db)
                    })
                    .map(|t| t.translation)
            })
            .flatten();

        // Drive at the crate when we want one, otherwise at the enemy.
        let Some(target) = wanted_crate.or(enemy) else {
            // Last one standing, nothing to collect: roll to a stop.
            actions.set_value(&CarAction::Throttle, 0.0);
            actions.set_value(&CarAction::Steer, 0.0);
            actions.release(&CarAction::Fire);
            continue;
        };

        if bot.reversing_time > 0.0 {
            // Unsticking: back out with wheels turned, guns quiet.
            bot.reversing_time -= dt;
            actions.set_value(&CarAction::Throttle, -1.0);
            actions.set_value(&CarAction::Steer, 0.7);
            actions.release(&CarAction::Fire);
            continue;
        }

        // Repath periodically (or when the path ran out).
        bot.repath_time -= dt;
        if bot.repath_time <= 0.0 || bot.path.is_empty() {
            bot.repath_time = 0.4;
            bot.path = if nav.line_of_sight(pos, target) {
                Vec::new() // straight shot; no waypoints needed
            } else {
                nav.find_path(pos, target).unwrap_or_default()
            };
        }

        // Drop reached waypoints, steer at the furthest visible one
        // (string-pulling lite); fall back to the enemy itself.
        while bot
            .path
            .first()
            .is_some_and(|w| (*w - pos).xz().length() < 2.2)
        {
            bot.path.remove(0);
        }
        let steer_point = bot
            .path
            .iter()
            .take(8)
            .rev()
            .find(|w| nav.line_of_sight(pos, **w))
            .copied()
            .unwrap_or(target);

        let forward = *transform.forward();
        let to_point = ((steer_point - pos) * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
        let angle = forward.cross(to_point).y.atan2(forward.dot(to_point));
        let steer = (-angle * 1.5).clamp(-1.0, 1.0);

        // Trigger control: armed, and the enemy is in range, in the cone,
        // and visible.
        let can_shoot = !needs_gun
            && enemy.is_some_and(|enemy| {
            let to_enemy = (enemy - pos).xz();
            to_enemy.length() < FIRE_RANGE
                && forward.xz().angle_to(to_enemy).abs() < AIM_CONE
                && nav.line_of_sight(pos, enemy)
        });

        // Keep some standoff in a shootout — unless we're crate-hunting, in
        // which case the crate matters more than trading peashooter fire.
        let close_to_enemy =
            enemy.is_some_and(|enemy| (enemy - pos).xz().length() < STANDOFF_RANGE);
        let throttle = if can_shoot && close_to_enemy && !needs_gun {
            0.35
        } else {
            1.0
        };

        actions.set_value(&CarAction::Throttle, throttle);
        actions.set_value(&CarAction::Steer, steer);
        if can_shoot {
            bot.burst_time = (bot.burst_time + dt) % BURST_PERIOD;
            if bot.burst_time < BURST_LENGTH {
                actions.press(&CarAction::Fire);
            } else {
                actions.release(&CarAction::Fire);
            }
        } else {
            bot.burst_time = 0.0;
            actions.release(&CarAction::Fire);
        }

        // Stuck detection: wants to move but barely moving.
        let planar_speed = velocity.0.xz().length();
        if planar_speed < 1.0 {
            bot.stuck_time += dt;
        } else {
            bot.stuck_time = 0.0;
        }
        if bot.stuck_time > 1.2 {
            bot.stuck_time = 0.0;
            bot.reversing_time = 0.9;
            bot.path.clear(); // force a repath after backing out
            bot.repath_time = 0.0;
        }
    }
}
