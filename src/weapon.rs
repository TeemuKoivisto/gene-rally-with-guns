//! The machine gun: front-mounted, ammo-limited, hold-to-fire (design §8).
//!
//! Projectiles are small fast rigid bodies with swept CCD; hits are resolved
//! from avian collision events. Sensors (pickups) never block bullets.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::input::CarAction;
use crate::vehicle::{Car, CarAssets, Health, Player};

const FIRE_RATE: f32 = 9.0; // rounds per second
const PROJECTILE_SPEED: f32 = 45.0;
const PROJECTILE_DAMAGE: f32 = 9.0;
const PROJECTILE_LIFETIME: f32 = 1.2;
pub const START_AMMO: u32 = 25;

/// A car's (single, design §8) weapon slot. M2: machine gun only.
#[derive(Component)]
pub struct WeaponSlot {
    pub ammo: u32,
    cooldown: f32,
}

impl Default for WeaponSlot {
    fn default() -> Self {
        Self {
            ammo: START_AMMO,
            cooldown: 0.0,
        }
    }
}

#[derive(Component)]
pub struct Projectile {
    damage: f32,
    shooter: Entity,
}

/// Generic despawn-after-seconds, also used by debris.
#[derive(Component)]
pub struct Lifetime(pub f32);

/// Brief white-hot tint on the car body when hit.
#[derive(Component)]
pub struct DamageFlash(pub f32);

const FLASH_TIME: f32 = 0.15;

#[derive(Resource)]
struct ProjectileAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_projectile_assets)
            .add_systems(FixedUpdate, fire_weapons)
            .add_systems(Update, (resolve_projectile_hits, tick_lifetimes, tick_damage_flash));
    }
}

fn setup_projectile_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ProjectileAssets {
        mesh: meshes.add(Sphere::new(0.09)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.3),
            emissive: LinearRgba::rgb(4.0, 3.2, 0.6),
            unlit: true,
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
            &LinearVelocity,
            &mut WeaponSlot,
        ),
        With<Car>,
    >,
) {
    let dt = time.delta_secs();
    for (car, actions, transform, velocity, mut slot) in &mut cars {
        slot.cooldown = (slot.cooldown - dt).max(0.0);
        if !actions.pressed(&CarAction::Fire) || slot.cooldown > 0.0 || slot.ammo == 0 {
            continue;
        }
        slot.cooldown = 1.0 / FIRE_RATE;
        slot.ammo -= 1;

        let forward = *transform.forward();
        // Muzzle sits clear of the car's own collider (half-length 1.0).
        let muzzle = transform.translation + forward * 1.5 + Vec3::Y * 0.1;
        // Inherit the car's planar velocity so shots stay accurate at speed.
        let planar_vel = Vec3::new(velocity.x, 0.0, velocity.z);

        commands.spawn((
            Name::new("Bullet"),
            Projectile {
                damage: PROJECTILE_DAMAGE,
                shooter: car,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(muzzle),
            RigidBody::Dynamic,
            Collider::sphere(0.09),
            Mass(0.15),
            GravityScale(0.0),
            SweptCcd::default(),
            LinearVelocity(planar_vel + forward * PROJECTILE_SPEED),
            CollisionEventsEnabled,
            Lifetime(PROJECTILE_LIFETIME),
        ));
    }
}

/// Consume collision events involving projectiles: damage what they hit
/// (if it has Health), flash it, and despawn the bullet.
fn resolve_projectile_hits(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    projectiles: Query<&Projectile>,
    sensors: Query<(), With<Sensor>>,
    mut healths: Query<&mut Health>,
) {
    for event in collisions.read() {
        // A projectile can be on either side of the event.
        let pairs = [
            (event.collider1, event.collider2, event.body2),
            (event.collider2, event.collider1, event.body1),
        ];
        for (maybe_bullet, other, other_body) in pairs {
            let Ok(projectile) = projectiles.get(maybe_bullet) else {
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
                health.current -= projectile.damage;
                commands.entity(target).insert(DamageFlash(FLASH_TIME));
            }
            commands.entity(maybe_bullet).try_despawn();
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
        let handle = &assets.body_materials[player.id % assets.body_materials.len()];
        if let Some(mut material) = materials.get_mut(handle) {
            if flash.0 <= 0.0 {
                material.emissive = LinearRgba::BLACK;
            } else {
                let intensity = 6.0 * (flash.0 / FLASH_TIME);
                material.emissive = LinearRgba::rgb(intensity, intensity, intensity);
            }
        }
        if flash.0 <= 0.0 {
            commands.entity(entity).remove::<DamageFlash>();
        }
    }
}
