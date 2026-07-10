//! The police (design §4, §8): AI-driven patrol cars that ram players.
//!
//! Cops don't shoot — they're heavier than player cars, so getting hit (or
//! ramming one) hurts. One cop spawns at round start; wrecking a cop spawns
//! TWO replacements (the "hydra rule"), capped so rounds stay playable.
//!
//! AI is deliberately simple and legible: chase the nearest living player
//! at full throttle, steer proportionally, and back out when stuck.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::nav::NavGrid;
use crate::vehicle::{self, apply_drive, Car, CarAssets, DriveParams, Health, Player};
use crate::weapon::{DamageFlash, Lifetime};

/// Heavier and a touch slower than players: escapable, but bad to touch.
const COP_DRIVE: DriveParams = DriveParams {
    max_speed: 15.5,
    max_reverse_speed: 7.0,
    engine_accel: 22.0,
    brake_accel: 40.0,
    coast_drag: 6.0,
    // Longer cruiser (chassis 2.2): wider minimum turn than the players.
    wheelbase: 1.3,
    max_steer_angle: 0.42,
    grip: 42.0,           // grippier than players: predictable pursuit
    fast_grip: 42.0,      // no speed fade: cops corner on rails
    handbrake_grip: 42.0, // cops never drift
    yaw_response: 10.0,
    slide_scrub: 0.7,
    pivot_yaw_rate: 0.0, // cops always chase on the move; no pivot assist
};

const COP_HEALTH: f32 = 60.0;
const COP_MASS: f32 = 14.0;
/// Wrecking one cop spawns this many replacements.
const REPLACEMENTS_PER_WRECK: usize = 2;
/// Hard cap on simultaneous cops (bounded by short rounds, design §4).
const MAX_COPS: usize = 9;
/// Ram damage: gentle touches (below the speed threshold) are free; above it,
/// damage scales with closing speed but is clamped into a narrow band so rams
/// are consistent rather than swingy.
const RAM_SPEED_THRESHOLD: f32 = 4.0;
const RAM_DAMAGE_SCALE: f32 = 1.5;
const RAM_MIN_DAMAGE: f32 = 8.0;
const RAM_MAX_DAMAGE: f32 = 22.0;
/// Cops hit players twice as hard as players hit cops (they're the heavies):
/// the cop takes half of whatever it dealt.
const RAM_SELF_DAMAGE: f32 = 0.5;

/// Spawn locations along the arena edges (indexed pseudo-randomly).
const COP_SPAWN_POINTS: [Vec3; 6] = [
    Vec3::new(-30.0, 0.6, -17.0),
    Vec3::new(30.0, 0.6, -17.0),
    Vec3::new(-30.0, 0.6, 17.0),
    Vec3::new(30.0, 0.6, 17.0),
    Vec3::new(0.0, 0.6, -18.0),
    Vec3::new(0.0, 0.6, 18.0),
];

#[derive(Component, Default)]
pub struct CopCar {
    throttle: f32,
    steer: f32,
    stuck_time: f32,
    reversing_time: f32,
    /// Remaining A* waypoints toward the current target (world space).
    path: Vec<Vec3>,
    /// Countdown to the next repath.
    repath_time: f32,
}

#[derive(Resource)]
pub struct CopAssets {
    chassis: Handle<Mesh>,
    cabin: Handle<Mesh>,
    wheel: Handle<Mesh>,
    light: Handle<Mesh>,
    body_material: Handle<StandardMaterial>,
    cabin_material: Handle<StandardMaterial>,
    wheel_material: Handle<StandardMaterial>,
    red_light: Handle<StandardMaterial>,
    blue_light: Handle<StandardMaterial>,
}

pub struct CopPlugin;

impl Plugin for CopPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_cop_assets)
            .add_systems(FixedUpdate, (cop_ai, cop_drive).chain())
            .add_systems(Update, (cop_rams, wreck_cops));
    }
}

fn setup_cop_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CopAssets {
        chassis: meshes.add(Cuboid::new(1.27, 0.52, 2.53)),
        cabin: meshes.add(Cuboid::new(1.04, 0.46, 1.15)),
        wheel: meshes.add(Cuboid::new(0.23, 0.46, 0.46)),
        light: meshes.add(Cuboid::new(0.25, 0.14, 0.35)),
        body_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.92, 0.92, 0.95),
            perceptual_roughness: 0.35,
            ..default()
        }),
        cabin_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.08, 0.1),
            perceptual_roughness: 0.5,
            ..default()
        }),
        wheel_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.12, 0.12),
            perceptual_roughness: 0.9,
            ..default()
        }),
        red_light: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.1, 0.1),
            emissive: LinearRgba::rgb(6.0, 0.2, 0.2),
            unlit: true,
            ..default()
        }),
        blue_light: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.3, 1.0),
            emissive: LinearRgba::rgb(0.4, 0.8, 8.0),
            unlit: true,
            ..default()
        }),
    });
}

