//! Weapons (design §8): shotgun, bazooka, grenade launcher.
//! Cars spawn unarmed — the only way to get (or replace) a gun is a crate,
//! and a gun that runs dry leaves you unarmed again.
//!
//! Single active slot per car; crates swap the carried weapon. Projectiles
//! are small fast rigid bodies with swept CCD; hits resolve from avian
//! collision events. Explosive rounds send `Explode` messages consumed by a
//! shared AoE system (damage falloff + physics knockback + flash VFX).

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::camera::ShakeCamera;
use crate::input::CarAction;
use crate::vehicle::{Car, CarAssets, Health, Player};

/// Shotgun: pellets per shell, fan half-angle (rad), pellet speed (m/s), and
/// the rearward kick per blast (m/s). Pellet lifetime caps the effective
/// range (~speed * lifetime), so blasts are brutal close and harmless far.
const SHOTGUN_PELLETS: usize = 7;
const SHOTGUN_SPREAD: f32 = 0.24;
const SHOTGUN_PELLET_SPEED: f32 = 42.0;
const SHOTGUN_PELLET_DAMAGE: f32 = 5.0;
const SHOTGUN_PELLET_LIFETIME: f32 = 0.26;
const SHOTGUN_RECOIL: f32 = 3.5;
const BAZOOKA_RECOIL: f32 = 2.8;
const GRENADE_RECOIL: f32 = 1.4;
const SHOTGUN_SHAKE: f32 = 0.22;
const BAZOOKA_SHAKE: f32 = 0.38;
const GRENADE_SHAKE: f32 = 0.2;
const HIT_SHAKE: f32 = 0.14;
const EXPLOSION_SHAKE: f32 = 0.28;
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
    Shotgun,
    Bazooka,
    GrenadeLauncher,
}

impl WeaponKind {
    pub fn refill_ammo(self) -> u32 {
        match self {
            Self::Shotgun => 12,
            Self::Bazooka => 5,
            Self::GrenadeLauncher => 8,
        }
    }

    fn cooldown(self) -> f32 {
        match self {
            Self::Shotgun => 0.9, // pump-action pace
            Self::Bazooka => 1.0,
            Self::GrenadeLauncher => 0.45,
        }
    }
}

