//! Arcade toy-car driving: the simplified plane-force model from design §9,
//! plus car health and the floating health bars.
//!
//! The car is a dynamic rigid body with X/Z rotation locked (stays flat).
//! Each fixed tick we decompose planar velocity into forward/lateral parts,
//! apply throttle/brake to the forward part, bleed the lateral part within a
//! grip budget (slashed for handbrake drifts), and command yaw from a
//! kinematic bicycle model — turn radius from wheel angle and wheelbase,
//! capped by the same grip budget. No suspension sim.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z};
use crate::audio::{PlaySfx, SfxKind};
use crate::cop::CopCar;
use crate::input::{self, CarAction};
use crate::weapon::{self, Lifetime, Projectile, ProjectileAssets, WeaponSlot};

/// Arcade handling parameters; players and cops share the model, not the numbers.
pub struct DriveParams {
    pub max_speed: f32,
    pub max_reverse_speed: f32,
    pub engine_accel: f32,
    pub brake_accel: f32,
    /// Passive deceleration when coasting (m/s^2).
    pub coast_drag: f32,
    /// Distance between axles (m): sets the turning geometry. Minimum turn
    /// radius is `wheelbase / tan(max_steer_angle)`.
    pub wheelbase: f32,
    /// Front wheel angle at full lock (rad).
    pub max_steer_angle: f32,
    /// Lateral grip as an acceleration budget (m/s^2). It caps cornering
    /// force — demanding a tighter arc than the tires can hold runs the car
    /// wide — and (at low speed) the same budget bleeds off sideways sliding.
    pub grip: f32,
    /// Slide-bleed budget at max speed (m/s^2). Set below `grip` and fast
    /// cornering leaves sideways velocity alive: the tail steps out and the
    /// car drifts through quick corners while low-speed handling stays
    /// planted. Steering keeps the full `grip` cap, so you rotate into it.
    pub fast_grip: f32,
    /// Slide-bleed budget while the handbrake is held (m/s^2): low = the
    /// sideways velocity survives = big drifts. Steering geometry keeps the
    /// full `grip` cap, so you can rotate into the slide.
    pub handbrake_grip: f32,
    /// How quickly the actual yaw rate follows the steering geometry (1/s).
    /// Lower = smoother, heavier turn-in; higher = twitchier.
    pub yaw_response: f32,
    /// How strongly persistent sideways velocity drags forward speed down
    /// (1/s per m/s of slide). Sliding scrubs speed: clean lines beat sloppy
    /// ones, and drifts trade speed for angle.
    pub slide_scrub: f32,
    /// Pivot assist (rad/s at full lock, standstill): geometry can't turn a
    /// stationary car, but guns fire forward and aiming matters, so at
    /// walking speed steering also pivots the car directly. Fades out by
    /// `PIVOT_FADE_SPEED`. 0 = off.
    pub pivot_yaw_rate: f32,
}

/// Speed (m/s) at which the pivot assist has fully faded out.
const PIVOT_FADE_SPEED: f32 = 2.0;

pub const PLAYER_DRIVE: DriveParams = DriveParams {
    max_speed: 18.0,
    max_reverse_speed: 8.0,
    engine_accel: 42.0,
    brake_accel: 58.0,
    // Strong engine braking: lifting the gas sheds speed fast (full-speed
    // coast rolls out in ~13 m rather than gliding across the arena).
    coast_drag: 12.0,
    // A touch shorter than the visual wheel rows (z = ±0.69): kept at the
    // tuned value so the bigger body doesn't dull the agility.
    wheelbase: 1.2,
    // ~31 degrees: minimum turn radius ~2.0 m.
    max_steer_angle: 0.55,
    // Toy-car grippy: full-speed turn radius = v^2 / grip ~ 6.7 m.
    // Lower = everything drifts, higher = on rails.
    grip: 56.0,
    fast_grip: 38.0,
    handbrake_grip: 12.0,
    yaw_response: 15.0,
    slide_scrub: 0.7,
    pivot_yaw_rate: 2.0,
};

pub const MAX_HEALTH: f32 = 100.0;
const HEALTH_BAR_WIDTH: f32 = 1.4;

/// Candy-bright, high-contrast player colors (design §10; 8 local players).
pub const PLAYER_COLORS: [Color; 8] = [
    Color::srgb(0.95, 0.25, 0.25), // red
    Color::srgb(0.25, 0.55, 0.95), // blue
    Color::srgb(0.35, 0.85, 0.35), // green
    Color::srgb(0.95, 0.85, 0.25), // yellow
    Color::srgb(0.85, 0.35, 0.85), // magenta
    Color::srgb(0.30, 0.85, 0.85), // cyan
    Color::srgb(0.95, 0.55, 0.20), // orange
    Color::srgb(0.90, 0.90, 0.90), // white
];

