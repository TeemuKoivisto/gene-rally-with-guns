//! Toy-city arena built from city blocks: segmented streets, river, park, buildings.

use avian3d::prelude::*;
use bevy::prelude::*;

pub const ARENA_HALF_X: f32 = 36.0;
pub const ARENA_HALF_Z: f32 = 22.0;
pub const MAX_GEOMETRY_HEIGHT: f32 = 6.5;

/// Static obstacle AABBs for physics + pathfinding: (center, dimensions).
pub const BLOCKS: [(Vec3, Vec3); 18] = [
    // West district — one building per city block (clear of road corridors)
    (Vec3::new(-33.0, 1.2, -19.0), Vec3::new(3.8, 2.4, 1.8)),
    (Vec3::new(-17.0, 2.4, -19.0), Vec3::new(9.5, 4.8, 1.8)),
    (Vec3::new(0.0, 1.5, -19.0), Vec3::new(4.8, 3.0, 1.8)),
    (Vec3::new(-33.0, 1.1, 7.0), Vec3::new(3.8, 2.2, 5.0)),
    (Vec3::new(-17.0, 1.7, 7.0), Vec3::new(6.5, 3.4, 5.5)),
    (Vec3::new(0.0, 1.2, 7.0), Vec3::new(4.8, 2.4, 5.0)),
    // East district
    (Vec3::new(19.0, 2.2, -19.0), Vec3::new(3.8, 4.4, 1.8)),
    (Vec3::new(31.0, 1.7, -19.0), Vec3::new(5.5, 3.4, 1.8)),
    (Vec3::new(31.0, 2.8, 7.0), Vec3::new(5.5, 5.6, 6.0)),
    (Vec3::new(31.0, 1.5, 18.5), Vec3::new(6.5, 3.0, 2.8)),
    // River (impassable except bridges at z = -14, 0, 14)
    (Vec3::new(11.0, 0.35, -18.5), Vec3::new(8.0, 0.7, 3.0)),
    (Vec3::new(11.0, 0.35, -7.0), Vec3::new(8.0, 0.7, 8.0)),
    (Vec3::new(11.0, 0.35, 7.0), Vec3::new(8.0, 0.7, 8.0)),
    (Vec3::new(11.0, 0.35, 18.5), Vec3::new(8.0, 0.7, 3.0)),
    // Park trees
    (Vec3::new(-32.0, 0.55, -6.0), Vec3::new(0.5, 1.1, 0.5)),
    (Vec3::new(-34.5, 0.55, -8.5), Vec3::new(0.5, 1.1, 0.5)),
    (Vec3::new(-30.5, 0.55, -8.0), Vec3::new(0.5, 1.1, 0.5)),
    (Vec3::new(-33.5, 0.55, -4.0), Vec3::new(0.5, 1.1, 0.5)),
];

const WALL_HEIGHT: f32 = 2.0;
const WALL_THICKNESS: f32 = 1.0;

const ROAD_W: f32 = 4.0;
const PAVE_W: f32 = 1.1;
const ROAD_Y: f32 = 0.05;
const PAVE_Y: f32 = 0.04;

const ROADS_X: [f32; 3] = [-26.0, -8.0, 24.0];
const ROADS_Z: [f32; 3] = [-14.0, 0.0, 14.0];

const RIVER_X: f32 = 11.0;
const RIVER_HALF_W: f32 = 4.0;
const RIVER_WEST: f32 = 6.0;
const RIVER_EAST: f32 = 16.0;
const MAP_WEST: f32 = -35.0;
const MAP_EAST: f32 = 35.0;
const MAP_NORTH: f32 = -20.0;
const MAP_SOUTH: f32 = 20.0;

