//! Pedestrians that wander the map, flee from cars, and can be run over to create blood/gore.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z, BLOCKS};
use crate::nav::NavGrid;
use crate::vehicle::Car;
use crate::weapon::Lifetime;

const PEDESTRIAN_SPEED: f32 = 2.5;
const FLEE_SPEED: f32 = 4.5;
const FLEE_DISTANCE: f32 = 6.0;
const BLOOD_LIFETIME: f32 = 15.0;

#[derive(Component)]
pub struct Pedestrian {
    pub target: Option<Vec3>,
    pub path: Vec<Vec3>,
    pub idle_time: f32,
    pub repath_time: f32,
    pub speed: f32,
}

impl Default for Pedestrian {
    fn default() -> Self {
        Self {
            target: None,
            path: Vec::new(),
            idle_time: 0.0,
            repath_time: 0.0,
            speed: PEDESTRIAN_SPEED,
        }
    }
}

#[derive(Component)]
pub struct BloodSplat {
    pub remaining: f32,
}

#[derive(Resource)]
pub struct PedestrianAssets {
    pub body_mesh: Handle<Mesh>,
    pub head_mesh: Handle<Mesh>,
    pub shirt_material: Handle<StandardMaterial>,
    pub skin_material: Handle<StandardMaterial>,
    pub blood_mesh: Handle<Mesh>,
    pub blood_material: Handle<StandardMaterial>,
    pub blood_debris_material: Handle<StandardMaterial>,
}

pub struct PedestrianPlugin;

impl Plugin for PedestrianPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_pedestrian_assets)
            .add_systems(FixedUpdate, (pedestrian_ai, pedestrian_movement).chain())
            .add_systems(Update, (pedestrian_collisions, fade_blood_splats));
    }
}

fn setup_pedestrian_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PedestrianAssets {
        body_mesh: meshes.add(Cuboid::new(0.35, 0.6, 0.25)),
        head_mesh: meshes.add(Sphere::new(0.15)),
        shirt_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.7, 0.8), // Bright cyan shirt
            perceptual_roughness: 0.8,
            ..default()
        }),
        skin_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.75, 0.65), // Skin tone
            perceptual_roughness: 0.6,
            ..default()
        }),
        blood_mesh: meshes.add(Cuboid::new(1.0, 0.005, 1.0)),
        blood_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.65, 0.02, 0.02, 0.85), // Red blood
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        blood_debris_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.01, 0.01),
            perceptual_roughness: 0.9,
            ..default()
        }),
    });
}

pub fn spawn_pedestrians(
    commands: &mut Commands,
    assets: &PedestrianAssets,
    nav: &NavGrid,
    seed: f32,
) {
    let count = 10;
    for i in 0..count {
        let pos = nav.random_free_cell(seed + i as f32 * 4.3, i);
        // Spawn slightly above the ground
        let spawn_pos = Vec3::new(pos.x, 0.4, pos.z);

        commands
            .spawn((
                Name::new(format!("Pedestrian {i}")),
                Pedestrian::default(),
                Transform::from_translation(spawn_pos),
                (
                    RigidBody::Dynamic,
                    Collider::cuboid(0.35, 0.8, 0.25),
                    LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                    Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
                    Restitution::new(0.2).with_combine_rule(CoefficientCombine::Max),
                    Mass(1.0),
                    CollisionEventsEnabled,
                ),
            ))
            .with_children(|parent| {
                // Body/Shirt
                parent.spawn((
                    Mesh3d(assets.body_mesh.clone()),
                    MeshMaterial3d(assets.shirt_material.clone()),
                    Transform::from_xyz(0.0, 0.1, 0.0),
                ));
                // Head
                parent.spawn((
                    Mesh3d(assets.head_mesh.clone()),
                    MeshMaterial3d(assets.skin_material.clone()),
                    Transform::from_xyz(0.0, 0.5, 0.0),
                ));
            });
    }
}