#[derive(Component)]
pub struct Car;

/// Cosmetic body shell (chassis mesh + cabin + wheels): a child of the flat
/// physics body that leans with acceleration — roll in corners, pitch under
/// throttle and braking — so the car visually carries weight.
#[derive(Component, Default)]
pub struct CarBody {
    roll: f32,
    pitch: f32,
    prev_fwd_speed: f32,
}

/// Lean angles per m/s^2 of acceleration, their caps (rad), and how quickly
/// the shell settles onto the target lean (1/s).
const ROLL_PER_ACCEL: f32 = 0.0028;
const PITCH_PER_ACCEL: f32 = 0.0016;
const MAX_ROLL: f32 = 0.14;
const MAX_PITCH: f32 = 0.09;
const LEAN_RESPONSE: f32 = 8.0;

/// Ramps digital (keyboard) steering so full lock isn't reached in one frame.
/// Sticks are analog and bots write analog values, so only keyboard cars get
/// this. Deflecting is slower than returning to center: turn-in stays
/// controllable, letting go recovers instantly.
#[derive(Component, Default)]
pub struct SteerAssist {
    current: f32,
}

const STEER_ATTACK_RATE: f32 = 10.0;
const STEER_RELEASE_RATE: f32 = 16.0;

impl SteerAssist {
    fn slew(&mut self, target: f32, dt: f32) -> f32 {
        // Deflecting further in the same direction uses the slow attack rate;
        // easing off or crossing center uses the fast release rate.
        let deflecting = self.current * target >= 0.0 && target.abs() > self.current.abs();
        let rate = if deflecting {
            STEER_ATTACK_RATE
        } else {
            STEER_RELEASE_RATE
        };
        let step = rate * dt;
        self.current += (target - self.current).clamp(-step, step);
        self.current
    }
}

#[derive(Component)]
pub struct Player {
    pub id: usize,
    /// Index into PLAYER_COLORS (chosen in the lobby).
    pub color: usize,
}

#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn frac(&self) -> f32 {
        (self.current / self.max).clamp(0.0, 1.0)
    }
}

/// How a player's car is controlled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    Keyboard,
    Gamepad(Entity),
    /// AI-driven (lobby-added bot for testing / filling out a party).
    Cpu,
}

pub struct PlayerSlot {
    pub id: usize,
    pub source: InputSource,
    /// Index into lobby::NAMES.
    pub name_index: usize,
    /// Index into PLAYER_COLORS.
    pub color_index: usize,
    /// Lobby ready flag; meaningless once in game.
    pub ready: bool,
    /// Match points (party scoring); reset when a new match starts.
    pub score: u32,
}

/// Everyone who has joined the session (survives round resets).
#[derive(Resource, Default)]
pub struct Roster {
    pub players: Vec<PlayerSlot>,
}

/// Floating health bar; a world-aligned top-level entity following one car.
#[derive(Component)]
pub struct HealthBar {
    pub car: Entity,
}

#[derive(Component)]
struct HealthBarFill;

/// Shared meshes/materials for spawning cars.
#[derive(Resource)]
pub struct CarAssets {
    chassis: Handle<Mesh>,
    cabin: Handle<Mesh>,
    wheel: Handle<Mesh>,
    pub debris: Handle<Mesh>,
    wheel_material: Handle<StandardMaterial>,
    pub body_materials: Vec<Handle<StandardMaterial>>,
    bar_fill_mesh: Handle<Mesh>,
    bar_back_mesh: Handle<Mesh>,
    bar_fill_material: Handle<StandardMaterial>,
    bar_back_material: Handle<StandardMaterial>,
}

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Roster>()
            .add_systems(Startup, setup_car_assets)
            .add_systems(Update, (update_health_bars, car_wall_impacts))
            .add_systems(FixedUpdate, (drive_cars, lean_car_bodies).chain());
    }
}

/// Speed below which hitting a wall is a nudge, not an event.
const WALL_IMPACT_MIN_SPEED: f32 = 7.0;

