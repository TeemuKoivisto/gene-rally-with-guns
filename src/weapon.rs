//! Weapons (design §8): machine gun, bazooka, grenade launcher.
//!
//! Single active slot per car; crates swap the carried weapon. Projectiles
//! are small fast rigid bodies with swept CCD; hits resolve from avian
//! collision events. Explosive rounds send `Explode` messages consumed by a
//! shared AoE system (damage falloff + physics knockback + flash VFX).

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::input::CarAction;
use crate::vehicle::{Car, CarAssets, Health, Player};

pub const START_AMMO: u32 = 60;
/// Minigun: per-shot spread half-angle (rad) and rearward recoil per shot (m/s).
const MINIGUN_SPREAD: f32 = 0.09;
const MINIGUN_RECOIL: f32 = 0.55;
/// Grenade launch: elevation angle (rad) and charge-scaled speed range.
/// Tap = drop it at your feet (mine-like); full charge = fast arc across the map.
/// Speed scales with charge^2 so the short end of the range stays controllable.
const GRENADE_ELEVATION: f32 = 0.62;
const GRENADE_MIN_SPEED: f32 = 4.0;
const GRENADE_MAX_SPEED: f32 = 36.0;
const GRENADE_CHARGE_TIME: f32 = 0.8;
const GRENADE_GRAVITY: f32 = 2.6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeaponKind {
    Minigun,
    Bazooka,
    GrenadeLauncher,
}

impl WeaponKind {
    pub fn refill_ammo(self) -> u32 {
        match self {
            Self::Minigun => 80,
            Self::Bazooka => 5,
            Self::GrenadeLauncher => 8,
        }
    }

    fn cooldown(self) -> f32 {
        match self {
            Self::Minigun => 1.0 / 16.0,
            Self::Bazooka => 1.0,
            Self::GrenadeLauncher => 0.45,
        }
    }
}

/// A car's (single, design §8) weapon slot.
#[derive(Component)]
pub struct WeaponSlot {
    pub kind: WeaponKind,
    pub ammo: u32,
    cooldown: f32,
    /// Grenade launcher: seconds the fire button has been held.
    charge: f32,
}

impl Default for WeaponSlot {
    fn default() -> Self {
        Self {
            kind: WeaponKind::Minigun,
            ammo: START_AMMO,
            cooldown: 0.0,
            charge: 0.0,
        }
    }
}

#[derive(Component)]
pub struct Projectile {
    /// Damage applied directly to whatever the round hits.
    direct_damage: f32,
    shooter: Entity,
    /// `Some((radius, damage))`: also explode on impact.
    explosive: Option<(f32, f32)>,
}

/// Timed detonation backup for grenades (so duds can't lie around).
#[derive(Component)]
struct Fuse(f32);

/// Rocket motor: the round leaves the tube slow and accelerates along its
/// flight direction until max speed — dodgeable up close, lethal down range.
#[derive(Component)]
struct RocketMotor {
    accel: f32,
    max_speed: f32,
}

/// Generic despawn-after-seconds, also used by debris.
#[derive(Component)]
pub struct Lifetime(pub f32);

/// Brief white-hot tint on the car body when hit.
#[derive(Component)]
pub struct DamageFlash(pub f32);

const FLASH_TIME: f32 = 0.15;

/// AoE detonation request: consumed by `resolve_explosions`.
#[derive(Message)]
pub struct Explode {
    pub pos: Vec3,
    pub radius: f32,
    pub damage: f32,
}

/// Growing flash sphere left by an explosion.
#[derive(Component)]
struct ExplosionVfx {
    age: f32,
    radius: f32,
}