fn pedestrian_ai(
    time: Res<Time>,
    nav: Res<NavGrid>,
    mut query: Query<(&mut Pedestrian, &Transform, &LinearVelocity)>,
    cars: Query<&Transform, With<Car>>,
) {
    let dt = time.delta_secs();
    let mut rng_seed = time.elapsed_secs();

    for (mut ped, transform, _velocity) in &mut query {
        let pos = transform.translation;

        // Stuck prevention: check if we have waypoints but are barely moving
        ped.repath_time -= dt;

        // Flee detection: run away if a car is close
        let mut nearest_car_pos: Option<Vec3> = None;
        let mut min_car_dist = FLEE_DISTANCE;
        for car_trans in &cars {
            let dist = pos.xz().distance(car_trans.translation.xz());
            if dist < min_car_dist {
                min_car_dist = dist;
                nearest_car_pos = Some(car_trans.translation);
            }
        }

        if let Some(car_pos) = nearest_car_pos {
            // Panic! Flee directly away from the nearest car
            let flee_dir = (pos - car_pos) * Vec3::new(1.0, 0.0, 1.0);
            let dir = flee_dir.normalize_or_zero();
            
            // Set flee target 6.0m away in that direction
            let target = pos + dir * 6.0;
            // Clamp to arena bounds
            let clamped_target = Vec3::new(
                target.x.clamp(-ARENA_HALF_X + 1.5, ARENA_HALF_X - 1.5),
                pos.y,
                target.z.clamp(-ARENA_HALF_Z + 1.5, ARENA_HALF_Z - 1.5),
            );

            ped.target = Some(clamped_target);
            ped.path.clear(); // Clear normal path
            ped.speed = FLEE_SPEED;
            ped.idle_time = 0.0;
            continue;
        }

        // Normal wandering behavior
        ped.speed = PEDESTRIAN_SPEED;

        if ped.idle_time > 0.0 {
            ped.idle_time -= dt;
            ped.target = None;
            continue;
        }

        // If we reached the target or don't have one, pick a new one
        let needs_target = ped.target.map_or(true, |t| pos.xz().distance(t.xz()) < 0.8);
        if needs_target {
            if ped.path.is_empty() {
                // Pause briefly before walking to the next spot
                ped.idle_time = 1.0 + (rng_seed * 7.1).fract().abs() * 2.0;
                rng_seed += 1.0;

                // Pick a new wander target
                let wander_pos = nav.random_free_cell(rng_seed, 0);
                ped.path = nav.find_path(pos, wander_pos).unwrap_or_default();
            }

            if !ped.path.is_empty() {
                let next_wp = ped.path.remove(0);
                ped.target = Some(next_wp);
            } else {
                ped.target = None;
            }
        } else if ped.repath_time <= 0.0 && !ped.path.is_empty() {
            // Periodic path verification/smoothing
            ped.repath_time = 0.5;
            // Check if we can skip some waypoints if we have line of sight
            while ped.path.len() > 1 && nav.line_of_sight(pos, ped.path[1]) {
                ped.path.remove(0);
            }
            if !ped.path.is_empty() {
                ped.target = Some(ped.path[0]);
            }
        }
    }
}

fn pedestrian_movement(
    mut query: Query<(&Pedestrian, &mut Transform, &mut LinearVelocity)>,
) {
    for (ped, mut transform, mut velocity) in &mut query {
        if let Some(target) = ped.target {
            let to_target = (target - transform.translation) * Vec3::new(1.0, 0.0, 1.0);
            let dist = to_target.length();
            if dist > 0.1 {
                let dir = to_target / dist;
                velocity.x = dir.x * ped.speed;
                velocity.z = dir.z * ped.speed;

                // Rotate to face direction of movement
                let yaw = (-dir.x).atan2(-dir.z);
                transform.rotation = Quat::from_rotation_y(yaw);
            }
        } else {
            // Stop moving when idling
            velocity.x = 0.0;
            velocity.z = 0.0;
        }
    }
}

