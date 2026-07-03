//! Arcade toy-car driving: the simplified plane-force model from design §9.
//!
//! The car is a dynamic rigid body with X/Z rotation locked (stays flat).
//! Each fixed tick we decompose planar velocity into forward/lateral parts,
//! apply throttle/brake to the forward part, bleed the lateral part with a
//! grip factor (weaken it for handbrake drifts), and set yaw rate directly
//! from steering scaled by speed. Simple, fully tunable, no suspension sim.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::arena::ARENA_HALF;
use crate::input::{self, CarAction};

// --- Handling tuning. M1's whole job is playing with these. ---
const MAX_SPEED: f32 = 18.0;
const MAX_REVERSE_SPEED: f32 = 8.0;
const ENGINE_ACCEL: f32 = 28.0;
const BRAKE_ACCEL: f32 = 50.0;
/// Passive deceleration when coasting (m/s^2).
const COAST_DRAG: f32 = 6.0;
/// How fast lateral (sideways) velocity is bled off, per second. High = grippy.
const GRIP: f32 = 12.0;
/// Grip while the handbrake is held: low = big slidey drifts.
const HANDBRAKE_GRIP: f32 = 1.5;
/// Max yaw rate in rad/s at full steering lock.
const MAX_YAW_RATE: f32 = 2.8;
/// Fraction of MAX_SPEED at which steering reaches full authority.
const FULL_STEER_AT: f32 = 0.3;

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

#[derive(Component)]
pub struct Player {
    pub id: usize,
}

/// Tracks which gamepad entities already own a car.
#[derive(Resource, Default)]
struct Roster {
    assigned_gamepads: Vec<Entity>,
    next_player: usize,
}

/// Shared meshes/materials for spawning cars.
#[derive(Resource)]
struct CarAssets {
    chassis: Handle<Mesh>,
    cabin: Handle<Mesh>,
    wheel: Handle<Mesh>,
    wheel_material: Handle<StandardMaterial>,
    body_materials: Vec<Handle<StandardMaterial>>,
}

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Roster>()
            .add_systems(Startup, (setup_car_assets, spawn_keyboard_player).chain())
            .add_systems(Update, gamepad_join)
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
        wheel_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.12, 0.12),
            perceptual_roughness: 0.9,
            ..default()
        }),
        body_materials,
    });
}

fn spawn_keyboard_player(mut commands: Commands, assets: Res<CarAssets>, mut roster: ResMut<Roster>) {
    let id = roster.next_player;
    roster.next_player += 1;
    spawn_car(&mut commands, &assets, id, input::keyboard_map());
}

/// Press South (A / Cross) on an unassigned pad to join with a new car.
fn gamepad_join(
    mut commands: Commands,
    gamepads: Query<(Entity, &Gamepad)>,
    assets: Res<CarAssets>,
    mut roster: ResMut<Roster>,
) {
    for (pad_entity, gamepad) in &gamepads {
        if roster.next_player < PLAYER_COLORS.len()
            && gamepad.just_pressed(GamepadButton::South)
            && !roster.assigned_gamepads.contains(&pad_entity)
        {
            let id = roster.next_player;
            roster.next_player += 1;
            roster.assigned_gamepads.push(pad_entity);
            spawn_car(&mut commands, &assets, id, input::gamepad_map(pad_entity));
            info!("Player {} joined on gamepad {pad_entity:?}", id + 1);
        }
    }
}

fn spawn_car(
    commands: &mut Commands,
    assets: &CarAssets,
    player_id: usize,
    input_map: InputMap<CarAction>,
) {
    // Spawn points ring the center, facing inward.
    let angle = player_id as f32 * std::f32::consts::TAU / PLAYER_COLORS.len() as f32;
    let pos = Vec3::new(angle.cos(), 0.0, angle.sin()) * (ARENA_HALF * 0.6);
    let body = assets.body_materials[player_id % PLAYER_COLORS.len()].clone();

    commands
        .spawn((
            Name::new(format!("Car P{}", player_id + 1)),
            Car,
            Player { id: player_id },
            Mesh3d(assets.chassis.clone()),
            MeshMaterial3d(body.clone()),
            Transform::from_translation(pos + Vec3::Y * 0.6)
                .looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y),
            RigidBody::Dynamic,
            Collider::cuboid(1.0, 0.4, 2.0),
            LockedAxes::new().lock_rotation_x().lock_rotation_z(),
            Friction::new(0.1),
            Restitution::new(0.2),
            Mass(6.0),
            input_map,
            ActionState::<CarAction>::default(),
        ))
        .with_children(|parent| {
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
}

fn drive_cars(
    time: Res<Time>,
    mut cars: Query<
        (
            &ActionState<CarAction>,
            &Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<Car>,
    >,
) {
    let dt = time.delta_secs();
    for (actions, transform, mut lin_vel, mut ang_vel) in &mut cars {
        let throttle = actions.clamped_value(&CarAction::Throttle);
        let steer = actions.clamped_value(&CarAction::Steer);
        let handbrake = actions.pressed(&CarAction::Handbrake);

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
            let accel = if opposing { BRAKE_ACCEL } else { ENGINE_ACCEL };
            fwd_speed += throttle * accel * dt;
        } else {
            let drag = COAST_DRAG * dt;
            fwd_speed -= fwd_speed.clamp(-drag, drag);
        }
        fwd_speed = fwd_speed.clamp(-MAX_REVERSE_SPEED, MAX_SPEED);

        // Lateral grip: bleed sideways velocity; handbrake lets it live (drift).
        let grip = if handbrake { HANDBRAKE_GRIP } else { GRIP };
        lat_speed *= (1.0 - grip * dt).max(0.0);

        lin_vel.0 = forward * fwd_speed + right * lat_speed + Vec3::Y * v.y;

        // Steering: authority ramps up with speed, flips when reversing.
        let authority = (fwd_speed.abs() / (MAX_SPEED * FULL_STEER_AT)).clamp(0.0, 1.0);
        let direction = if fwd_speed < -0.5 { -1.0 } else { 1.0 };
        ang_vel.y = -steer * MAX_YAW_RATE * authority * direction;
    }
}