/// A car's (single, design §8) weapon slot. `kind: None` = unarmed; cars
/// spawn that way and return to it when their gun runs dry.
#[derive(Component, Default)]
pub struct WeaponSlot {
    pub kind: Option<WeaponKind>,
    pub ammo: u32,
    cooldown: f32,
    /// Grenade launcher: seconds the fire button has been held.
    charge: f32,
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

/// Brief hot tint on the car body when hit.
#[derive(Component)]
pub struct DamageFlash(pub f32);

const FLASH_TIME: f32 = 0.22;

#[derive(Component)]
struct MuzzleFlash {
    age: f32,
    lifetime: f32,
    start_scale: f32,
}

#[derive(Component)]
struct HitSpark {
    age: f32,
    lifetime: f32,
}

/// Fading trail puff left by a projectile.
#[derive(Component)]
struct TrailPuff {
    age: f32,
    lifetime: f32,
}

#[derive(Clone, Copy)]
enum TrailKind {
    Tracer,
    RocketSmoke,
    Grenade,
}

#[derive(Component)]
struct TrailEmitter {
    kind: TrailKind,
    cooldown: f32,
}

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
    pellet_mesh: Handle<Mesh>,
    pellet_material: Handle<StandardMaterial>,
    rocket_mesh: Handle<Mesh>,
    rocket_material: Handle<StandardMaterial>,
    grenade_mesh: Handle<Mesh>,
    grenade_material: Handle<StandardMaterial>,
    explosion_mesh: Handle<Mesh>,
    explosion_material: Handle<StandardMaterial>,
    muzzle_flash_mesh: Handle<Mesh>,
    flash_minigun: Handle<StandardMaterial>,
    flash_rocket: Handle<StandardMaterial>,
    flash_grenade: Handle<StandardMaterial>,
    spark_mesh: Handle<Mesh>,
    spark_material: Handle<StandardMaterial>,
    trail_tracer_mesh: Handle<Mesh>,
    trail_tracer_material: Handle<StandardMaterial>,
    trail_smoke_mesh: Handle<Mesh>,
    trail_smoke_material: Handle<StandardMaterial>,
    trail_grenade_material: Handle<StandardMaterial>,
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
                    emit_projectile_trails,
                    pulse_grenade_fuse,
                    tick_lifetimes,
                    tick_damage_flash,
                    tick_muzzle_flashes,
                    tick_hit_sparks,
                    tick_trail_puffs,
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
        // Chunky bright streaks so the shotgun fan is readable from the iso
        // camera (a to-scale pellet would be sub-pixel).
        pellet_mesh: meshes.add(Cuboid::new(0.12, 0.12, 0.4)),
        pellet_material: materials.add(StandardMaterial {
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
        muzzle_flash_mesh: meshes.add(Sphere::new(0.22)),
        flash_minigun: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.95, 0.5, 0.95),
            emissive: LinearRgba::rgb(14.0, 12.0, 2.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        flash_rocket: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.55, 0.15, 0.95),
            emissive: LinearRgba::rgb(12.0, 4.0, 0.5),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        flash_grenade: materials.add(StandardMaterial {
            base_color: Color::srgba(0.5, 1.0, 0.45, 0.95),
            emissive: LinearRgba::rgb(2.0, 10.0, 1.5),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        spark_mesh: meshes.add(Cuboid::new(0.12, 0.12, 0.12)),
        spark_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.75, 0.2, 1.0),
            emissive: LinearRgba::rgb(10.0, 6.0, 1.0),
            unlit: true,
            ..default()
        }),
        trail_tracer_mesh: meshes.add(Cuboid::new(0.1, 0.1, 0.35)),
        trail_tracer_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.9, 0.25, 0.55),
            emissive: LinearRgba::rgb(4.0, 3.5, 0.5),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        trail_smoke_mesh: meshes.add(Sphere::new(0.2)),
        trail_smoke_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.35, 0.38, 0.5),
            emissive: LinearRgba::rgb(0.3, 0.3, 0.3),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        trail_grenade_material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.95, 0.4, 0.45),
            emissive: LinearRgba::rgb(1.0, 4.0, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
    });
}

