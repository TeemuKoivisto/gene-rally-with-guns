//! Ammo crates: fixed spawn points, sensor colliders, timed respawn (design §8).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::weapon::WeaponSlot;

const AMMO_PER_CRATE: u32 = 35;
const RESPAWN_SECONDS: f32 = 8.0;

/// Fixed crate spawn points around the arena.
const SPAWN_POINTS: [Vec3; 5] = [
    Vec3::new(0.0, 0.7, 0.0),
    Vec3::new(-26.0, 0.7, -15.0),
    Vec3::new(26.0, 0.7, -15.0),
    Vec3::new(-26.0, 0.7, 15.0),
    Vec3::new(26.0, 0.7, 15.0),
];

#[derive(Component)]
struct AmmoCrate {
    point: usize,
}

/// Per-spawn-point respawn countdowns (only ticked while the point is empty).
#[derive(Resource)]
struct RespawnTimers([f32; SPAWN_POINTS.len()]);

#[derive(Resource)]
struct PickupAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RespawnTimers([0.0; SPAWN_POINTS.len()]))
            .add_systems(Startup, setup_pickup_assets)
            .add_systems(Update, (respawn_crates, collect_crates, spin_crates));
    }
}

fn setup_pickup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PickupAssets {
        mesh: meshes.add(Cuboid::new(0.8, 0.8, 0.8)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.75, 0.1),
            emissive: LinearRgba::rgb(1.2, 0.8, 0.1),
            perceptual_roughness: 0.4,
            ..default()
        }),
    });
}

fn spawn_crate(commands: &mut Commands, assets: &PickupAssets, point: usize) {
    commands.spawn((
        Name::new(format!("Ammo crate {point}")),
        AmmoCrate { point },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
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
    crates: Query<&AmmoCrate>,
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
            spawn_crate(&mut commands, &assets, point);
        }
    }
}

/// Drive over a crate to grab its ammo.
fn collect_crates(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    crates: Query<&AmmoCrate>,
    mut timers: ResMut<RespawnTimers>,
    mut cars: Query<&mut WeaponSlot>,
) {
    for event in collisions.read() {
        let pairs = [
            (event.collider1, event.collider2, event.body2),
            (event.collider2, event.collider1, event.body1),
        ];
        for (maybe_crate, other, other_body) in pairs {
            let Ok(ammo_crate) = crates.get(maybe_crate) else {
                continue;
            };
            let collector = other_body.unwrap_or(other);
            let Ok(mut slot) = cars.get_mut(collector) else {
                continue;
            };
            slot.ammo += AMMO_PER_CRATE;
            timers.0[ammo_crate.point] = RESPAWN_SECONDS;
            commands.entity(maybe_crate).try_despawn();
            info!("Ammo collected ({} rounds)", slot.ammo);
        }
    }
}

/// Cosmetic idle spin + bob.
fn spin_crates(time: Res<Time>, mut crates: Query<(&AmmoCrate, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (ammo_crate, mut transform) in &mut crates {
        transform.rotate_y(time.delta_secs() * 1.5);
        transform.translation.y =
            SPAWN_POINTS[ammo_crate.point].y + (t * 2.0 + ammo_crate.point as f32).sin() * 0.15;
    }
}
