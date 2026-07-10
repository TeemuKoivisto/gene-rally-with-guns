//! Weapon crates: fixed spawn points, sensor colliders, timed respawn.
//! Each crate carries a weapon (color-coded); driving over it swaps your gun
//! and refills its ammo — single-slot, grab-overwrites (design §8).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::audio::{PlaySfx, SfxKind};
use crate::weapon::{WeaponKind, WeaponSlot};

const RESPAWN_SECONDS: f32 = 8.0;

/// Fixed crate spawn points around the arena.
const SPAWN_POINTS: [Vec3; 5] = [
    Vec3::new(0.0, 0.7, 0.0),
    Vec3::new(-26.0, 0.7, -15.0),
    Vec3::new(26.0, 0.7, -15.0),
    Vec3::new(-26.0, 0.7, 15.0),
    Vec3::new(26.0, 0.7, 15.0),
];

const KINDS: [WeaponKind; 3] = [
    WeaponKind::Shotgun,
    WeaponKind::Bazooka,
    WeaponKind::GrenadeLauncher,
];

/// Public so the bot AI can steer toward uncollected crates.
#[derive(Component)]
pub struct WeaponCrate {
    point: usize,
    kind: WeaponKind,
}

/// Per-spawn-point respawn countdowns (only ticked while the point is empty).
#[derive(Resource)]
struct RespawnTimers([f32; SPAWN_POINTS.len()]);

/// Global crate counter: kinds rotate, so distribution is exactly equal.
#[derive(Resource, Default)]
struct CrateCounter(usize);

#[derive(Resource)]
struct PickupAssets {
    mesh: Handle<Mesh>,
    /// One material per weapon kind, same order as `KINDS`.
    materials: [Handle<StandardMaterial>; 3],
}

pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RespawnTimers([0.0; SPAWN_POINTS.len()]))
            .init_resource::<CrateCounter>()
            .add_systems(Startup, setup_pickup_assets)
            .add_systems(Update, (respawn_crates, collect_crates, spin_crates));
    }
}

fn setup_pickup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut make = |base: Color, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: base,
            emissive,
            perceptual_roughness: 0.4,
            ..default()
        })
    };
    commands.insert_resource(PickupAssets {
        mesh: meshes.add(Cuboid::new(0.8, 0.8, 0.8)),
        materials: [
            // Shotgun: amber.
            make(Color::srgb(1.0, 0.75, 0.1), LinearRgba::rgb(1.2, 0.8, 0.1)),
            // Bazooka: red.
            make(Color::srgb(1.0, 0.2, 0.15), LinearRgba::rgb(1.6, 0.2, 0.1)),
            // Grenade launcher: green.
            make(Color::srgb(0.25, 0.95, 0.3), LinearRgba::rgb(0.3, 1.5, 0.3)),
        ],
    });
}

fn spawn_crate(commands: &mut Commands, assets: &PickupAssets, point: usize, counter: usize) {
    let kind_index = counter % KINDS.len();
    commands.spawn((
        Name::new(format!("Weapon crate {point}")),
        WeaponCrate {
            point,
            kind: KINDS[kind_index],
        },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.materials[kind_index].clone()),
        Transform::from_translation(SPAWN_POINTS[point]),
        Collider::cuboid(1.0, 1.0, 1.0),
        Sensor,
        CollisionEventsEnabled,
    ));
}

/// Tick empty spawn points; respawn their crate when the countdown ends.
fn respawn_crates(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<PickupAssets>,
    mut timers: ResMut<RespawnTimers>,
    mut counter: ResMut<CrateCounter>,
    crates: Query<&WeaponCrate>,
) {
    let mut occupied = [false; SPAWN_POINTS.len()];
    for c in &crates {
        occupied[c.point] = true;
    }
    for (point, timer) in timers.0.iter_mut().enumerate() {
        if occupied[point] {
            continue;
        }
        *timer -= time.delta_secs();
        if *timer <= 0.0 {
            spawn_crate(&mut commands, &assets, point, counter.0);
            counter.0 += 1;
        }
    }
}

/// Drive over a crate to swap to its weapon (full ammo).
fn collect_crates(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    mut sfx: MessageWriter<PlaySfx>,
    crates: Query<(&WeaponCrate, &Transform)>,
    mut timers: ResMut<RespawnTimers>,
    mut cars: Query<&mut WeaponSlot>,
) {
    for event in collisions.read() {
        let pairs = [
            (event.collider1, event.collider2, event.body2),
            (event.collider2, event.collider1, event.body1),
        ];
        for (maybe_crate, other, other_body) in pairs {
            let Ok((weapon_crate, transform)) = crates.get(maybe_crate) else {
                continue;
            };
            let collector = other_body.unwrap_or(other);
            let Ok(mut slot) = cars.get_mut(collector) else {
                continue;
            };
            slot.kind = Some(weapon_crate.kind);
            slot.ammo = weapon_crate.kind.refill_ammo();
            timers.0[weapon_crate.point] = RESPAWN_SECONDS;
            sfx.write(PlaySfx {
                kind: SfxKind::Pickup,
                position: Some(transform.translation),
            });
            commands.entity(maybe_crate).try_despawn();
            info!("Picked up {:?} ({} rounds)", slot.kind, slot.ammo);
        }
    }
}

/// Cosmetic idle spin + bob.
fn spin_crates(time: Res<Time>, mut crates: Query<(&WeaponCrate, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (weapon_crate, mut transform) in &mut crates {
        transform.rotate_y(time.delta_secs() * 1.5);
        transform.translation.y =
            SPAWN_POINTS[weapon_crate.point].y + (t * 2.0 + weapon_crate.point as f32).sin() * 0.15;
    }
}