/// Top Y of the base grass plane.
const GRASS_TOP: f32 = 0.0;
/// Top Y of park-grass slabs.
const PARK_TOP: f32 = 0.06;
/// Top Y of pavement strips beside roads.
const PAVEMENT_TOP: f32 = PAVE_Y + PAVE_Y;
/// Top Y of asphalt road surfaces.
const ROAD_TOP: f32 = ROAD_Y + ROAD_Y;
/// Top Y of bridge decks spanning the river.
const BRIDGE_TOP: f32 = ROAD_Y + 0.06 + 0.07;
/// Top Y of pavement strips on bridge decks.
const BRIDGE_PAVEMENT_TOP: f32 = PAVE_Y + 0.05 + 0.05;

/// World-space Y of the highest ground surface at `(x, z)`.
///
/// Mirrors the slab layout in [`spawn_street_network`] / [`spawn_bridges`] so
/// decals (skid marks, etc.) sit just above whichever surface the wheels are on.
pub fn ground_surface_y(x: f32, z: f32) -> f32 {
    let mut y = GRASS_TOP;

    if in_xz_rect(x, z, PARK_CENTER, PARK_SIZE) {
        y = y.max(PARK_TOP);
    }

    for &rz in &ROADS_Z {
        y = y.max(h_road_surface_y(x, z, MAP_WEST, RIVER_WEST, rz));
        y = y.max(h_road_surface_y(x, z, RIVER_EAST, MAP_EAST, rz));
        y = y.max(bridge_surface_y(x, z, rz));
    }

    for &rx in &ROADS_X {
        y = y.max(v_road_surface_y(x, z, rx, MAP_NORTH, MAP_SOUTH));
    }

    y
}

fn in_xz_rect(x: f32, z: f32, center: Vec3, size: Vec3) -> bool {
    (x - center.x).abs() <= size.x * 0.5 && (z - center.z).abs() <= size.z * 0.5
}

fn h_road_surface_y(x: f32, z: f32, x0: f32, x1: f32, rz: f32) -> f32 {
    if x < x0 || x > x1 {
        return GRASS_TOP;
    }
    let dz = (z - rz).abs();
    if dz <= ROAD_W * 0.5 {
        ROAD_TOP
    } else if dz <= ROAD_W * 0.5 + PAVE_W {
        PAVEMENT_TOP
    } else {
        GRASS_TOP
    }
}

fn v_road_surface_y(x: f32, z: f32, rx: f32, z0: f32, z1: f32) -> f32 {
    if z < z0 || z > z1 {
        return GRASS_TOP;
    }
    let dx = (x - rx).abs();
    if dx <= ROAD_W * 0.5 {
        ROAD_TOP
    } else if dx <= ROAD_W * 0.5 + PAVE_W {
        PAVEMENT_TOP
    } else {
        GRASS_TOP
    }
}

fn bridge_surface_y(x: f32, z: f32, rz: f32) -> f32 {
    if x < RIVER_WEST || x > RIVER_EAST {
        return GRASS_TOP;
    }
    let dz = (z - rz).abs();
    if dz <= (ROAD_W + 0.4) * 0.5 {
        BRIDGE_TOP
    } else if dz <= ROAD_W * 0.5 + PAVE_W {
        BRIDGE_PAVEMENT_TOP
    } else {
        GRASS_TOP
    }
}

#[derive(Clone, Copy)]
enum BuildingKind {
    Residential,
    Shop,
    Office,
    Warehouse,
    Civic,
}

struct BuildingDef {
    center: Vec3,
    size: Vec3,
    kind: BuildingKind,
}

