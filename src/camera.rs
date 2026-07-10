//! Fixed shared isometric camera, Gene Rally style: one static view that
//! frames the whole arena. No panning, no zooming — the arena is designed
//! to the camera (design §6), so everyone reads the same tiny-diorama scene.
//!
//! [`CameraShake`] adds brief positional punch on explosions, recoil, and hits
//! while keeping the overall framing fixed.

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
/// Max world-unit shake offset at full trauma.
const SHAKE_MAX_OFFSET: f32 = 0.45;
/// Trauma decay rate (1/s); higher = snappier settle.
const SHAKE_DECAY: f32 = 14.0;

/// Decaying shake intensity in `[0, 1]`.
#[derive(Resource, Default)]
pub struct CameraShake {
    pub trauma: f32,
}

/// Request a camera punch; intensities are typicaly `0.05`–`0.5`.
#[derive(Message, Clone, Copy, Debug)]
pub struct ShakeCamera {
    pub intensity: f32,
}

/// Stores the unshaken camera pose.
#[derive(Component)]
struct IsoCamera {
    base: Transform,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            .add_message::<ShakeCamera>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (absorb_shake, apply_camera_shake).chain());
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
        IsoCamera { base: transform },
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

fn absorb_shake(mut shake: ResMut<CameraShake>, mut events: MessageReader<ShakeCamera>) {
    for event in events.read() {
        shake.trauma = (shake.trauma + event.intensity).min(1.0);
    }
}

fn apply_camera_shake(
    time: Res<Time>,
    mut shake: ResMut<CameraShake>,
    camera: Single<(&IsoCamera, &mut Transform), With<Camera3d>>,
) {
    let dt = time.delta_secs();
    shake.trauma = (shake.trauma - SHAKE_DECAY * dt).max(0.0);

    let (iso, mut transform) = camera.into_inner();
    if shake.trauma <= 0.001 {
        *transform = iso.base;
        return;
    }

    let amp = shake.trauma * shake.trauma * SHAKE_MAX_OFFSET;
    let t = time.elapsed_secs();
    // Cheap deterministic noise — no rand crate.
    let nx = ((t * 17.3).sin() * 43758.5453).fract() * 2.0 - 1.0;
    let ny = ((t * 23.7).sin() * 43758.5453).fract() * 2.0 - 1.0;

    let right = *iso.base.right();
    let up = *iso.base.up();
    transform.translation = iso.base.translation + right * nx * amp + up * ny * amp;
    transform.rotation = iso.base.rotation;
}