#[derive(Resource)]
struct ProjectileAssets {
    tracer_mesh: Handle<Mesh>,
    tracer_material: Handle<StandardMaterial>,
    rocket_mesh: Handle<Mesh>,
    rocket_material: Handle<StandardMaterial>,
    grenade_mesh: Handle<Mesh>,
    grenade_material: Handle<StandardMaterial>,
    explosion_mesh: Handle<Mesh>,
    explosion_material: Handle<StandardMaterial>,
}

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Explode>()
            .add_systems(Startup, setup_projectile_assets)
            .add_systems(FixedUpdate, (fire_weapons, drive_rockets))
            .add_systems(
                Update,
                (
                    resolve_projectile_hits,
                    tick_fuses,
                    resolve_explosions,
                    tick_lifetimes,
                    tick_damage_flash,
                    grow_explosion_vfx,
                    draw_grenade_trajectory,
                )
                    .chain(),
            );
    }
}

fn setup_projectile_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ProjectileAssets {
        // Fat, bright tracer so the MG stream is readable from the iso camera.
        tracer_mesh: meshes.add(Cuboid::new(0.14, 0.14, 0.55)),
        tracer_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.95, 0.2),
            emissive: LinearRgba::rgb(8.0, 7.0, 1.0),
            unlit: true,
            ..default()
        }),
        rocket_mesh: meshes.add(Cuboid::new(0.2, 0.2, 0.8)),
        rocket_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.35, 0.15),
            emissive: LinearRgba::rgb(5.0, 1.2, 0.2),
            unlit: true,
            ..default()
        }),
        grenade_mesh: meshes.add(Sphere::new(0.18)),
        grenade_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.9, 0.3),
            emissive: LinearRgba::rgb(0.6, 3.5, 0.6),
            unlit: true,
            ..default()
        }),
        explosion_mesh: meshes.add(Sphere::new(1.0)),
        explosion_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.6, 0.15, 0.85),
            emissive: LinearRgba::rgb(6.0, 2.5, 0.4),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
    });
}

fn fire_weapons(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<ProjectileAssets>,
    mut cars: Query<
        (
            Entity,
            &ActionState<CarAction>,
            &Transform,
            &mut LinearVelocity,
            &mut WeaponSlot,
        ),
        With<Car>,
    >,
) {
    let dt = time.delta_secs();
    for (car, actions, transform, mut velocity, mut slot) in &mut cars {
        slot.cooldown = (slot.cooldown - dt).max(0.0);
        let pressed = actions.pressed(&CarAction::Fire);
        let released = actions.just_released(&CarAction::Fire);
        let ready = slot.cooldown <= 0.0 && slot.ammo > 0;

        let forward = *transform.forward();
        // Muzzle sits clear of the car's own collider (half-length ~1.1).
        let muzzle = transform.translation + forward * 1.6 + Vec3::Y * 0.15;
        // Inherit the car's planar velocity so shots stay accurate at speed.
        let planar_vel = Vec3::new(velocity.x, 0.0, velocity.z);

        match slot.kind {
            WeaponKind::Minigun => {
                if pressed && ready {
                    slot.cooldown = slot.kind.cooldown();
                    slot.ammo -= 1;
                    // Per-shot spread: cheap deterministic hash, no rand crate.
                    let noise = ((time.elapsed_secs() * 12.9898 + slot.ammo as f32 * 78.233)
                        .sin()
                        * 43758.5453)
                        .fract();
                    let yaw = (noise * 2.0 - 1.0) * MINIGUN_SPREAD;
                    let dir = Quat::from_rotation_y(yaw) * forward;
                    // Recoil: every shot shoves the car backward a touch.
                    velocity.0 -= forward * MINIGUN_RECOIL;
                    commands.spawn((
                        Name::new("Tracer"),
                        Projectile {
                            direct_damage: 5.0,
                            shooter: car,
                            explosive: None,
                        },
                        Mesh3d(assets.tracer_mesh.clone()),
                        MeshMaterial3d(assets.tracer_material.clone()),
                        Transform::from_translation(muzzle).looking_to(dir, Vec3::Y),
                        projectile_physics(0.09, planar_vel + dir * 45.0, 0.0),
                        // Short-to-mid range: tracers die sooner than before.
                        Lifetime(0.65),
                    ));
                }
            }
            WeaponKind::Bazooka => {
                if pressed && ready {
                    slot.cooldown = slot.kind.cooldown();
                    slot.ammo -= 1;
                    commands.spawn((
                        Name::new("Rocket"),
                        Projectile {
                            direct_damage: 50.0,
                            shooter: car,
                            explosive: Some((5.5, 40.0)),
                        },
                        Mesh3d(assets.rocket_mesh.clone()),
                        MeshMaterial3d(assets.rocket_material.clone()),
                        Transform::from_translation(muzzle).looking_to(forward, Vec3::Y),
                        // No inherited car velocity; the motor does the work:
                        // slow launch, accelerating well past car top speed.
                        projectile_physics(0.12, forward * 12.0, 0.0),
                        RocketMotor {
                            accel: 30.0,
                            max_speed: 34.0,
                        },
                        Lifetime(3.0),
                    ));
                }
            }
            WeaponKind::GrenadeLauncher => {
                if pressed && slot.ammo > 0 {
                    // Hold to charge the throw.
                    slot.charge = (slot.charge + dt).min(GRENADE_CHARGE_TIME);
                } else if released && ready {
                    let power = (slot.charge / GRENADE_CHARGE_TIME).powi(2);
                    slot.charge = 0.0;
                    slot.cooldown = slot.kind.cooldown();
                    slot.ammo -= 1;
                    let speed =
                        GRENADE_MIN_SPEED + (GRENADE_MAX_SPEED - GRENADE_MIN_SPEED) * power;
                    // Launch in an arc: clears the 3-high buildings mid-charge.
                    let dir = forward * GRENADE_ELEVATION.cos() + Vec3::Y * GRENADE_ELEVATION.sin();
                    commands.spawn((
                        Name::new("Grenade"),
                        Projectile {
                            direct_damage: 15.0,
                            shooter: car,
                            explosive: Some((5.0, 70.0)),
                        },
                        Mesh3d(assets.grenade_mesh.clone()),
                        MeshMaterial3d(assets.grenade_material.clone()),
                        Transform::from_translation(muzzle + Vec3::Y * 0.5),
                        projectile_physics(0.18, planar_vel + dir * speed, GRENADE_GRAVITY),
                        Fuse(2.0),
                        Lifetime(6.0),
                    ));
                } else {
                    slot.charge = 0.0;
                }
            }
        }
    }
}