/// Crunch feedback when a car slams into the world (walls, buildings):
/// sound and sparks, gated by impact speed. Car/cop contacts are excluded —
/// rams have their own damage-and-flash path.
fn car_wall_impacts(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    assets: Res<ProjectileAssets>,
    mut sfx: MessageWriter<PlaySfx>,
    cars: Query<(&LinearVelocity, &Transform), With<Car>>,
    not_world: Query<
        (),
        Or<(
            With<Car>,
            With<CopCar>,
            With<Projectile>,
            With<Sensor>,
            With<Lifetime>,
        )>,
    >,
) {
    for event in collisions.read() {
        let a = event.body1.unwrap_or(event.collider1);
        let b = event.body2.unwrap_or(event.collider2);
        for (car_entity, other) in [(a, b), (b, a)] {
            let Ok((velocity, transform)) = cars.get(car_entity) else {
                continue;
            };
            if not_world.contains(other) {
                continue;
            }
            let speed = velocity.0.xz().length();
            if speed < WALL_IMPACT_MIN_SPEED {
                continue;
            }
            sfx.write(PlaySfx {
                kind: SfxKind::Crunch,
                position: Some(transform.translation),
            });
            weapon::spawn_hit_sparks(
                &mut commands,
                &assets,
                transform.translation,
                car_entity.to_bits() as u32,
            );
            break; // one crunch per contact pair
        }
    }
}

/// Tilt each car's cosmetic shell with its acceleration: roll away from the
/// turn (centripetal accel = speed * yaw rate) and pitch with speed changes
/// (nose up on launch, nose dive on braking). Physics never sees the tilt.
fn lean_car_bodies(
    time: Res<Time>,
    cars: Query<(&Transform, &LinearVelocity, &AngularVelocity), With<Car>>,
    mut bodies: Query<(&ChildOf, &mut CarBody, &mut Transform), Without<Car>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let blend = 1.0 - (-LEAN_RESPONSE * dt).exp();
    for (child_of, mut body, mut transform) in &mut bodies {
        let Ok((car_transform, vel, ang)) = cars.get(child_of.parent()) else {
            continue;
        };
        let forward = *car_transform.forward();
        let planar = Vec3::new(vel.x, 0.0, vel.z);
        let fwd_speed = planar.dot(forward);
        let lat_accel = fwd_speed * ang.y;
        let fwd_accel = (fwd_speed - body.prev_fwd_speed) / dt;
        body.prev_fwd_speed = fwd_speed;

        let target_roll = (-lat_accel * ROLL_PER_ACCEL).clamp(-MAX_ROLL, MAX_ROLL);
        let target_pitch = (fwd_accel * PITCH_PER_ACCEL).clamp(-MAX_PITCH, MAX_PITCH);
        body.roll += (target_roll - body.roll) * blend;
        body.pitch += (target_pitch - body.pitch) * blend;
        transform.rotation =
            Quat::from_rotation_x(body.pitch) * Quat::from_rotation_z(body.roll);
    }
}

fn setup_car_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body_materials = PLAYER_COLORS
        .iter()
        .map(|&color| {
            materials.add(StandardMaterial {
                base_color: color,
                // Glossy die-cast toy look.
                perceptual_roughness: 0.3,
                metallic: 0.1,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(CarAssets {
        chassis: meshes.add(Cuboid::new(1.15, 0.46, 2.3)),
        cabin: meshes.add(Cuboid::new(0.92, 0.4, 1.05)),
        wheel: meshes.add(Cuboid::new(0.23, 0.4, 0.4)),
        debris: meshes.add(Cuboid::new(0.3, 0.3, 0.3)),
        wheel_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.12, 0.12),
            perceptual_roughness: 0.9,
            ..default()
        }),
        body_materials,
        bar_fill_mesh: meshes.add(Cuboid::new(HEALTH_BAR_WIDTH, 0.1, 0.16)),
        bar_back_mesh: meshes.add(Cuboid::new(HEALTH_BAR_WIDTH + 0.1, 0.08, 0.22)),
        bar_fill_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.9, 0.3),
            unlit: true,
            ..default()
        }),
        bar_back_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.08, 0.08),
            unlit: true,
            ..default()
        }),
    });
}