fn spawn_muzzle_flash(
    commands: &mut Commands,
    assets: &ProjectileAssets,
    kind: WeaponKind,
    pos: Vec3,
    dir: Vec3,
) {
    let (material, lifetime, scale) = match kind {
        WeaponKind::Shotgun => (assets.flash_minigun.clone(), 0.08, 1.25),
        WeaponKind::Bazooka => (assets.flash_rocket.clone(), 0.1, 1.35),
        WeaponKind::GrenadeLauncher => (assets.flash_grenade.clone(), 0.08, 1.1),
    };
    commands.spawn((
        Name::new("Muzzle flash"),
        MuzzleFlash {
            age: 0.0,
            lifetime,
            start_scale: scale,
        },
        Mesh3d(assets.muzzle_flash_mesh.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(pos)
            .looking_to(dir, Vec3::Y)
            .with_scale(Vec3::splat(scale)),
    ));
}

fn spawn_hit_sparks(commands: &mut Commands, assets: &ProjectileAssets, pos: Vec3, seed: u32) {
    for i in 0..6 {
        let angle = seed.wrapping_mul(17).wrapping_add(i * 97) as f32 * 0.31;
        let offset = Vec3::new(angle.cos() * 0.3, 0.2 + (i % 2) as f32 * 0.15, angle.sin() * 0.3);
        commands.spawn((
            Name::new("Hit spark"),
            HitSpark {
                age: 0.0,
                lifetime: 0.2,
            },
            Mesh3d(assets.spark_mesh.clone()),
            MeshMaterial3d(assets.spark_material.clone()),
            Transform::from_translation(pos + offset)
                .with_scale(Vec3::splat(0.8 + (i % 2) as f32 * 0.35)),
        ));
    }
}

fn fire_weapons(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<ProjectileAssets>,
    mut sfx: MessageWriter<PlaySfx>,
    mut shake: MessageWriter<ShakeCamera>,
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
        // A gun that runs dry is gone: unarmed until the next crate.
        if slot.kind.is_some() && slot.ammo == 0 {
            info!("Out of ammo — unarmed!");
            slot.kind = None;
        }
        slot.cooldown = (slot.cooldown - dt).max(0.0);
        let Some(kind) = slot.kind else {
            slot.charge = 0.0;
            continue;
        };
        let pressed = actions.pressed(&CarAction::Fire);
        let released = actions.just_released(&CarAction::Fire);
        let ready = slot.cooldown <= 0.0 && slot.ammo > 0;

        let forward = *transform.forward();
        // Muzzle sits clear of the car's own collider (half-length ~1.1).
        let muzzle = transform.translation + forward * 1.6 + Vec3::Y * 0.15;
        // Inherit the car's planar velocity so shots stay accurate at speed.
        let planar_vel = Vec3::new(velocity.x, 0.0, velocity.z);

        match kind {
            WeaponKind::Shotgun => {
                if pressed && ready {
                    slot.cooldown = kind.cooldown();
                    slot.ammo -= 1;
                    // Per-blast jitter: cheap deterministic hash, no rand crate.
                    let noise = ((time.elapsed_secs() * 12.9898 + slot.ammo as f32 * 78.233)
                        .sin()
                        * 43758.5453)
                        .fract();
                    velocity.0 -= forward * SHOTGUN_RECOIL;
                    spawn_muzzle_flash(&mut commands, &assets, kind, muzzle, forward);
                    shake.write(ShakeCamera {
                        intensity: SHOTGUN_SHAKE,
                    });
                    for i in 0..SHOTGUN_PELLETS {
                        let frac = i as f32 / (SHOTGUN_PELLETS - 1) as f32;
                        let yaw =
                            (frac * 2.0 - 1.0) * SHOTGUN_SPREAD + (noise * 2.0 - 1.0) * 0.05;
                        let dir = Quat::from_rotation_y(yaw) * forward;
                        commands.spawn((
                            Name::new("Pellet"),
                            Projectile {
                                direct_damage: SHOTGUN_PELLET_DAMAGE,
                                shooter: car,
                                explosive: None,
                            },
                            Mesh3d(assets.pellet_mesh.clone()),
                            MeshMaterial3d(assets.pellet_material.clone()),
                            Transform::from_translation(muzzle).looking_to(dir, Vec3::Y),
                            projectile_physics(
                                0.07,
                                planar_vel + dir * SHOTGUN_PELLET_SPEED,
                                0.0,
                            ),
                            TrailEmitter {
                                kind: TrailKind::Tracer,
                                cooldown: 0.0,
                            },
                            Lifetime(SHOTGUN_PELLET_LIFETIME),
                        ));
                    }
                    sfx.write(PlaySfx {
                        kind: SfxKind::Minigun,
                        position: Some(muzzle),
                    });
                }
            }
            WeaponKind::Bazooka => {
                if pressed && ready {
                    slot.cooldown = kind.cooldown();
                    slot.ammo -= 1;
                    velocity.0 -= forward * BAZOOKA_RECOIL;
                    spawn_muzzle_flash(&mut commands, &assets, kind, muzzle, forward);
                    shake.write(ShakeCamera {
                        intensity: BAZOOKA_SHAKE,
                    });
                    sfx.write(PlaySfx {
                        kind: SfxKind::RocketFire,
                        position: Some(muzzle),
                    });
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
                        projectile_physics(0.12, forward * 12.0, 0.0),
                        RocketMotor {
                            accel: 30.0,
                            max_speed: 34.0,
                        },
                        TrailEmitter {
                            kind: TrailKind::RocketSmoke,
                            cooldown: 0.0,
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
                    slot.cooldown = kind.cooldown();
                    slot.ammo -= 1;
                    let speed =
                        GRENADE_MIN_SPEED + (GRENADE_MAX_SPEED - GRENADE_MIN_SPEED) * power;
                    // Launch in an arc: clears the 3-high buildings mid-charge.
                    let dir = forward * GRENADE_ELEVATION.cos() + Vec3::Y * GRENADE_ELEVATION.sin();
                    let launch_pos = muzzle + Vec3::Y * 0.5;
                    velocity.0 -= forward * GRENADE_RECOIL;
                    spawn_muzzle_flash(&mut commands, &assets, kind, launch_pos, dir);
                    shake.write(ShakeCamera {
                        intensity: GRENADE_SHAKE,
                    });
                    sfx.write(PlaySfx {
                        kind: SfxKind::GrenadeLaunch,
                        position: Some(launch_pos),
                    });
                    commands.spawn((
                        Name::new("Grenade"),
                        Projectile {
                            direct_damage: 15.0,
                            shooter: car,
                            explosive: Some((5.0, 70.0)),
                        },
                        Mesh3d(assets.grenade_mesh.clone()),
                        MeshMaterial3d(assets.grenade_material.clone()),
                        Transform::from_translation(launch_pos),
                        projectile_physics(0.18, planar_vel + dir * speed, GRENADE_GRAVITY),
                        TrailEmitter {
                            kind: TrailKind::Grenade,
                            cooldown: 0.0,
                        },
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

/// Projectiles live on their own collision layer and ignore each other: a
/// shotgun fan spawns 7 overlapping pellets at the muzzle, and without this
/// they'd register hits on their neighbors and despawn on the spot.
const PROJECTILE_LAYER: LayerMask = LayerMask(1 << 1);

/// Common physics bundle for all rounds. `gravity` 0 = flies flat.
fn projectile_physics(radius: f32, velocity: Vec3, gravity: f32) -> impl Bundle {
    (
        RigidBody::Dynamic,
        Collider::sphere(radius),
        CollisionLayers::new(PROJECTILE_LAYER, !PROJECTILE_LAYER),
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
    assets: Res<ProjectileAssets>,
    mut collisions: MessageReader<CollisionStart>,
    mut explosions: MessageWriter<Explode>,
    mut sfx: MessageWriter<PlaySfx>,
    mut shake: MessageWriter<ShakeCamera>,
    projectiles: Query<(&Projectile, &Transform)>,
    sensors: Query<(), With<Sensor>>,
    players: Query<(), With<Player>>,
    mut healths: Query<(&Transform, &mut Health, &mut LinearVelocity)>,
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
            let hit_pos = bullet_transform.translation;
            if let Ok((target_transform, mut health, mut velocity)) = healths.get_mut(target) {
                health.current -= projectile.direct_damage;
                let knock_dir =
                    (target_transform.translation - hit_pos).normalize_or_zero() + Vec3::Y * 0.2;
                let knock_strength = if projectile.explosive.is_some() { 2.0 } else { 4.5 };
                velocity.0 += knock_dir * knock_strength;
                if players.contains(target) {
                    commands.entity(target).try_insert(DamageFlash(FLASH_TIME));
                    shake.write(ShakeCamera {
                        intensity: HIT_SHAKE,
                    });
                }
            }
            spawn_hit_sparks(
                &mut commands,
                &assets,
                hit_pos,
                maybe_bullet.to_bits() as u32,
            );
            if projectile.explosive.is_none() {
                sfx.write(PlaySfx {
                    kind: SfxKind::Hit,
                    position: Some(hit_pos),
                });
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
    mut sfx: MessageWriter<PlaySfx>,
    mut shake: MessageWriter<ShakeCamera>,
    assets: Res<ProjectileAssets>,
    players: Query<(), With<Player>>,
    mut healths: Query<(Entity, &Transform, &mut Health)>,
    mut bodies: Query<(&Transform, &mut LinearVelocity), Without<Sensor>>,
) {
    for explosion in explosions.read() {
        let big = explosion.radius >= 5.0;
        sfx.write(PlaySfx {
            kind: if big {
                SfxKind::ExplosionBig
            } else {
                SfxKind::Explosion
            },
            position: Some(explosion.pos),
        });
        shake.write(ShakeCamera {
            intensity: if big {
                EXPLOSION_SHAKE
            } else {
                EXPLOSION_SHAKE * 0.65
            },
        });
        spawn_hit_sparks(
            &mut commands,
            &assets,
            explosion.pos + Vec3::Y * 0.2,
            explosion.pos.x.to_bits() ^ explosion.pos.z.to_bits(),
        );
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

fn emit_projectile_trails(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<ProjectileAssets>,
    mut emitters: Query<(&Transform, &mut TrailEmitter)>,
) {
    let dt = time.delta_secs();
    for (transform, mut emitter) in &mut emitters {
        emitter.cooldown -= dt;
        let interval = match emitter.kind {
            TrailKind::Tracer => 0.022,
            TrailKind::RocketSmoke => 0.045,
            TrailKind::Grenade => 0.06,
        };
        if emitter.cooldown > 0.0 {
            continue;
        }
        emitter.cooldown = interval;

        let (mesh, material, lifetime, scale) = match emitter.kind {
            TrailKind::Tracer => (
                assets.trail_tracer_mesh.clone(),
                assets.trail_tracer_material.clone(),
                0.12,
                Vec3::new(0.9, 0.9, 1.2),
            ),
            TrailKind::RocketSmoke => (
                assets.trail_smoke_mesh.clone(),
                assets.trail_smoke_material.clone(),
                0.35,
                Vec3::splat(0.9 + (time.elapsed_secs() * 3.0).sin().abs() * 0.25),
            ),
            TrailKind::Grenade => (
                assets.trail_smoke_mesh.clone(),
                assets.trail_grenade_material.clone(),
                0.25,
                Vec3::splat(0.55),
            ),
        };

        commands.spawn((
            Name::new("Trail puff"),
            TrailPuff {
                age: 0.0,
                lifetime,
            },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(transform.translation - Vec3::Y * 0.05).with_scale(scale),
        ));
    }
}

fn pulse_grenade_fuse(
    time: Res<Time>,
    mut grenades: Query<(&Fuse, &mut Transform)>,
) {
    for (fuse, mut transform) in &mut grenades {
        if fuse.0 > 0.6 {
            continue;
        }
        let urgency = 1.0 - fuse.0 / 0.6;
        let pulse = 1.0 + urgency * 0.25 * (time.elapsed_secs() * 20.0).sin().abs();
        transform.scale = Vec3::splat(pulse);
    }
}

fn tick_muzzle_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut flashes: Query<(Entity, &mut MuzzleFlash, &mut Transform)>,
) {
    for (entity, mut flash, mut transform) in &mut flashes {
        flash.age += time.delta_secs();
        if flash.age >= flash.lifetime {
            commands.entity(entity).try_despawn();
            continue;
        }
        let t = 1.0 - flash.age / flash.lifetime;
        transform.scale = Vec3::splat(flash.start_scale * (0.25 + 0.75 * t));
    }
}

fn tick_hit_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut HitSpark, &mut Transform)>,
) {
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += time.delta_secs();
        let t = 1.0 - (spark.age / spark.lifetime).min(1.0);
        transform.translation.y += time.delta_secs() * 1.5;
        transform.scale = Vec3::splat(t.max(0.1));
        if t <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

fn tick_trail_puffs(
    mut commands: Commands,
    time: Res<Time>,
    mut puffs: Query<(Entity, &mut TrailPuff, &mut Transform)>,
) {
    for (entity, mut puff, mut transform) in &mut puffs {
        puff.age += time.delta_secs();
        let t = 1.0 - (puff.age / puff.lifetime).min(1.0);
        transform.scale *= 1.0 + time.delta_secs() * 0.8;
        transform.translation.y += time.delta_secs() * 0.2;
        if t <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Fade the hot emissive tint back out after a hit.
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
                let t = flash.0 / FLASH_TIME;
                let intensity = 8.0 * t;
                material.emissive = LinearRgba::rgb(intensity * 1.1, intensity * 0.35, intensity * 0.08);
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
        if slot.kind == Some(WeaponKind::GrenadeLauncher) && slot.charge > 0.0 {
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

