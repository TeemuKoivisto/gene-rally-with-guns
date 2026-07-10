//! Fixed shared isometric camera, Gene Rally style: one static view that
//! frames the whole arena. No panning, no zooming — the arena is designed
//! to the camera (design §6), so everyone reads the same tiny-diorama scene.

use bevy::camera::ScalingMode;
use bevy::prelude::*;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z};

/// Fixed camera offset from the arena center: elevated and pulled back along
/// +Z only, so the rectangular arena reads horizontally on screen (no diamond).
const ISO_OFFSET: Vec3 = Vec3::new(0.0, 45.0, 40.0);
/// Ortho viewport height in world units when projection scale is 1.0.
const BASE_VIEWPORT_HEIGHT: f32 = 40.0;
/// World-units margin around the arena in the framing.
const FIT_MARGIN: f32 = 4.0;
/// Tallest world geometry that must stay in frame (walls, buildings).
const MAX_GEOMETRY_HEIGHT: f32 = 3.0;
/// Assumed window aspect for framing; close enough for a fixed shot.
const ASPECT: f32 = 16.0 / 9.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera);
    }
}

fn spawn_camera(mut commands: Commands) {
    let focus = Vec3::ZERO;
    let transform = Transform::from_translation(focus + ISO_OFFSET).looking_at(focus, Vec3::Y);

    // Project the arena's bounding corners onto the camera plane and pick the
    // orthographic scale that fits them all, so the whole map is always visible.
    let cam_right = *transform.right();
    let cam_up = *transform.up();
    let (mut extent_r, mut extent_u) = (0.0f32, 0.0f32);
    for x in [-ARENA_HALF_X, ARENA_HALF_X] {
        for y in [0.0, MAX_GEOMETRY_HEIGHT] {
            for z in [-ARENA_HALF_Z, ARENA_HALF_Z] {
                let corner = Vec3::new(x, y, z) - focus;
                extent_r = extent_r.max(corner.dot(cam_right).abs());
                extent_u = extent_u.max(corner.dot(cam_up).abs());
            }
        }
    }
    let needed_height =
        (extent_u * 2.0 + FIT_MARGIN).max((extent_r * 2.0 + FIT_MARGIN) / ASPECT);

    commands.spawn((
        Name::new("Iso camera"),
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: BASE_VIEWPORT_HEIGHT,
            },
            scale: needed_height / BASE_VIEWPORT_HEIGHT,
            ..OrthographicProjection::default_3d()
        }),
        transform,
    ));
}