/// Spawn a car (and its health bar) for a roster slot; `position` is the
/// slot's index on the spawn ellipse (roster order). Used on round start/reset.
pub fn spawn_car(
    commands: &mut Commands,
    assets: &CarAssets,
    materials: &mut Assets<StandardMaterial>,
    slot: &PlayerSlot,
    position: usize,
) {
    // Spawn points on an ellipse around the center, facing inward.
    let angle = position as f32 * std::f32::consts::TAU / PLAYER_COLORS.len() as f32;
    let pos = Vec3::new(
        angle.cos() * ARENA_HALF_X * 0.7,
        0.0,
        angle.sin() * ARENA_HALF_Z * 0.7,
    );
    let color_index = slot.color_index;
    if let Some(mut mat) = materials.get_mut(&assets.body_materials[color_index]) {
        mat.emissive = LinearRgba::BLACK;
    }
    let body = assets.body_materials[color_index].clone();

    let mut car_entity = commands
        .spawn((
            Name::new(format!("Car P{}", slot.id + 1)),
            Car,
            Player {
                id: slot.id,
                color: color_index,
            },
            Health {
                current: MAX_HEALTH,
                max: MAX_HEALTH,
            },
            WeaponSlot::default(),
            // The visible shell lives on a CarBody child so it can lean while
            // the physics body stays flat.
            Visibility::default(),
            Transform::from_translation(pos + Vec3::Y * 0.6)
                .looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y),
            // Nested: bundles cap at 16 top-level elements.
            (
                RigidBody::Dynamic,
                Collider::cuboid(1.15, 0.46, 2.3),
                LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                // Frictionless contacts: grip lives in the drive model, and
                // scraping a wall must not glue the car to it.
                Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
                // Springy: wall hits bounce you off instead of pinning you.
                Restitution::new(0.6).with_combine_rule(CoefficientCombine::Max),
                Mass(6.0),
                // Without this, avian emits no CollisionStart for car-vs-wall
                // pairs (walls have no event flag) and the crunch never fires.
                CollisionEventsEnabled,
            ),
            ActionState::<CarAction>::default(),
        ));
    // Humans get device bindings feeding their ActionState; bots get an AI
    // driver that writes into the same ActionState, so drive/fire systems
    // treat both identically.
    match slot.source {
        InputSource::Cpu => {
            car_entity.insert(crate::bot::BotDriver::default());
        }
        source => {
            car_entity.insert(input::map_for(source));
            if source == InputSource::Keyboard {
                car_entity.insert(SteerAssist::default());
            }
        }
    }
    car_entity.with_children(|parent| {
        parent
            .spawn((
                CarBody::default(),
                Mesh3d(assets.chassis.clone()),
                MeshMaterial3d(body.clone()),
                Transform::default(),
            ))
            .with_children(|shell| {
                // Cabin.
                shell.spawn((
                    Mesh3d(assets.cabin.clone()),
                    MeshMaterial3d(body),
                    Transform::from_xyz(0.0, 0.4, 0.17),
                ));
                // Wheels (cosmetic).
                for (x, z) in [(-0.63, -0.69), (0.63, -0.69), (-0.63, 0.69), (0.63, 0.69)] {
                    shell.spawn((
                        Mesh3d(assets.wheel.clone()),
                        MeshMaterial3d(assets.wheel_material.clone()),
                        Transform::from_xyz(x, -0.12, z),
                    ));
                }
            });
    });
    let car = car_entity.id();

    spawn_health_bar(commands, assets, car, pos);
}

/// World-aligned health bar floating above a vehicle (not a child: it must
/// not rotate with the chassis). Used for players and cops alike.
pub fn spawn_health_bar(commands: &mut Commands, assets: &CarAssets, car: Entity, pos: Vec3) {
    commands
        .spawn((
            Name::new("Health bar"),
            HealthBar { car },
            Transform::from_translation(pos + Vec3::Y * 1.8),
            Visibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.bar_back_mesh.clone()),
                MeshMaterial3d(assets.bar_back_material.clone()),
                Transform::default(),
            ));
            parent.spawn((
                HealthBarFill,
                Mesh3d(assets.bar_fill_mesh.clone()),
                MeshMaterial3d(assets.bar_fill_material.clone()),
                Transform::from_xyz(0.0, 0.02, 0.0),
            ));
        });
}

/// Keep bars above their cars, scale the fill with health, and clean up bars
/// whose car is gone (covers every despawn path).
fn update_health_bars(
    mut commands: Commands,
    cars: Query<(&Transform, &Health), (Without<HealthBar>, Without<HealthBarFill>)>,
    mut bars: Query<(Entity, &mut Transform, &HealthBar, &Children), Without<HealthBarFill>>,
    mut fills: Query<&mut Transform, (With<HealthBarFill>, Without<HealthBar>)>,
) {
    for (bar_entity, mut bar_transform, bar, children) in &mut bars {
        let Ok((car_transform, health)) = cars.get(bar.car) else {
            commands.entity(bar_entity).try_despawn();
            continue;
        };
        bar_transform.translation = car_transform.translation + Vec3::Y * 1.8;
        let frac = health.frac();
        for child in children.iter() {
            if let Ok(mut fill) = fills.get_mut(child) {
                fill.scale.x = frac.max(0.001);
                // Keep the fill anchored to the left edge as it shrinks.
                fill.translation.x = -(1.0 - frac) * HEALTH_BAR_WIDTH / 2.0;
            }
        }
    }
}