/// Accelerate rocket rounds along their current flight direction.
fn drive_rockets(time: Res<Time>, mut rockets: Query<(&RocketMotor, &mut LinearVelocity)>) {
    let dt = time.delta_secs();
    for (motor, mut velocity) in &mut rockets {
        let speed = velocity.0.length();
        if speed < 0.01 {
            continue;
        }
        let new_speed = (speed + motor.accel * dt).min(motor.max_speed);
        velocity.0 *= new_speed / speed;
    }
}

/// Common physics bundle for all rounds. `gravity` 0 = flies flat.
fn projectile_physics(radius: f32, velocity: Vec3, gravity: f32) -> impl Bundle {
    (
        RigidBody::Dynamic,
        Collider::sphere(radius),
        Mass(0.15),
        GravityScale(gravity),
        SweptCcd::default(),
        LinearVelocity(velocity),
        CollisionEventsEnabled,
    )
}

/// Consume collision events involving projectiles: apply direct damage,
/// request the AoE if explosive, despawn the round.
fn resolve_projectile_hits(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    mut explosions: MessageWriter<Explode>,
    projectiles: Query<(&Projectile, &Transform)>,
    sensors: Query<(), With<Sensor>>,
    players: Query<(), With<Player>>,
    mut healths: Query<&mut Health>,
) {
    for event in collisions.read() {
        // A projectile can be on either side of the event.
        let pairs = [
            (event.collider1, event.collider2, event.body2),
            (event.collider2, event.collider1, event.body1),
        ];
        for (maybe_bullet, other, other_body) in pairs {
            let Ok((projectile, bullet_transform)) = projectiles.get(maybe_bullet) else {
                continue;
            };
            // Pickups and other sensors don't stop bullets.
            if sensors.contains(other) {
                continue;
            }
            let target = other_body.unwrap_or(other);
            // Never interact with the car that fired it.
            if target == projectile.shooter {
                continue;
            }
            if let Ok(mut health) = healths.get_mut(target) {
                health.current -= projectile.direct_damage;
                // Flash only players: the flash tint runs on body color materials.
                // try_*: the target may be despawned by another system this frame.
                if players.contains(target) {
                    commands.entity(target).try_insert(DamageFlash(FLASH_TIME));
                }
            }
            if let Some((radius, damage)) = projectile.explosive {
                explosions.write(Explode {
                    pos: bullet_transform.translation,
                    radius,
                    damage,
                });
            }
            commands.entity(maybe_bullet).try_despawn();
        }
    }
}

