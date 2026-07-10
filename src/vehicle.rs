//! Arcade toy-car driving: the simplified plane-force model from design §9,
//! plus car health and the floating health bars.
//!
//! The car is a dynamic rigid body with X/Z rotation locked (stays flat).
//! Each fixed tick we decompose planar velocity into forward/lateral parts,
//! apply throttle/brake to the forward part, bleed the lateral part with a
//! grip factor (weaken it for handbrake drifts), and set yaw rate directly
//! from steering scaled by speed. Simple, fully tunable, no suspension sim.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z};
use crate::input::{self, CarAction};
use crate::weapon::WeaponSlot;

/// Arcade handling parameters; players and cops share the model, not the numbers.
pub struct DriveParams {
    pub max_speed: f32,
    pub max_reverse_speed: f32,
    pub engine_accel: f32,
    pub brake_accel: f32,
    /// Passive deceleration when coasting (m/s^2).
    pub coast_drag: f32,
    /// Lateral-velocity bleed rate (1/s) when driving slowly. High = grippy.
    pub grip_low_speed: f32,
    /// Lateral-velocity bleed rate at max speed: lower than `grip_low_speed`,
    /// so fast cornering drifts wide before the velocity catches the heading.
    pub grip_high_speed: f32,
    /// Grip while the handbrake is held: low = big slidey drifts.
    pub handbrake_grip: f32,
    /// Max yaw rate in rad/s at full steering lock.
    pub max_yaw_rate: f32,
    /// Fraction of max_speed at which steering reaches full authority.
    pub full_steer_at: f32,
    /// How quickly the actual yaw rate follows the steering input (1/s).
    /// Lower = smoother, heavier turn-in; higher = twitchier.
    pub yaw_response: f32,
    /// Floor on the speed-based steering authority fraction (0..1). Stops
    /// steering from going dead at standstill while high-speed grip fade still
    /// enables corner slides.
    pub min_steer_authority: f32,
    /// Steering authority fraction left at max speed (0..1). Below 1.0,
    /// steering gets calmer the faster you go — tight turns want braking
    /// first, and small corrections at speed stop overshooting.
    pub high_speed_steer: f32,
}

pub const PLAYER_DRIVE: DriveParams = DriveParams {
    max_speed: 18.0,
    max_reverse_speed: 8.0,
    engine_accel: 42.0,
    brake_accel: 58.0,
    coast_drag: 5.5,
    grip_low_speed: 9.0,
    grip_high_speed: 4.5,
    handbrake_grip: 1.0,
    max_yaw_rate: 4.8,
    full_steer_at: 0.2,
    yaw_response: 15.0,
    min_steer_authority: 0.65,
    high_speed_steer: 0.75,
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
            .add_systems(Update, update_health_bars)
            .add_systems(FixedUpdate, drive_cars);
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
        chassis: meshes.add(Cuboid::new(1.0, 0.4, 2.0)),
        cabin: meshes.add(Cuboid::new(0.8, 0.35, 0.9)),
        wheel: meshes.add(Cuboid::new(0.2, 0.35, 0.35)),
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
pub fn spawn_car(commands: &mut Commands, assets: &CarAssets, slot: &PlayerSlot, position: usize) {
    // Spawn points on an ellipse around the center, facing inward.
    let angle = position as f32 * std::f32::consts::TAU / PLAYER_COLORS.len() as f32;
    let pos = Vec3::new(
        angle.cos() * ARENA_HALF_X * 0.7,
        0.0,
        angle.sin() * ARENA_HALF_Z * 0.7,
    );
    let body = assets.body_materials[slot.color_index % PLAYER_COLORS.len()].clone();

    let mut car_entity = commands
        .spawn((
            Name::new(format!("Car P{}", slot.id + 1)),
            Car,
            Player {
                id: slot.id,
                color: slot.color_index,
            },
            Health {
                current: MAX_HEALTH,
                max: MAX_HEALTH,
            },
            WeaponSlot::default(),
            Mesh3d(assets.chassis.clone()),
            MeshMaterial3d(body.clone()),
            Transform::from_translation(pos + Vec3::Y * 0.6)
                .looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y),
            // Nested: bundles cap at 16 top-level elements.
            (
                RigidBody::Dynamic,
                Collider::cuboid(1.0, 0.4, 2.0),
                LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                // Frictionless contacts: grip lives in the drive model, and
                // scraping a wall must not glue the car to it.
                Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
                // Springy: wall hits bounce you off instead of pinning you.
                Restitution::new(0.6).with_combine_rule(CoefficientCombine::Max),
                Mass(6.0),
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
            // Cabin.
            parent.spawn((
                Mesh3d(assets.cabin.clone()),
                MeshMaterial3d(body),
                Transform::from_xyz(0.0, 0.35, 0.15),
            ));
            // Wheels (cosmetic).
            for (x, z) in [(-0.55, -0.6), (0.55, -0.6), (-0.55, 0.6), (0.55, 0.6)] {
                parent.spawn((
                    Mesh3d(assets.wheel.clone()),
                    MeshMaterial3d(assets.wheel_material.clone()),
                    Transform::from_xyz(x, -0.1, z),
                ));
            }
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

    // Lateral grip: bleed sideways velocity; handbrake lets it live (drift).
    // Grip fades with speed, so fast corners slide before they stick.
    let speed_frac = (fwd_speed.abs() / params.max_speed).clamp(0.0, 1.0);
    let grip = if handbrake {
        params.handbrake_grip
    } else {
        params.grip_low_speed + (params.grip_high_speed - params.grip_low_speed) * speed_frac
    };
    lat_speed *= (1.0 - grip * dt).max(0.0);

    lin_vel.0 = forward * fwd_speed + right * lat_speed + Vec3::Y * v.y;

    // Steering: authority ramps up with speed, then tapers back off toward
    // `high_speed_steer` at top speed (fast = calm, tight turns need braking).
    // Left/right stay consistent in reverse — no sign flip anywhere, so
    // there's nothing to twist the car on its own and pivoting in place
    // always works. Yaw rate eases toward the target instead of snapping.
    let ramp_up = (fwd_speed.abs() / (params.max_speed * params.full_steer_at))
        .clamp(params.min_steer_authority, 1.0);
    let taper = 1.0 - (1.0 - params.high_speed_steer) * speed_frac;
    let authority = ramp_up * taper;
    let target_yaw = -steer * params.max_yaw_rate * authority;
    let blend = 1.0 - (-params.yaw_response * dt).exp();
    ang_vel.y += (target_yaw - ang_vel.y) * blend;
}