fn pedestrian_collisions(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    pedestrians: Query<Entity, With<Pedestrian>>,
    cars: Query<&LinearVelocity, With<Car>>,
    assets: Res<PedestrianAssets>,
) {
    let mut rng_val = 0.0;
    for event in collisions.read() {
        let a = event.body1.unwrap_or(event.collider1);
        let b = event.body2.unwrap_or(event.collider2);

        let pairs = [(a, b), (b, a)];
        for (ped_entity, car_entity) in pairs {
            if !pedestrians.contains(ped_entity) {
                continue;
            }
            let Ok(car_vel) = cars.get(car_entity) else {
                continue;
            };

            // Pedestrian hit! Despawn immediately
            commands.entity(ped_entity).try_despawn();

            // Get hit location
            let hit_pos = event.point1.unwrap_or(Vec3::new(0.0, 0.4, 0.0));

            // 1. Gore burst (debris giblets)
            let vel = car_vel.0;
            let forward_dir = vel.normalize_or_zero();
            let speed = vel.length().max(5.0);

            for i in 0..8 {
                let angle = i as f32 * 0.785 + rng_val;
                let spread_vel = Vec3::new(
                    forward_dir.x * speed + angle.cos() * 3.0,
                    2.0 + (i % 3) as f32 * 1.5,
                    forward_dir.z * speed + angle.sin() * 3.0,
                );

                commands.spawn((
                    Name::new("Gore Debris"),
                    Mesh3d(assets.blood_mesh.clone()), // reuse blood quad or create a tiny block
                    MeshMaterial3d(assets.blood_debris_material.clone()),
                    Transform::from_translation(hit_pos + Vec3::Y * 0.2)
                        .with_scale(Vec3::splat(0.12)),
                    RigidBody::Dynamic,
                    Collider::cuboid(0.12, 0.12, 0.12),
                    Mass(0.1),
                    LinearVelocity(spread_vel),
                    Lifetime(2.5),
                ));
            }

            // 2. Spawn ground blood splats (3-4 overlapping splats)
            for i in 0..4 {
                let offset_x = (i as f32 * 3.14).cos() * 0.4;
                let offset_z = (i as f32 * 3.14).sin() * 0.4;
                let splat_pos = Vec3::new(hit_pos.x + offset_x, 0.008, hit_pos.z + offset_z);
                let rotation = Quat::from_rotation_y(i as f32 * 1.23);
                let scale = 0.5 + (i % 3) as f32 * 0.35;

                commands.spawn((
                    Name::new("Blood Splat Ground"),
                    BloodSplat {
                        remaining: BLOOD_LIFETIME,
                    },
                    Mesh3d(assets.blood_mesh.clone()),
                    MeshMaterial3d(assets.blood_material.clone()),
                    Transform {
                        translation: splat_pos,
                        rotation,
                        scale: Vec3::new(scale, 1.0, scale),
                    },
                ));
            }

            // 3. Wall splat check
            // Project a ray in vehicle travel direction
            if speed > 2.0 {
                let wall_check_pos = hit_pos + forward_dir * 3.5;
                if let Some((surf_pt, surf_norm)) = find_closest_surface(wall_check_pos) {
                    let dist = surf_pt.distance(hit_pos);
                    // If the wall surface is close to the splash path (within ~4.5m of the hit)
                    if dist < 4.5 {
                        let rotation = Quat::from_rotation_arc(Vec3::Y, surf_norm);
                        let scale_x = 0.6 + rng_val.cos().abs() * 0.6;
                        let scale_z = 0.6 + rng_val.sin().abs() * 0.6;
                        
                        commands.spawn((
                            Name::new("Blood Splat Wall"),
                            BloodSplat {
                                remaining: BLOOD_LIFETIME,
                            },
                            Mesh3d(assets.blood_mesh.clone()),
                            MeshMaterial3d(assets.blood_material.clone()),
                            Transform {
                                translation: surf_pt + surf_norm * 0.008,
                                rotation,
                                scale: Vec3::new(scale_x, 1.0, scale_z),
                            },
                        ));
                    }
                }
            }

            rng_val += 1.0;
            break; // Stop processing this event once the pedestrian is run over
        }
    }
}