/// Pick a pseudo-random spawn point (no rand dependency; time-seeded).
pub fn pick_spawn_point(seed: f32, salt: usize) -> Vec3 {
    let index = (seed * 13.7) as usize + salt * 3 + 1;
    COP_SPAWN_POINTS[index % COP_SPAWN_POINTS.len()]
}

pub fn spawn_cop(
    commands: &mut Commands,
    assets: &CopAssets,
    car_assets: &CarAssets,
    pos: Vec3,
) {
    let cop = commands
        .spawn((
            Name::new("Cop"),
            CopCar::default(),
            Health {
                current: COP_HEALTH,
                max: COP_HEALTH,
            },
            Mesh3d(assets.chassis.clone()),
            MeshMaterial3d(assets.body_material.clone()),
            Transform::from_translation(pos).looking_at(Vec3::new(0.0, pos.y, 0.0), Vec3::Y),
            (
                RigidBody::Dynamic,
                Collider::cuboid(1.27, 0.52, 2.53),
                LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
                Restitution::new(0.5).with_combine_rule(CoefficientCombine::Max),
                Mass(COP_MASS),
                CollisionEventsEnabled,
            ),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.cabin.clone()),
                MeshMaterial3d(assets.cabin_material.clone()),
                Transform::from_xyz(0.0, 0.46, 0.23),
            ));
            // Roof light bar: red + blue.
            parent.spawn((
                Mesh3d(assets.light.clone()),
                MeshMaterial3d(assets.red_light.clone()),
                Transform::from_xyz(-0.15, 0.76, 0.23),
            ));
            parent.spawn((
                Mesh3d(assets.light.clone()),
                MeshMaterial3d(assets.blue_light.clone()),
                Transform::from_xyz(0.15, 0.76, 0.23),
            ));
            for (x, z) in [(-0.69, -0.75), (0.69, -0.75), (-0.69, 0.75), (0.69, 0.75)] {
                parent.spawn((
                    Mesh3d(assets.wheel.clone()),
                    MeshMaterial3d(assets.wheel_material.clone()),
                    Transform::from_xyz(x, -0.12, z),
                ));
            }
        })
        .id();

    vehicle::spawn_health_bar(commands, car_assets, cop, pos);
}

/// Decide throttle/steer for each cop: chase the nearest living player along
/// an A* path (straight-line when it has line of sight), back out when stuck.
fn cop_ai(
    time: Res<Time>,
    nav: Res<NavGrid>,
    mut cops: Query<(&mut CopCar, &Transform, &LinearVelocity)>,
    players: Query<&Transform, (With<Car>, With<Player>, Without<CopCar>)>,
) {
    let dt = time.delta_secs();
    for (mut cop, transform, velocity) in &mut cops {
        let pos = transform.translation;
        // Nearest player, planar distance.
        let target = players.iter().min_by(|a, b| {
            let da = (a.translation - pos).xz().length_squared();
            let db = (b.translation - pos).xz().length_squared();
            da.total_cmp(&db)
        });
        let Some(target) = target else {
            // Nobody left to chase.
            cop.throttle = 0.0;
            cop.steer = 0.0;
            continue;
        };

        if cop.reversing_time > 0.0 {
            // Unsticking: back out with wheels turned.
            cop.reversing_time -= dt;
            cop.throttle = -1.0;
            cop.steer = 0.7;
            continue;
        }

        // Repath periodically (or when the path ran out).
        cop.repath_time -= dt;
        if cop.repath_time <= 0.0 || cop.path.is_empty() {
            cop.repath_time = 0.4;
            cop.path = if nav.line_of_sight(pos, target.translation) {
                Vec::new() // straight shot; no waypoints needed
            } else {
                nav.find_path(pos, target.translation).unwrap_or_default()
            };
        }

        // Drop waypoints we've reached, then steer at the furthest one we can
        // see (string-pulling lite); fall back to the player itself.
        while cop
            .path
            .first()
            .is_some_and(|w| (*w - pos).xz().length() < 2.2)
        {
            cop.path.remove(0);
        }
        let steer_point = cop
            .path
            .iter()
            .take(8)
            .rev()
            .find(|w| nav.line_of_sight(pos, **w))
            .copied()
            .unwrap_or(target.translation);

        let forward = *transform.forward();
        let to_target = (steer_point - pos) * Vec3::new(1.0, 0.0, 1.0);
        let desired = to_target.normalize_or_zero();
        // Signed angle around Y from our heading to the target direction:
        // positive = target is to our left (counterclockwise from above).
        let angle = forward.cross(desired).y.atan2(forward.dot(desired));

        // Positive steer turns right, so steer against the sign of the angle.
        cop.steer = (-angle * 1.5).clamp(-1.0, 1.0);
        cop.throttle = 1.0;

        // Stuck detection: full throttle but barely moving.
        let planar_speed = velocity.0.xz().length();
        if planar_speed < 1.0 {
            cop.stuck_time += dt;
        } else {
            cop.stuck_time = 0.0;
        }
        if cop.stuck_time > 1.2 {
            cop.stuck_time = 0.0;
            cop.reversing_time = 0.9;
            cop.path.clear(); // force a repath after backing out
            cop.repath_time = 0.0;
        }
    }
}

