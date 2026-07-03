//! Shared isometric camera: orthographic, fixed angle, zoom-to-fit all cars.
//!
//! Each frame we take the bounding box of every car, project it into camera
//! space, pick the orthographic scale that fits it (plus margin), clamp to
//! [MIN, MAX] viewport height, and smooth toward it. Fewer cars alive later
//! means a naturally tighter shot (design §6).

use bevy::camera::ScalingMode;
use bevy::prelude::*;

use crate::vehicle::Car;

/// Fixed iso offset direction from the focus point (classic 45° diorama view).
const ISO_OFFSET: Vec3 = Vec3::new(40.0, 45.0, 40.0);
/// Ortho viewport height in world units when projection scale is 1.0.
const BASE_VIEWPORT_HEIGHT: f32 = 40.0;
/// Never zoom tighter than this viewport height (cars stay readable, world visible).
const MIN_VIEWPORT_HEIGHT: f32 = 24.0;
/// Never zoom wider than this (bounded arenas are designed to fit within it).
const MAX_VIEWPORT_HEIGHT: f32 = 80.0;
/// World-units margin added around the fitted car bounds.
const FIT_MARGIN: f32 = 10.0;
/// Smoothing rate for pan/zoom (higher = snappier).
const SMOOTHING: f32 = 4.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, fit_camera_to_cars);
    }
}

#[derive(Component)]
struct IsoCamera {
    focus: Vec3,
}

fn spawn_camera(mut commands: Commands) {
    let focus = Vec3::ZERO;
    commands.spawn((
        Name::new("Iso camera"),
        IsoCamera { focus },
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: BASE_VIEWPORT_HEIGHT,
            },
            scale: 1.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(focus + ISO_OFFSET).looking_at(focus, Vec3::Y),
    ));
}

fn fit_camera_to_cars(
    time: Res<Time>,
    cars: Query<&Transform, (With<Car>, Without<IsoCamera>)>,
    camera: Single<(&mut Transform, &mut Projection, &mut IsoCamera)>,
) {
    let (mut cam_transform, mut projection, mut iso) = camera.into_inner();

    let positions: Vec<Vec3> = cars.iter().map(|t| t.translation).collect();
    if positions.is_empty() {
        return;
    }

    // Camera-space basis (constant while the iso angle is fixed).
    let cam_right = *cam_transform.right();
    let cam_up = *cam_transform.up();

    // Fit the cars' extent along the camera's right/up axes.
    let center = positions.iter().sum::<Vec3>() / positions.len() as f32;
    let (mut extent_r, mut extent_u) = (0.0f32, 0.0f32);
    for p in &positions {
        let d = *p - center;
        extent_r = extent_r.max(d.dot(cam_right).abs());
        extent_u = extent_u.max(d.dot(cam_up).abs());
    }

    let aspect = 16.0 / 9.0; // close enough for fitting; exact aspect isn't critical
    let needed_height = ((extent_u * 2.0 + FIT_MARGIN).max((extent_r * 2.0 + FIT_MARGIN) / aspect))
        .clamp(MIN_VIEWPORT_HEIGHT, MAX_VIEWPORT_HEIGHT);

    // Smooth toward the target focus + zoom (frame-rate independent lerp).
    let t = 1.0 - (-SMOOTHING * time.delta_secs()).exp();
    iso.focus = iso.focus.lerp(center, t);
    cam_transform.translation = iso.focus + ISO_OFFSET;

    if let Projection::Orthographic(ref mut ortho) = *projection {
        let target_scale = needed_height / BASE_VIEWPORT_HEIGHT;
        ortho.scale += (target_scale - ortho.scale) * t;
    }
}
