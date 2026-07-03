//! The arena: flat ground, bounding walls, and a few props to bump into.
//! M0 uses a plain bounded lot; the toy-city block comes with MapPlugin later.

use avian3d::prelude::*;
use bevy::prelude::*;

/// Half-extent of the square drivable area, in world units (~toy meters).
pub const ARENA_HALF: f32 = 30.0;
const WALL_HEIGHT: f32 = 2.0;
const WALL_THICKNESS: f32 = 1.0;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_arena, spawn_props, spawn_light));
    }
}

fn spawn_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let size = ARENA_HALF * 2.0;

    // Ground slab. Kept deliberately desaturated so player cars pop (design §10).
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Cuboid::new(size, 1.0, size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.37, 0.40),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.5, 0.0),
        RigidBody::Static,
        Collider::cuboid(size, 1.0, size),
    ));

    // Four bounding walls.
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.53, 0.50),
        perceptual_roughness: 0.9,
        ..default()
    });
    let wall_length = size + WALL_THICKNESS * 2.0;
    let walls = [
        // (position, dimensions)
        (
            Vec3::new(0.0, WALL_HEIGHT / 2.0, -(ARENA_HALF + WALL_THICKNESS / 2.0)),
            Vec3::new(wall_length, WALL_HEIGHT, WALL_THICKNESS),
        ),
        (
            Vec3::new(0.0, WALL_HEIGHT / 2.0, ARENA_HALF + WALL_THICKNESS / 2.0),
            Vec3::new(wall_length, WALL_HEIGHT, WALL_THICKNESS),
        ),
        (
            Vec3::new(-(ARENA_HALF + WALL_THICKNESS / 2.0), WALL_HEIGHT / 2.0, 0.0),
            Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_length),
        ),
        (
            Vec3::new(ARENA_HALF + WALL_THICKNESS / 2.0, WALL_HEIGHT / 2.0, 0.0),
            Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_length),
        ),
    ];
    for (i, (pos, dim)) in walls.into_iter().enumerate() {
        commands.spawn((
            Name::new(format!("Wall {i}")),
            Mesh3d(meshes.add(Cuboid::new(dim.x, dim.y, dim.z))),
            MeshMaterial3d(wall_material.clone()),
            Transform::from_translation(pos),
            RigidBody::Static,
            Collider::cuboid(dim.x, dim.y, dim.z),
        ));
    }
}

/// A few static obstacles and dynamic crates: something to drive around and smash.
fn spawn_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let block_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.48, 0.55),
        perceptual_roughness: 0.9,
        ..default()
    });
    // Static "buildings" to weave between.
    let blocks = [
        (Vec3::new(-12.0, 1.5, -10.0), Vec3::new(8.0, 3.0, 6.0)),
        (Vec3::new(14.0, 1.5, 8.0), Vec3::new(6.0, 3.0, 10.0)),
        (Vec3::new(-8.0, 1.5, 14.0), Vec3::new(10.0, 3.0, 5.0)),
        (Vec3::new(8.0, 1.5, -16.0), Vec3::new(5.0, 3.0, 7.0)),
    ];
    for (i, (pos, dim)) in blocks.into_iter().enumerate() {
        commands.spawn((
            Name::new(format!("Block {i}")),
            Mesh3d(meshes.add(Cuboid::new(dim.x, dim.y, dim.z))),
            MeshMaterial3d(block_material.clone()),
            Transform::from_translation(pos),
            RigidBody::Static,
            Collider::cuboid(dim.x, dim.y, dim.z),
        ));
    }

    // Dynamic crates: satisfying to plow through, and they exercise the physics.
    let crate_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let crate_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.6, 0.35),
        perceptual_roughness: 0.8,
        ..default()
    });
    for x in 0..3 {
        for z in 0..3 {
            commands.spawn((
                Name::new(format!("Crate {x}-{z}")),
                Mesh3d(crate_mesh.clone()),
                MeshMaterial3d(crate_material.clone()),
                Transform::from_xyz(2.0 + x as f32 * 1.1, 0.5, 2.0 + z as f32 * 1.1),
                RigidBody::Dynamic,
                Collider::cuboid(1.0, 1.0, 1.0),
                Mass(20.0),
            ));
        }
    }
}

fn spawn_light(mut commands: Commands) {
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 8000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_at(Vec3::new(-0.6, -1.0, -0.4), Vec3::Y),
    ));
}