fn cop_drive(
    time: Res<Time>,
    mut cops: Query<(&CopCar, &Transform, &mut LinearVelocity, &mut AngularVelocity)>,
) {
    let dt = time.delta_secs();
    for (cop, transform, mut lin_vel, mut ang_vel) in &mut cops {
        apply_drive(
            dt,
            &COP_DRIVE,
            cop.throttle,
            cop.steer,
            false,
            transform,
            &mut lin_vel,
            &mut ang_vel,
        );
    }
}

/// Cop-vs-player contact: damage scales with closing speed. The cop takes a
/// small cut too — ramming is their weapon, attrition is the players' answer.
fn cop_rams(
    mut collisions: MessageReader<CollisionStart>,
    mut cops: Query<(&LinearVelocity, &mut Health), (With<CopCar>, Without<Player>)>,
    mut players: Query<(&LinearVelocity, &mut Health), (With<Player>, Without<CopCar>)>,
    mut commands: Commands,
) {
    for event in collisions.read() {
        let a = event.body1.unwrap_or(event.collider1);
        let b = event.body2.unwrap_or(event.collider2);
        let pairs = [(a, b), (b, a)];
        for (cop_entity, player_entity) in pairs {
            let Ok((cop_vel, mut cop_health)) = cops.get_mut(cop_entity) else {
                continue;
            };
            let Ok((player_vel, mut player_health)) = players.get_mut(player_entity) else {
                continue;
            };
            let closing_speed = (cop_vel.0 - player_vel.0).xz().length();
            let damage = if closing_speed > RAM_SPEED_THRESHOLD {
                ((closing_speed - RAM_SPEED_THRESHOLD) * RAM_DAMAGE_SCALE)
                    .clamp(RAM_MIN_DAMAGE, RAM_MAX_DAMAGE)
            } else {
                0.0
            };
            if damage > 0.0 {
                player_health.current -= damage;
                cop_health.current -= damage * RAM_SELF_DAMAGE;
                commands.entity(player_entity).try_insert(DamageFlash(0.15));
            }
        }
    }
}

/// Wrecked cops burst into debris and call for backup: two fresh units each.
fn wreck_cops(
    mut commands: Commands,
    time: Res<Time>,
    cop_assets: Res<CopAssets>,
    car_assets: Res<CarAssets>,
    mut sfx: MessageWriter<PlaySfx>,
    cops: Query<(Entity, &Health, &Transform), With<CopCar>>,
) {
    let alive = cops.iter().filter(|(_, h, _)| h.current > 0.0).count();
    let mut budget = MAX_COPS.saturating_sub(alive);

    for (entity, health, transform) in &cops {
        if health.current > 0.0 {
            continue;
        }
        info!("Cop wrecked — backup incoming!");
        sfx.write(PlaySfx {
            kind: SfxKind::Wreck,
            position: Some(transform.translation),
        });
        commands.entity(entity).try_despawn();

        // Dark debris burst.
        for i in 0..8 {
            let angle = i as f32 * 2.399963;
            commands.spawn((
                Name::new("Cop debris"),
                Mesh3d(car_assets.debris.clone()),
                MeshMaterial3d(cop_assets.cabin_material.clone()),
                Transform::from_translation(transform.translation + Vec3::Y * 0.3),
                RigidBody::Dynamic,
                Collider::cuboid(0.3, 0.3, 0.3),
                Mass(0.5),
                LinearVelocity(Vec3::new(
                    angle.cos() * 4.0,
                    4.0 + (i % 3) as f32,
                    angle.sin() * 4.0,
                )),
                Lifetime(2.5),
            ));
        }

        // The hydra rule.
        let spawns = REPLACEMENTS_PER_WRECK.min(budget);
        budget -= spawns;
        for i in 0..spawns {
            let pos = pick_spawn_point(time.elapsed_secs(), i);
            spawn_cop(&mut commands, &cop_assets, &car_assets, pos);
        }
    }
}
