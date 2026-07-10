//! Shared isometric camera with Micro Machines–style framing: zooms in on the
//! player cluster for couch-TV legibility, but never zooms out past the full
//! arena so the whole map stays playable.

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::arena::{ARENA_HALF_X, ARENA_HALF_Z};
use crate::vehicle::{Car, Player};

/// Fixed camera offset from the focus point: elevated and pulled back along
/// +Z only, so the rectangular arena reads horizontally on screen (no diamond).
const ISO_OFFSET: Vec3 = Vec3::new(0.0, 45.0, 40.0);
/// Ortho viewport height in world units when projection scale is 1.0.
const BASE_VIEWPORT_HEIGHT: f32 = 40.0;
/// World-units margin around the arena in the widest (full-map) shot.
const FIT_MARGIN: f32 = 4.0;
/// Tallest world geometry that must stay in frame (walls, buildings).
const MAX_GEOMETRY_HEIGHT: f32 = 3.0;
/// Extra padding around the player cluster when zoomed in.
const PLAYER_FRAME_PADDING: f32 = 9.0;
/// Minimum half-extent of the framing box so a lone survivor still gets a
/// readable zoom level on a big TV.
const MIN_FRAME_HALF_EXTENT: f32 = 13.0;
/// Tightest zoom: ortho viewport height in world units (cars ~4 units long).
const MIN_VIEWPORT_HEIGHT: f32 = 24.0;
const FOCUS_SMOOTH: f32 = 5.0;
const ZOOM_SMOOTH: f32 = 4.0;

#[derive(Component)]
struct IsoCamera;

/// Full-arena framing computed once at startup.
#[derive(Resource)]
struct ArenaFraming {
    full_viewport_height: f32,
    cam_right: Vec3,
    cam_up: Vec3,
}

/// Smoothed live framing state.
#[derive(Resource)]
struct LiveFraming {
    focus: Vec3,
    viewport_height: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, update_framing);
    }
}

fn arena_full_viewport(cam_right: Vec3, cam_up: Vec3, aspect: f32) -> f32 {
    let focus = Vec3::ZERO;
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
    (extent_u * 2.0 + FIT_MARGIN).max((extent_r * 2.0 + FIT_MARGIN) / aspect)
}

fn spawn_camera(mut commands: Commands, window: Query<&Window, With<PrimaryWindow>>) {
    let focus = Vec3::ZERO;
    let transform = Transform::from_translation(focus + ISO_OFFSET).looking_at(focus, Vec3::Y);
    let cam_right = *transform.right();
    let cam_up = *transform.up();
    let aspect = window
        .single()
        .map(|w| w.resolution.width() / w.resolution.height())
        .unwrap_or(16.0 / 9.0);
    let full_height = arena_full_viewport(cam_right, cam_up, aspect);

    commands.insert_resource(ArenaFraming {
        full_viewport_height: full_height,
        cam_right,
        cam_up,
    });
    commands.insert_resource(LiveFraming {
        focus,
        viewport_height: full_height,
    });

    commands.spawn((
        Name::new("Iso camera"),
        IsoCamera,
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: BASE_VIEWPORT_HEIGHT,
            },
            scale: full_height / BASE_VIEWPORT_HEIGHT,
            ..OrthographicProjection::default_3d()
        }),
        transform,
    ));
}

/// Ortho viewport height needed to fit an XZ bounds box on screen.
fn viewport_for_bounds(
    center: Vec3,
    half_x: f32,
    half_z: f32,
    cam_right: Vec3,
    cam_up: Vec3,
    aspect: f32,
) -> f32 {
    let corners = [
        Vec3::new(center.x - half_x, 0.0, center.z - half_z),
        Vec3::new(center.x + half_x, 0.0, center.z - half_z),
        Vec3::new(center.x - half_x, 0.0, center.z + half_z),
        Vec3::new(center.x + half_x, 0.0, center.z + half_z),
    ];
    let (mut extent_r, mut extent_u) = (0.0f32, 0.0f32);
    for corner in corners {
        let rel = corner - center;
        extent_r = extent_r.max(rel.dot(cam_right).abs());
        extent_u = extent_u.max(rel.dot(cam_up).abs());
    }
    let padded_r = extent_r + PLAYER_FRAME_PADDING;
    let padded_u = extent_u + PLAYER_FRAME_PADDING;
    (padded_u * 2.0).max((padded_r * 2.0) / aspect)
}

fn update_framing(
    time: Res<Time>,
    arena: Res<ArenaFraming>,
    mut live: ResMut<LiveFraming>,
    window: Query<&Window, With<PrimaryWindow>>,
    players: Query<&Transform, (With<Player>, With<Car>, Without<IsoCamera>)>,
    mut camera: Query<
        (&mut Transform, &mut Projection),
        (With<IsoCamera>, Without<Car>),
    >,
) {
    let Ok((mut cam_transform, mut projection)) = camera.single_mut() else {
        return;
    };
    let aspect = window
        .single()
        .map(|w| w.resolution.width() / w.resolution.height())
        .unwrap_or(16.0 / 9.0);

    let dt = time.delta_secs();
    let full = arena.full_viewport_height;

    let (target_focus, target_height) = match players.iter().len() {
        0 => (Vec3::ZERO, full),
        _ => {
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            for transform in &players {
                let p = transform.translation;
                min = min.min(p);
                max = max.max(p);
            }
            let centroid = (min + max) * 0.5;
            let mut half_x = ((max.x - min.x) * 0.5).max(MIN_FRAME_HALF_EXTENT);
            let mut half_z = ((max.z - min.z) * 0.5).max(MIN_FRAME_HALF_EXTENT);
            half_x = half_x.min(ARENA_HALF_X);
            half_z = half_z.min(ARENA_HALF_Z);

            let cluster_height = viewport_for_bounds(
                centroid,
                half_x,
                half_z,
                arena.cam_right,
                arena.cam_up,
                aspect,
            );
            let height = cluster_height.clamp(MIN_VIEWPORT_HEIGHT, full);

            // Pull focus toward the cluster when zoomed in; stay centered at full zoom.
            let zoom_t = 1.0 - (height - MIN_VIEWPORT_HEIGHT) / (full - MIN_VIEWPORT_HEIGHT);
            let focus = Vec3::new(centroid.x * zoom_t, 0.0, centroid.z * zoom_t);
            (focus, height)
        }
    };

    let blend_focus = 1.0 - (-FOCUS_SMOOTH * dt).exp();
    let blend_zoom = 1.0 - (-ZOOM_SMOOTH * dt).exp();
    let focus = live.focus;
    let viewport_height = live.viewport_height;
    live.focus += (target_focus - focus) * blend_focus;
    live.viewport_height += (target_height - viewport_height) * blend_zoom;

    cam_transform.translation = live.focus + ISO_OFFSET;
    *cam_transform = cam_transform.looking_at(live.focus, Vec3::Y);

    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = live.viewport_height / BASE_VIEWPORT_HEIGHT;
    }
}