fn find_closest_surface(pos: Vec3) -> Option<(Vec3, Vec3)> {
    let mut closest_point = Vec3::ZERO;
    let mut closest_normal = Vec3::Y;
    let mut min_dist = f32::MAX;

    // Check bounding walls
    // Left wall: x = -ARENA_HALF_X
    let dist_left = (pos.x - (-ARENA_HALF_X)).abs();
    if dist_left < min_dist {
        min_dist = dist_left;
        closest_point = Vec3::new(-ARENA_HALF_X, pos.y, pos.z);
        closest_normal = Vec3::X;
    }
    // Right wall: x = ARENA_HALF_X
    let dist_right = (pos.x - ARENA_HALF_X).abs();
    if dist_right < min_dist {
        min_dist = dist_right;
        closest_point = Vec3::new(ARENA_HALF_X, pos.y, pos.z);
        closest_normal = -Vec3::X;
    }
    // Back wall: z = -ARENA_HALF_Z
    let dist_back = (pos.z - (-ARENA_HALF_Z)).abs();
    if dist_back < min_dist {
        min_dist = dist_back;
        closest_point = Vec3::new(pos.x, pos.y, -ARENA_HALF_Z);
        closest_normal = Vec3::Z;
    }
    // Front wall: z = ARENA_HALF_Z
    let dist_front = (pos.z - ARENA_HALF_Z).abs();
    if dist_front < min_dist {
        min_dist = dist_front;
        closest_point = Vec3::new(pos.x, pos.y, ARENA_HALF_Z);
        closest_normal = -Vec3::Z;
    }

    // Check building blocks
    for &(center, dim) in &BLOCKS {
        let half_x = dim.x / 2.0;
        let half_z = dim.z / 2.0;
        let min_x = center.x - half_x;
        let max_x = center.x + half_x;
        let min_z = center.z - half_z;
        let max_z = center.z + half_z;

        // Projection of pos onto the AABB
        let px = pos.x.clamp(min_x, max_x);
        let pz = pos.z.clamp(min_z, max_z);
        let py = pos.y.clamp(0.0, dim.y); // buildings start at ground

        let projected = Vec3::new(px, py, pz);
        let dist = pos.distance(projected);

        // If the position is outside the block or very close, calculate normal
        if dist < min_dist {
            min_dist = dist;
            closest_point = projected;

            // Find closest face of the block
            let dx_min = (pos.x - min_x).abs();
            let dx_max = (pos.x - max_x).abs();
            let dz_min = (pos.z - min_z).abs();
            let dz_max = (pos.z - max_z).abs();
            let dy_max = (pos.y - dim.y).abs();

            let mut d_min = dx_min;
            closest_normal = -Vec3::X;

            if dx_max < d_min {
                d_min = dx_max;
                closest_normal = Vec3::X;
            }
            if dz_min < d_min {
                d_min = dz_min;
                closest_normal = -Vec3::Z;
            }
            if dz_max < d_min {
                d_min = dz_max;
                closest_normal = Vec3::Z;
            }
            if dy_max < d_min {
                closest_normal = Vec3::Y;
            }
        }
    }

    if min_dist < f32::MAX {
        Some((closest_point, closest_normal))
    } else {
        None
    }
}

fn fade_blood_splats(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<PedestrianAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut splats: Query<(Entity, &mut BloodSplat, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();

    for (entity, mut splat, mut mat_handle) in &mut splats {
        splat.remaining -= dt;
        if splat.remaining <= 0.0 {
            commands.entity(entity).try_despawn();
            continue;
        }

        let alpha = (splat.remaining / BLOOD_LIFETIME).clamp(0.0, 1.0) * 0.85;

        // Clone material on first fade tick (when it's still the shared handle)
        if mat_handle.0 == assets.blood_material {
            let mut new_mat = materials.get(&assets.blood_material).unwrap().clone();
            new_mat.base_color = Color::srgba(0.65, 0.02, 0.02, alpha);
            mat_handle.0 = materials.add(new_mat);
        } else if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.base_color = Color::srgba(0.65, 0.02, 0.02, alpha);
        }
    }
}