pub(crate) fn drive_cars(
    time: Res<Time>,
    mut cars: Query<
        (
            &ActionState<CarAction>,
            &Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            Option<&mut SteerAssist>,
        ),
        With<Car>,
    >,
) {
    let dt = time.delta_secs();
    for (actions, transform, mut lin_vel, mut ang_vel, assist) in &mut cars {
        let raw_steer = actions.clamped_value(&CarAction::Steer);
        let steer = match assist {
            Some(mut assist) => assist.slew(raw_steer, dt),
            None => raw_steer,
        };
        apply_drive(
            dt,
            &PLAYER_DRIVE,
            actions.clamped_value(&CarAction::Throttle),
            steer,
            actions.pressed(&CarAction::Handbrake),
            transform,
            &mut lin_vel,
            &mut ang_vel,
        );
    }
}

/// One fixed tick of the arcade plane-force model (see module docs).
/// Shared by player driving and cop AI.
#[allow(clippy::too_many_arguments)]
pub fn apply_drive(
    dt: f32,
    params: &DriveParams,
    throttle: f32,
    steer: f32,
    handbrake: bool,
    transform: &Transform,
    lin_vel: &mut LinearVelocity,
    ang_vel: &mut AngularVelocity,
) {
    let forward = *transform.forward();
    let right = *transform.right();

    // Decompose planar velocity into forward + lateral parts.
    let v = lin_vel.0;
    let planar = Vec3::new(v.x, 0.0, v.z);
    let mut fwd_speed = planar.dot(forward);
    let mut lat_speed = planar.dot(right);

    // Throttle / brake / coast.
    if throttle.abs() > 0.01 {
        let opposing = throttle.signum() != fwd_speed.signum() && fwd_speed.abs() > 0.5;
        let accel = if opposing {
            params.brake_accel
        } else {
            params.engine_accel
        };
        fwd_speed += throttle * accel * dt;
    } else {
        let drag = params.coast_drag * dt;
        fwd_speed -= fwd_speed.clamp(-drag, drag);
    }
    fwd_speed = fwd_speed.clamp(-params.max_reverse_speed, params.max_speed);

    // Lateral grip: bleed sideways velocity like dry friction — a flat
    // acceleration budget, not a rate — so cornering force has a hard cap.
    // The budget fades from `grip` to `fast_grip` with speed (fast corners
    // drift), and the handbrake slashes it outright (deliberate drift).
    let speed_frac = (fwd_speed.abs() / params.max_speed).clamp(0.0, 1.0);
    let bleed_budget = if handbrake {
        params.handbrake_grip
    } else {
        params.grip + (params.fast_grip - params.grip) * speed_frac
    };
    let bleed = bleed_budget * dt;
    lat_speed -= lat_speed.clamp(-bleed, bleed);

    // Sliding scrubs speed: whatever sideways velocity survives the bleed
    // drags the forward component down. Grip driving keeps the slide tiny so
    // it costs ~nothing; drifts trade real speed for angle.
    let scrub = lat_speed.abs() * params.slide_scrub * dt;
    fwd_speed -= fwd_speed.clamp(-scrub, scrub);

    lin_vel.0 = forward * fwd_speed + right * lat_speed + Vec3::Y * v.y;

    // Steering: kinematic bicycle model. Yaw comes from geometry — the turn
    // radius is set by the wheel angle, yaw rate grows with speed, reversing
    // steers mirrored like a real car, and a stationary car cannot rotate.
    let steer_angle = steer * params.max_steer_angle;
    let geometric_yaw = -(fwd_speed / params.wheelbase) * steer_angle.tan();
    // ...until the tires run out: centripetal acceleration (v * yaw) is
    // capped by the grip budget, so overspeeding a corner pushes you wide
    // instead of magically rotating the car. (Full `grip` even while the
    // handbrake is held — the front wheels still steer you into the slide.)
    let yaw_cap = params.grip / fwd_speed.abs().max(0.1);
    // Pivot assist: lets a (nearly) stationary car turn to aim, fading out
    // as geometric steering takes over. Forward-sense, like inching forward
    // with the wheels turned.
    let pivot_fade = (1.0 - fwd_speed.abs() / PIVOT_FADE_SPEED).max(0.0);
    let pivot_yaw = -steer * params.pivot_yaw_rate * pivot_fade;
    let target_yaw = (geometric_yaw + pivot_yaw).clamp(-yaw_cap, yaw_cap);
    let blend = 1.0 - (-params.yaw_response * dt).exp();
    ang_vel.y += (target_yaw - ang_vel.y) * blend;
}