/// Grenade fuse: detonate in place when the timer runs out.
fn tick_fuses(
    mut commands: Commands,
    time: Res<Time>,
    mut explosions: MessageWriter<Explode>,
    mut fuses: Query<(Entity, &mut Fuse, &Transform, &Projectile)>,
) {
    for (entity, mut fuse, transform, projectile) in &mut fuses {
        fuse.0 -= time.delta_secs();
        if fuse.0 > 0.0 {
            continue;
        }
        if let Some((radius, damage)) = projectile.explosive {
            explosions.write(Explode {
                pos: transform.translation,
                radius,
                damage,
            });
        }
        commands.entity(entity).try_despawn();
    }
}

/// Shared AoE: falloff damage to everything with Health (shooter included —
/// mind your own grenades), knockback to every dynamic body, flash VFX.
fn resolve_explosions(
    mut commands: Commands,
    mut explosions: MessageReader<Explode>,
    assets: Res<ProjectileAssets>,
    players: Query<(), With<Player>>,
    mut healths: Query<(Entity, &Transform, &mut Health)>,
    mut bodies: Query<(&Transform, &mut LinearVelocity), Without<Sensor>>,
) {
    for explosion in explosions.read() {
        // Damage with linear falloff.
        for (entity, transform, mut health) in &mut healths {
            let dist = (transform.translation - explosion.pos).xz().length();
            if dist > explosion.radius {
                continue;
            }
            let falloff = 1.0 - dist / explosion.radius;
            health.current -= explosion.damage * (0.25 + 0.75 * falloff);
            if players.contains(entity) {
                commands.entity(entity).try_insert(DamageFlash(FLASH_TIME));
            }
        }
        // Knockback on anything dynamic nearby (cars, crates, debris).
        for (transform, mut velocity) in &mut bodies {
            let delta = transform.translation - explosion.pos;
            let dist = delta.xz().length();
            if dist > explosion.radius || dist < 0.01 {
                continue;
            }
            let falloff = 1.0 - dist / explosion.radius;
            let push = delta.normalize_or_zero() * 14.0 * falloff;
            velocity.0 += push + Vec3::Y * 5.0 * falloff;
        }
        // Flash sphere.
        commands.spawn((
            Name::new("Explosion"),
            ExplosionVfx {
                age: 0.0,
                radius: explosion.radius,
            },
            Mesh3d(assets.explosion_mesh.clone()),
            MeshMaterial3d(assets.explosion_material.clone()),
            Transform::from_translation(explosion.pos).with_scale(Vec3::splat(0.3)),
        ));
    }
}

fn grow_explosion_vfx(
    mut commands: Commands,
    time: Res<Time>,
    mut vfx: Query<(Entity, &mut ExplosionVfx, &mut Transform)>,
) {
    const GROW_TIME: f32 = 0.22;
    const HOLD_TIME: f32 = 0.3;
    for (entity, mut explosion, mut transform) in &mut vfx {
        explosion.age += time.delta_secs();
        let t = (explosion.age / GROW_TIME).min(1.0);
        transform.scale = Vec3::splat(0.3 + (explosion.radius - 0.3) * t);
        if explosion.age > HOLD_TIME {
            commands.entity(entity).try_despawn();
        }
    }
}