const BUILDINGS: [BuildingDef; 10] = [
    BuildingDef {
        center: Vec3::new(-33.0, 1.2, -19.0),
        size: Vec3::new(3.8, 2.4, 1.8),
        kind: BuildingKind::Residential,
    },
    BuildingDef {
        center: Vec3::new(-17.0, 2.4, -19.0),
        size: Vec3::new(9.5, 4.8, 1.8),
        kind: BuildingKind::Office,
    },
    BuildingDef {
        center: Vec3::new(0.0, 1.5, -19.0),
        size: Vec3::new(4.8, 3.0, 1.8),
        kind: BuildingKind::Shop,
    },
    BuildingDef {
        center: Vec3::new(-33.0, 1.1, 7.0),
        size: Vec3::new(3.8, 2.2, 5.0),
        kind: BuildingKind::Residential,
    },
    BuildingDef {
        center: Vec3::new(-17.0, 1.7, 7.0),
        size: Vec3::new(6.5, 3.4, 5.5),
        kind: BuildingKind::Civic,
    },
    BuildingDef {
        center: Vec3::new(0.0, 1.2, 7.0),
        size: Vec3::new(4.8, 2.4, 5.0),
        kind: BuildingKind::Residential,
    },
    BuildingDef {
        center: Vec3::new(19.0, 2.2, -19.0),
        size: Vec3::new(3.8, 4.4, 1.8),
        kind: BuildingKind::Office,
    },
    BuildingDef {
        center: Vec3::new(31.0, 1.7, -19.0),
        size: Vec3::new(5.5, 3.4, 1.8),
        kind: BuildingKind::Shop,
    },
    BuildingDef {
        center: Vec3::new(31.0, 2.8, 7.0),
        size: Vec3::new(5.5, 5.6, 6.0),
        kind: BuildingKind::Office,
    },
    BuildingDef {
        center: Vec3::new(31.0, 1.5, 18.5),
        size: Vec3::new(6.5, 3.0, 2.8),
        kind: BuildingKind::Warehouse,
    },
];

/// Southwest park lot — tucked west of the x = -26 arterial.
const PARK_CENTER: Vec3 = Vec3::new(-33.0, 0.0, -7.0);
const PARK_SIZE: Vec3 = Vec3::new(7.0, 0.08, 8.0);

const PARK_TREES: [Vec3; 6] = [
    Vec3::new(-32.0, 0.0, -6.0),
    Vec3::new(-34.5, 0.0, -8.5),
    Vec3::new(-30.5, 0.0, -8.0),
    Vec3::new(-33.5, 0.0, -4.0),
    Vec3::new(-35.0, 0.0, -5.5),
    Vec3::new(-31.0, 0.0, -9.5),
];

#[derive(Resource, Clone)]
struct ArenaMaterials {
    grass: Handle<StandardMaterial>,
    park_grass: Handle<StandardMaterial>,
    asphalt: Handle<StandardMaterial>,
    pavement: Handle<StandardMaterial>,
    water: Handle<StandardMaterial>,
    river_bank: Handle<StandardMaterial>,
    bridge: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    residential: Handle<StandardMaterial>,
    shop: Handle<StandardMaterial>,
    office: Handle<StandardMaterial>,
    warehouse: Handle<StandardMaterial>,
    civic: Handle<StandardMaterial>,
    roof_accent: Handle<StandardMaterial>,
    trunk: Handle<StandardMaterial>,
    foliage: Handle<StandardMaterial>,
    wood_crate: Handle<StandardMaterial>,
}

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_materials)
            .add_systems(Startup, spawn_arena.after(setup_materials));
    }
}

