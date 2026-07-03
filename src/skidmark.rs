//! Skid marks: dark rubber streaks left on the ground when a car drifts.
//!
//! Each tick, if a car's lateral (sideways) speed exceeds a threshold we spawn
//! a thin quad behind each rear wheel, just above the ground plane.  Each
//! segment stretches from the previous wheel position to the current one so
//! the trail is continuous.  Marks fade out over their lifetime then despawn.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::vehicle::Car;

/// Lateral speed (m/s) above which skid marks appear.
const DRIFT_THRESHOLD: f32 = 2.5;
/// How long a skid mark lives before fully fading (seconds).
const MARK_LIFETIME: f32 = 3.0;
/// How often we can spawn a new pair of marks per car (seconds).
const SPAWN_INTERVAL: f32 = 0.03;
/// Half-width of the rubber streak.
const MARK_HALF_WIDTH: f32 = 0.07;
/// Height above ground (avoid z-fighting with the floor).
const MARK_Y: f32 = 0.005;

/// Per-car state: cooldown timer + last world-space position of each rear
/// wheel, so consecutive segments can be connected.
#[derive(Component)]
struct SkidState {
    cooldown: f32,
    /// `None` when the car wasn't drifting last tick (trail start).
    prev_left: Option<Vec3>,
    prev_right: Option<Vec3>,
}

/// Fading skid-mark segment.
#[derive(Component)]
struct SkidMark {
    remaining: f32,
}

/// Shared mesh + material for all skid marks.  The mesh is a 1×1 unit quad
/// (X = width, Z = length) that gets non-uniformly scaled per-segment.
#[derive(Resource)]
struct SkidAssets {
    /// A 1-unit-long, 1-unit-wide, very thin cuboid.
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub struct SkidMarkPlugin;

impl Plugin for SkidMarkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_skid_assets)
            .add_systems(Update, (spawn_skid_marks, fade_skid_marks));
    }
}

fn setup_skid_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(SkidAssets {
        // Unit cuboid: 1.0 wide (X), very thin (Y), 1.0 long (Z).
        // Each segment will be scaled on X and Z to match the desired width
        // and the distance between prev/current wheel positions.
        mesh: meshes.add(Cuboid::new(1.0, 0.005, 1.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.05, 0.05, 0.05, 0.7),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
}

/// Compute the world-space position of a rear wheel given the car transform.
fn rear_wheel_world(transform: &Transform, wheel_x: f32) -> Vec3 {
    let right = *transform.right();
    let forward = *transform.forward();
    let pos = transform.translation;
    let offset = right * wheel_x + forward * (-0.6);
    Vec3::new(pos.x + offset.x, MARK_Y, pos.z + offset.z)
}

fn spawn_skid_marks(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidAssets>,
    mut cars: Query<
        (Entity, &Transform, &LinearVelocity),
        With<Car>,
    >,
    mut states: Query<&mut SkidState>,
) {
    let dt = time.delta_secs();

    for (entity, transform, lin_vel) in &mut cars {
        // Measure lateral speed.
        let right = *transform.right();
        let v = lin_vel.0;
        let planar = Vec3::new(v.x, 0.0, v.z);
        let lat_speed = planar.dot(right).abs();

        let drifting = lat_speed >= DRIFT_THRESHOLD;

        // Ensure the car has SkidState.
        let has_state = states.get(entity).is_ok();
        if !has_state {
            commands.entity(entity).try_insert(SkidState {
                cooldown: 0.0,
                prev_left: None,
                prev_right: None,
            });
            continue; // state won't be queryable until next frame
        }

        let mut state = states.get_mut(entity).unwrap();

        if !drifting {
            // Reset trail start so the next drift begins a fresh segment.
            state.prev_left = None;
            state.prev_right = None;
            continue;
        }

        // Tick cooldown.
        state.cooldown -= dt;
        if state.cooldown > 0.0 {
            continue;
        }
        state.cooldown = SPAWN_INTERVAL;

        // Current rear wheel positions (world space).
        let cur_left = rear_wheel_world(transform, -0.55);
        let cur_right = rear_wheel_world(transform, 0.55);

        // Spawn a connecting segment for each wheel that has a previous pos.
        for (prev_opt, cur) in [
            (state.prev_left, cur_left),
            (state.prev_right, cur_right),
        ] {
            if let Some(prev) = prev_opt {
                let delta = cur - prev;
                let length = delta.length();
                if length > 0.001 {
                    let midpoint = (prev + cur) * 0.5;
                    // Orient the quad so its local Z axis points along the
                    // segment direction (prev → cur) on the XZ plane.
                    let dir = delta / length;
                    let angle = (-dir.x).atan2(-dir.z); // yaw around Y

                    commands.spawn((
                        Name::new("Skid mark"),
                        SkidMark {
                            remaining: MARK_LIFETIME,
                        },
                        Mesh3d(assets.mesh.clone()),
                        MeshMaterial3d(assets.material.clone()),
                        Transform {
                            translation: midpoint,
                            rotation: Quat::from_rotation_y(angle),
                            scale: Vec3::new(
                                MARK_HALF_WIDTH * 2.0,
                                1.0,
                                length,
                            ),
                        },
                    ));
                }
            }
        }

        // Remember current positions for next tick.
        state.prev_left = Some(cur_left);
        state.prev_right = Some(cur_right);
    }
}

fn fade_skid_marks(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut marks: Query<(Entity, &mut SkidMark, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();

    // Each mark gets its own material clone on first fade tick so we can set
    // individual alpha.  Total mark count is bounded by
    // MARK_LIFETIME / SPAWN_INTERVAL * num_cars * 2 wheels ≈ 400, acceptable.
    for (entity, mut mark, mut mat_handle) in &mut marks {
        mark.remaining -= dt;
        if mark.remaining <= 0.0 {
            commands.entity(entity).try_despawn();
            continue;
        }

        let alpha = (mark.remaining / MARK_LIFETIME).clamp(0.0, 1.0) * 0.7;

        // Clone material on first fade tick (when it's still the shared handle).
        if mat_handle.0 == assets.material {
            let mut new_mat = materials.get(&assets.material).unwrap().clone();
            new_mat.base_color = Color::srgba(0.05, 0.05, 0.05, alpha);
            mat_handle.0 = materials.add(new_mat);
        } else if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.base_color = Color::srgba(0.05, 0.05, 0.05, alpha);
        }
    }
}
