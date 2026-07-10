//! Sound effects — see assets/sounds/LICENSE.txt for sources and attributions.
//!
//! Game systems send [`PlaySfx`] messages; this plugin spawns one-shot audio
//! entities. SFX play non-spatially — the iso camera sits ~60 world-units from
//! the arena, so Bevy's simple spatial panning attenuates gameplay sounds to
//! near-silence while UI clicks (already non-spatial) remain audible.

use bevy::audio::{AudioPlayer, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;

#[derive(Resource)]
pub struct SfxAssets {
    pub rocket_fire: Handle<AudioSource>,
    pub grenade_launch: Handle<AudioSource>,
    pub explosion: Handle<AudioSource>,
    pub explosion_big: Handle<AudioSource>,
    pub hit: Handle<AudioSource>,
    pub wreck: Handle<AudioSource>,
    pub pickup: Handle<AudioSource>,
    pub ui_select: Handle<AudioSource>,
    pub ui_click: Handle<AudioSource>,
    pub ui_pluck: Handle<AudioSource>,
    pub round_win: Handle<AudioSource>,
}

#[derive(Clone, Copy, Debug)]
pub enum SfxKind {
    /// Shotgun blast: the explosion sample pitched up into a single "blam".
    Shotgun,
    /// Car slamming into the world (walls/buildings), speed-gated by sender.
    Crunch,
    RocketFire,
    GrenadeLaunch,
    Explosion,
    ExplosionBig,
    Hit,
    Wreck,
    Pickup,
    UiSelect,
    UiClick,
    UiPluck,
    RoundWin,
}

/// Request a one-shot sound. `position` is retained for callers / future panning.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlaySfx {
    pub kind: SfxKind,
    #[allow(dead_code)]
    pub position: Option<Vec3>,
}

#[derive(Resource, Default)]
struct PitchJitter(usize);

pub struct AudioSfxPlugin;

impl Plugin for AudioSfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PitchJitter>()
            .add_message::<PlaySfx>()
            .add_systems(Startup, load_sfx_assets)
            // PostUpdate: run after FixedUpdate weapon fire and Update hit/explosion systems.
            .add_systems(PostUpdate, play_sfx);
    }
}

fn load_sfx_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let load = |path: &str| asset_server.load(format!("sounds/{path}"));
    commands.insert_resource(SfxAssets {
        rocket_fire: load("rocket_fire.ogg"),
        grenade_launch: load("grenade_launch.ogg"),
        explosion: load("explosion.ogg"),
        explosion_big: load("explosion_big.ogg"),
        hit: load("hit.ogg"),
        wreck: load("car_explosion.ogg"),
        pickup: load("pickup.ogg"),
        ui_select: load("ui_select.ogg"),
        ui_click: load("ui_click.ogg"),
        ui_pluck: load("ui_pluck.ogg"),
        round_win: load("round_win.ogg"),
    });
}

fn play_sfx(
    mut commands: Commands,
    assets: Res<SfxAssets>,
    mut events: MessageReader<PlaySfx>,
    mut jitter: ResMut<PitchJitter>,
) {
    for event in events.read() {
        let (handle, volume, speed) = match event.kind {
            SfxKind::Shotgun => {
                // One clean "blam": the explosion sample pitched up a touch.
                // No layering — stacked samples read as multiple shots.
                let index = jitter.0 % 3;
                jitter.0 = jitter.0.wrapping_add(1);
                (
                    assets.explosion.clone(),
                    Volume::Linear(0.8),
                    1.3 + index as f32 * 0.05,
                )
            }
            SfxKind::Crunch => {
                let index = jitter.0 % 3;
                jitter.0 = jitter.0.wrapping_add(1);
                // Wreck sound pitched up: short metallic crunch.
                (
                    assets.wreck.clone(),
                    Volume::Linear(0.5),
                    1.45 + index as f32 * 0.1,
                )
            }
            SfxKind::RocketFire => (assets.rocket_fire.clone(), Volume::Linear(0.8), 1.0),
            SfxKind::GrenadeLaunch => (assets.grenade_launch.clone(), Volume::Linear(0.65), 0.95),
            SfxKind::Explosion => (assets.explosion.clone(), Volume::Linear(0.85), 1.0),
            SfxKind::ExplosionBig => (assets.explosion_big.clone(), Volume::Linear(1.0), 0.9),
            SfxKind::Hit => (assets.hit.clone(), Volume::Linear(0.5), 1.0),
            SfxKind::Wreck => (assets.wreck.clone(), Volume::Linear(0.82), 0.95),
            SfxKind::Pickup => (assets.pickup.clone(), Volume::Linear(0.75), 1.0),
            SfxKind::UiSelect => (assets.ui_select.clone(), Volume::Linear(0.55), 1.0),
            SfxKind::UiClick => (assets.ui_click.clone(), Volume::Linear(0.45), 1.0),
            SfxKind::UiPluck => (assets.ui_pluck.clone(), Volume::Linear(0.5), 1.0),
            SfxKind::RoundWin => (assets.round_win.clone(), Volume::Linear(0.8), 1.0),
        };

        spawn_one(&mut commands, handle, volume.to_linear(), speed);
    }
}

/// Spawn a single one-shot, self-despawning audio player.
fn spawn_one(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((
        AudioPlayer::new(handle),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            speed,
            spatial: false,
            ..PlaybackSettings::ONCE
        },
    ));
}