fn setup_materials(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ArenaMaterials {
        grass: materials.add(StandardMaterial {
            base_color: Color::srgb(0.48, 0.52, 0.42),
            perceptual_roughness: 0.95,
            ..default()
        }),
        park_grass: materials.add(StandardMaterial {
            base_color: Color::srgb(0.38, 0.68, 0.34),
            perceptual_roughness: 0.9,
            ..default()
        }),
        asphalt: materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.29, 0.32),
            perceptual_roughness: 0.88,
            ..default()
        }),
        pavement: materials.add(StandardMaterial {
            base_color: Color::srgb(0.68, 0.67, 0.64),
            perceptual_roughness: 0.82,
            ..default()
        }),
        water: materials.add(StandardMaterial {
            base_color: Color::srgba(0.18, 0.45, 0.82, 0.95),
            perceptual_roughness: 0.08,
            metallic: 0.1,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        river_bank: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.5, 0.46),
            perceptual_roughness: 0.85,
            ..default()
        }),
        bridge: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.6, 0.56),
            perceptual_roughness: 0.72,
            ..default()
        }),
        wall: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.53, 0.50),
            perceptual_roughness: 0.9,
            ..default()
        }),
        residential: materials.add(StandardMaterial {
            base_color: Color::srgb(0.84, 0.58, 0.5),
            perceptual_roughness: 0.75,
            ..default()
        }),
        shop: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.7, 0.84),
            perceptual_roughness: 0.7,
            ..default()
        }),
        office: materials.add(StandardMaterial {
            base_color: Color::srgb(0.56, 0.58, 0.64),
            perceptual_roughness: 0.55,
            metallic: 0.12,
            ..default()
        }),
        warehouse: materials.add(StandardMaterial {
            base_color: Color::srgb(0.64, 0.6, 0.52),
            perceptual_roughness: 0.9,
            ..default()
        }),
        civic: materials.add(StandardMaterial {
            base_color: Color::srgb(0.74, 0.64, 0.8),
            perceptual_roughness: 0.65,
            ..default()
        }),
        roof_accent: materials.add(StandardMaterial {
            base_color: Color::srgb(0.36, 0.38, 0.42),
            perceptual_roughness: 0.8,
            ..default()
        }),
        trunk: materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.3, 0.2),
            perceptual_roughness: 0.95,
            ..default()
        }),
        foliage: materials.add(StandardMaterial {
            base_color: Color::srgb(0.24, 0.55, 0.26),
            perceptual_roughness: 0.85,
            ..default()
        }),
        wood_crate: materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.6, 0.35),
            perceptual_roughness: 0.8,
            ..default()
        }),
    });
}

fn spawn_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<ArenaMaterials>,
) {
    let mats = mats.as_ref();
    let size_x = ARENA_HALF_X * 2.0;
    let size_z = ARENA_HALF_Z * 2.0;

    spawn_base_ground(&mut commands, &mut meshes, mats, size_x, size_z);
    spawn_park(&mut commands, &mut meshes, mats);
    spawn_river(&mut commands, &mut meshes, mats);
    spawn_street_network(&mut commands, &mut meshes, mats);
    spawn_bridges(&mut commands, &mut meshes, mats);
    spawn_buildings(&mut commands, &mut meshes, mats);
    spawn_trees(&mut commands, &mut meshes, mats);
    spawn_boundary_walls(&mut commands, &mut meshes, mats, size_x, size_z);
    spawn_crates(&mut commands, &mut meshes, mats);
    spawn_light(&mut commands);
}

fn spawn_base_ground(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    mats: &ArenaMaterials,
    size_x: f32,
    size_z: f32,
) {
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Cuboid::new(size_x, 1.0, size_z))),
        MeshMaterial3d(mats.grass.clone()),
        Transform::from_xyz(0.0, -0.5, 0.0),
        RigidBody::Static,
        Collider::cuboid(size_x, 1.0, size_z),
    ));
}

fn spawn_park(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    spawn_slab(
        commands,
        meshes,
        mats.park_grass.clone(),
        PARK_CENTER + Vec3::Y * 0.02,
        PARK_SIZE,
    );
}

fn spawn_river(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    let depth = 0.35;
    commands.spawn((
        Name::new("River water"),
        Mesh3d(meshes.add(Cuboid::new(
            RIVER_HALF_W * 2.0,
            depth,
            MAP_SOUTH - MAP_NORTH,
        ))),
        MeshMaterial3d(mats.water.clone()),
        Transform::from_xyz(RIVER_X, -depth / 2.0 + 0.02, 0.0),
    ));

    for side in [-1.0, 1.0] {
        spawn_slab(
            commands,
            meshes,
            mats.river_bank.clone(),
            Vec3::new(RIVER_X + side * (RIVER_HALF_W + 0.45), 0.03, 0.0),
            Vec3::new(0.7, 0.06, MAP_SOUTH - MAP_NORTH),
        );
    }

    for (i, &(center, dim)) in BLOCKS[10..14].iter().enumerate() {
        commands.spawn((
            Name::new(format!("River collider {i}")),
            Transform::from_translation(center),
            RigidBody::Static,
            Collider::cuboid(dim.x, dim.y, dim.z),
        ));
    }
}