fn tick_lifetimes(
    mut commands: Commands,
    time: Res<Time>,
    mut lifetimes: Query<(Entity, &mut Lifetime)>,
) {
    for (entity, mut lifetime) in &mut lifetimes {
        lifetime.0 -= time.delta_secs();
        if lifetime.0 <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Fade the white-hot emissive tint back out after a hit.
fn tick_damage_flash(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<CarAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(Entity, &mut DamageFlash, &Player)>,
) {
    for (entity, mut flash, player) in &mut flashes {
        flash.0 -= time.delta_secs();
        let handle = &assets.body_materials[player.color % assets.body_materials.len()];
        if let Some(mut material) = materials.get_mut(handle) {
            if flash.0 <= 0.0 {
                material.emissive = LinearRgba::BLACK;
            } else {
                let intensity = 6.0 * (flash.0 / FLASH_TIME);
                material.emissive = LinearRgba::rgb(intensity, intensity, intensity);
            }
        }
        if flash.0 <= 0.0 {
            commands.entity(entity).try_remove::<DamageFlash>();
        }
    }
}

/// Draw a green line showing the grenade launcher trajectory and a landing circle representing the blast area when a player holds space/fire button to charge.
fn draw_grenade_trajectory(
    mut gizmos: Gizmos,
    query: Query<(&Transform, &LinearVelocity, &WeaponSlot), With<Player>>,
) {
    for (transform, velocity, slot) in &query {
        if slot.kind == WeaponKind::GrenadeLauncher && slot.charge > 0.0 {
            let forward = *transform.forward();
            // Muzzle sits clear of the car's own collider (half-length ~1.1).
            let muzzle = transform.translation + forward * 1.6 + Vec3::Y * 0.15;
            // Inherit the car's planar velocity so shots stay accurate at speed.
            let planar_vel = Vec3::new(velocity.x, 0.0, velocity.z);
            let power = (slot.charge / GRENADE_CHARGE_TIME).powi(2);
            let speed = GRENADE_MIN_SPEED + (GRENADE_MAX_SPEED - GRENADE_MIN_SPEED) * power;
            // Launch in an arc.
            let dir = forward * GRENADE_ELEVATION.cos() + Vec3::Y * GRENADE_ELEVATION.sin();
            let init_pos = muzzle + Vec3::Y * 0.5;
            let init_vel = planar_vel + dir * speed;
            let gravity_accel = Vec3::new(0.0, -9.81 * GRENADE_GRAVITY, 0.0);

            // Compute time of impact with ground y = 0.0:
            // 0.5 * g * t^2 - v0.y * t - p0.y = 0
            // Since g = 9.81 * GRENADE_GRAVITY
            let g_magnitude = 9.81 * GRENADE_GRAVITY;
            let discriminant = init_vel.y * init_vel.y + 2.0 * g_magnitude * init_pos.y;
            let t_hit = if discriminant >= 0.0 {
                (init_vel.y + discriminant.sqrt()) / g_magnitude
            } else {
                0.0
            };
            let max_t = t_hit.min(2.0); // limited by the 2.0-second fuse duration

            // Generate points for the trajectory line
            let segments = 30;
            let mut points = Vec::with_capacity(segments + 1);
            for i in 0..=segments {
                let t = (i as f32 / segments as f32) * max_t;
                let pos = init_pos + init_vel * t + 0.5 * gravity_accel * t * t;
                points.push(pos);
            }

            // Draw trajectory path
            gizmos.linestrip(points, Color::srgb(0.3, 0.9, 0.3));

            // Draw impact/blast circle (blast radius = 5.0)
            let impact_pos = init_pos + init_vel * max_t + 0.5 * gravity_accel * max_t * max_t;
            let mut impact_ground = impact_pos;
            impact_ground.y = 0.0; // clamp to ground plane
            let circle_points = (0..=32).map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 32.0;
                impact_ground + Vec3::new(angle.cos() * 5.0, 0.0, angle.sin() * 5.0)
            });
            gizmos.linestrip(circle_points, Color::srgb(0.3, 0.9, 0.3));
        }
    }
}