fn spawn_street_network(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    for rz in ROADS_Z {
        spawn_h_road(commands, meshes, mats, MAP_WEST, RIVER_WEST, rz);
        spawn_h_road(commands, meshes, mats, RIVER_EAST, MAP_EAST, rz);
    }

    for rx in ROADS_X {
        spawn_v_road(commands, meshes, mats, rx, MAP_NORTH, MAP_SOUTH);
    }

}

fn spawn_h_road(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    mats: &ArenaMaterials,
    x0: f32,
    x1: f32,
    z: f32,
) {
    let len = x1 - x0;
    let cx = (x0 + x1) * 0.5;
    spawn_slab(
        commands,
        meshes,
        mats.asphalt.clone(),
        Vec3::new(cx, ROAD_Y, z),
        Vec3::new(len, ROAD_Y * 2.0, ROAD_W),
    );
    for side in [-1.0, 1.0] {
        spawn_slab(
            commands,
            meshes,
            mats.pavement.clone(),
            Vec3::new(cx, PAVE_Y, z + side * (ROAD_W / 2.0 + PAVE_W / 2.0)),
            Vec3::new(len, PAVE_Y * 2.0, PAVE_W),
        );
    }
}

fn spawn_v_road(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    mats: &ArenaMaterials,
    x: f32,
    z0: f32,
    z1: f32,
) {
    let len = z1 - z0;
    let cz = (z0 + z1) * 0.5;
    spawn_slab(
        commands,
        meshes,
        mats.asphalt.clone(),
        Vec3::new(x, ROAD_Y, cz),
        Vec3::new(ROAD_W, ROAD_Y * 2.0, len),
    );
    for side in [-1.0, 1.0] {
        spawn_slab(
            commands,
            meshes,
            mats.pavement.clone(),
            Vec3::new(x + side * (ROAD_W / 2.0 + PAVE_W / 2.0), PAVE_Y, cz),
            Vec3::new(PAVE_W, PAVE_Y * 2.0, len),
        );
    }
}

fn spawn_bridges(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    let deck_w = RIVER_EAST - RIVER_WEST;
    for rz in ROADS_Z {
        spawn_slab(
            commands,
            meshes,
            mats.bridge.clone(),
            Vec3::new((RIVER_WEST + RIVER_EAST) * 0.5, ROAD_Y + 0.06, rz),
            Vec3::new(deck_w, 0.14, ROAD_W + 0.4),
        );
        for side in [-1.0, 1.0] {
            spawn_slab(
                commands,
                meshes,
                mats.pavement.clone(),
                Vec3::new(
                    (RIVER_WEST + RIVER_EAST) * 0.5,
                    PAVE_Y + 0.05,
                    rz + side * (ROAD_W / 2.0 + PAVE_W / 2.0),
                ),
                Vec3::new(deck_w, 0.1, PAVE_W),
            );
        }
    }
}

fn spawn_buildings(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    for (i, building) in BUILDINGS.iter().enumerate() {
        let body_mat = match building.kind {
            BuildingKind::Residential => mats.residential.clone(),
            BuildingKind::Shop => mats.shop.clone(),
            BuildingKind::Office => mats.office.clone(),
            BuildingKind::Warehouse => mats.warehouse.clone(),
            BuildingKind::Civic => mats.civic.clone(),
        };
        commands.spawn((
            Name::new(format!("Building {i}")),
            Mesh3d(meshes.add(Cuboid::new(
                building.size.x,
                building.size.y,
                building.size.z,
            ))),
            MeshMaterial3d(body_mat),
            Transform::from_translation(building.center),
            RigidBody::Static,
            Collider::cuboid(building.size.x, building.size.y, building.size.z),
        ));

        let roof_h = 0.22;
        spawn_slab(
            commands,
            meshes,
            mats.roof_accent.clone(),
            building.center + Vec3::new(0.0, building.size.y / 2.0 + roof_h / 2.0, 0.0),
            Vec3::new(building.size.x * 0.9, roof_h, building.size.z * 0.9),
        );
    }
}

fn spawn_trees(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    let trunk_mesh = meshes.add(Cylinder::new(0.16, 0.75));
    let foliage_mesh = meshes.add(Sphere::new(0.75));

    for (i, base) in PARK_TREES.iter().enumerate() {
        let collides = i < 4;
        let mut trunk = commands.spawn((
            Mesh3d(trunk_mesh.clone()),
            MeshMaterial3d(mats.trunk.clone()),
            Transform::from_translation(*base + Vec3::new(0.0, 0.38, 0.0)),
        ));
        if collides {
            trunk.insert((
                RigidBody::Static,
                Collider::cylinder(0.38, 0.16),
            ));
        }
        commands.spawn((
            Mesh3d(foliage_mesh.clone()),
            MeshMaterial3d(mats.foliage.clone()),
            Transform::from_translation(*base + Vec3::new(0.0, 1.15, 0.0)),
        ));
    }
}

fn spawn_boundary_walls(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    mats: &ArenaMaterials,
    size_x: f32,
    size_z: f32,
) {
    let length_x = size_x + WALL_THICKNESS * 2.0;
    let walls = [
        (
            Vec3::new(0.0, WALL_HEIGHT / 2.0, -(ARENA_HALF_Z + WALL_THICKNESS / 2.0)),
            Vec3::new(length_x, WALL_HEIGHT, WALL_THICKNESS),
        ),
        (
            Vec3::new(0.0, WALL_HEIGHT / 2.0, ARENA_HALF_Z + WALL_THICKNESS / 2.0),
            Vec3::new(length_x, WALL_HEIGHT, WALL_THICKNESS),
        ),
        (
            Vec3::new(-(ARENA_HALF_X + WALL_THICKNESS / 2.0), WALL_HEIGHT / 2.0, 0.0),
            Vec3::new(WALL_THICKNESS, WALL_HEIGHT, size_z),
        ),
        (
            Vec3::new(ARENA_HALF_X + WALL_THICKNESS / 2.0, WALL_HEIGHT / 2.0, 0.0),
            Vec3::new(WALL_THICKNESS, WALL_HEIGHT, size_z),
        ),
    ];
    for (i, (pos, dim)) in walls.into_iter().enumerate() {
        commands.spawn((
            Name::new(format!("Wall {i}")),
            Mesh3d(meshes.add(Cuboid::new(dim.x, dim.y, dim.z))),
            MeshMaterial3d(mats.wall.clone()),
            Transform::from_translation(pos),
            RigidBody::Static,
            Collider::cuboid(dim.x, dim.y, dim.z),
        ));
    }
}

fn spawn_crates(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, mats: &ArenaMaterials) {
    let crate_mesh = meshes.add(Cuboid::new(0.9, 0.9, 0.9));
    let alley = Vec3::new(0.0, 0.45, -7.0);
    for (dx, dz) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
        commands.spawn((
            Mesh3d(crate_mesh.clone()),
            MeshMaterial3d(mats.wood_crate.clone()),
            Transform::from_translation(alley + Vec3::new(dx, 0.0, dz)),
            RigidBody::Dynamic,
            Collider::cuboid(0.9, 0.9, 0.9),
            Mass(2.0),
        ));
    }
}

fn spawn_slab(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    material: Handle<StandardMaterial>,
    center: Vec3,
    size: Vec3,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
        MeshMaterial3d(material),
        Transform::from_translation(center),
    ));
}

fn spawn_light(commands: &mut Commands) {
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 16_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_at(Vec3::new(-0.45, -1.0, -0.4), Vec3::Y),
    ));
}