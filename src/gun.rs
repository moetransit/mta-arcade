//! The railgun and the target-practice dream.
//!
//! Hitscan (design doc: what you see is what you hit — and later, rollback
//! makes that true online too). Kills are judged against the calibrated beat
//! clock, bemani style:
//!
//!   MARVELOUS   ±17ms of a beat      5 pts
//!   MARV·OFF    ±17ms of the offbeat 4 pts
//!   GREAT       ±50ms of either      2 pts
//!   off-rhythm  anything else        1 pt
//!
//! Solo scoring runs on the local calibrated clock; the multiplayer version
//! moves this judgment into the deterministic sim (design doc §5).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::vibe::BeatClock;
use mta_sim::{judge as sim_judge, BeatGrid, Judgment};

const COOLDOWN_S: f32 = 0.5; // solo practice cadence; instagib 1.2s comes with mp
const RANGE: f32 = 200.0;
const MAX_TARGETS: usize = 8;
const TARGET_LIFETIME_S: f32 = 8.0;

pub struct GunPlugin;

/// Netplay flips this off when a match starts (practice is lobby-only there).
#[derive(Resource)]
pub struct GunEnabled(pub bool);

fn gun_enabled(enabled: Res<GunEnabled>) -> bool {
    enabled.0
}

impl Plugin for GunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Score>()
            .init_resource::<Cooldown>()
            .init_resource::<TargetSpawner>()
            .insert_resource(GunEnabled(true))
            .add_systems(Startup, setup_gun_ui)
            .add_systems(
                Update,
                (
                    spawn_targets,
                    animate_targets,
                    expire_targets,
                    fire,
                    fade_beams,
                    fade_judgments,
                    update_score_text,
                )
                    .run_if(gun_enabled),
            );
    }
}

/// Remove all practice entities (targets, beams, judgment popups).
pub fn cleanup_practice(
    mut commands: Commands,
    targets: Query<Entity, With<Target>>,
    beams: Query<Entity, With<Beam>>,
    judgments: Query<Entity, With<JudgmentText>>,
) {
    for e in targets.iter().chain(beams.iter()).chain(judgments.iter()) {
        commands.entity(e).despawn();
    }
}

#[derive(Resource, Default)]
pub struct Score {
    pub points: u32,
    pub frags: u32,
}

#[derive(Resource, Default)]
struct Cooldown(f32);

#[derive(Resource, Default)]
struct TargetSpawner {
    next_beat_idx: usize,
    mesh: Handle<Mesh>,
    material: Handle<crate::PsxMaterial>,
}

#[derive(Component)]
pub struct Target {
    born: f32,
    seed: f32,
}

#[derive(Component)]
pub struct Beam {
    ttl: f32,
}

#[derive(Component)]
pub struct JudgmentText {
    ttl: f32,
}

#[derive(Component)]
struct ScoreText;

/// Judge a kill instant against the beat grid via the deterministic sim core.
fn judge(clock: &BeatClock) -> (&'static str, u32) {
    // NOTE: solo mode judges on the calibrated presentation clock; netplay
    // will call sim_judge with a sim-tick-derived time instead (doc §5).
    let grid = BeatGrid::new(clock.beat_times.clone());
    let j: Judgment = sim_judge(&grid, clock.effective_time());
    (j.label(), j.points())
}

fn setup_gun_ui(
    mut commands: Commands,
    mut spawner: ResMut<TargetSpawner>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::PsxMaterial>>,
) {
    // dream target: chunky little ico shard in miku-adjacent pink
    spawner.mesh = meshes.add(Sphere::new(0.55).mesh().ico(1).expect("ico"));
    spawner.material = materials.add(crate::psx(Color::srgb(0.84, 0.16, 0.46), 0.7, 0.0));

    // crosshair
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            margin: UiRect {
                left: Val::Px(-2.0),
                top: Val::Px(-2.0),
                ..default()
            },
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        GlobalZIndex(1),
    ));

    // score, top-left
    commands.spawn((
        Text::new("score 0"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgb(0.53, 0.81, 0.80)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(8.0),
            ..default()
        },
        GlobalZIndex(1),
        ScoreText,
    ));
}

/// A target materializes every 2nd beat (up to a cap), placed by a golden-angle
/// walk around the arena so the spread feels dreamlike but fair.
fn spawn_targets(
    mut commands: Commands,
    clock: Res<BeatClock>,
    mut spawner: ResMut<TargetSpawner>,
    time: Res<Time>,
    targets: Query<(), With<Target>>,
) {
    if !clock.playing {
        return;
    }
    let beat_idx = clock
        .beat_times
        .partition_point(|&b| b <= clock.effective_time());
    if beat_idx < spawner.next_beat_idx || targets.iter().count() >= MAX_TARGETS {
        // (also handles track loop wrap: reset the walk)
        if beat_idx + 4 < spawner.next_beat_idx {
            spawner.next_beat_idx = beat_idx;
        }
        return;
    }
    spawner.next_beat_idx = beat_idx + 2;

    let i = beat_idx as f32;
    let angle = i * 2.399_963; // golden angle
    let radius = 9.0 + (i * 1.618).rem_euclid(13.0);
    let height = 1.5 + (i * 0.77).rem_euclid(7.0);
    commands.spawn((
        Mesh3d(spawner.mesh.clone()),
        MeshMaterial3d(spawner.material.clone()),
        Transform::from_xyz(angle.cos() * radius, height, angle.sin() * radius),
        Collider::sphere(0.55),
        RigidBody::Static,
        Sensor,
        Target {
            born: time.elapsed_secs(),
            seed: i,
        },
    ));
}

fn animate_targets(time: Res<Time>, mut targets: Query<(&Target, &mut Transform)>) {
    for (target, mut tf) in &mut targets {
        let t = time.elapsed_secs() + target.seed;
        tf.translation.y += (t * 2.1).sin() * 0.004;
        tf.rotate_y(0.9 * time.delta_secs());
    }
}

fn expire_targets(mut commands: Commands, time: Res<Time>, targets: Query<(Entity, &Target)>) {
    for (entity, target) in &targets {
        if time.elapsed_secs() - target.born > TARGET_LIFETIME_S {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fire(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&bevy::window::CursorOptions>,
    mut cooldown: ResMut<Cooldown>,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    player: Query<Entity, With<crate::PlayerInput>>,
    spatial: SpatialQuery,
    targets: Query<(), With<Target>>,
    clock: Res<BeatClock>,
    mut score: ResMut<Score>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::PsxMaterial>>,
    mut was_locked: Local<bool>,
) {
    cooldown.0 -= time.delta_secs();
    // fire only if the cursor was already captured BEFORE this frame —
    // the grab click itself must not shoot, and same-frame ordering vs
    // capture_cursor is nondeterministic
    let locked_now = window.grab_mode != bevy::window::CursorGrabMode::None;
    let can_fire = *was_locked;
    *was_locked = locked_now;
    if !buttons.just_pressed(MouseButton::Left) || !can_fire || cooldown.0 > 0.0 {
        return;
    }
    let Ok(cam) = camera.single() else {
        return;
    };
    cooldown.0 = COOLDOWN_S;

    let origin = cam.translation();
    let dir = cam.forward();
    // exclude the shooter: the ray starts inside the player's own collider,
    // which otherwise eats every shot at distance zero
    let filter = SpatialQueryFilter::default().with_excluded_entities(player.iter());
    let hit = spatial.cast_ray(origin, dir, RANGE, true, &filter);

    let (end, hit_target) = match hit {
        Some(h) => (
            origin + *dir * h.distance,
            targets.contains(h.entity).then_some(h.entity),
        ),
        None => (origin + *dir * RANGE, None),
    };

    // beam vfx: a thin bright shard from just under the camera to the endpoint
    let start = origin + *dir * 0.6 + Vec3::new(0.0, -0.25, 0.0);
    let length = (end - start).length();
    let mid = (start + end) / 2.0;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.035, 0.035, length))),
        MeshMaterial3d(materials.add(crate::PsxMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(1.0, 1.0, 1.0),
                emissive: LinearRgba::rgb(2.0, 6.0, 6.0),
                unlit: true,
                ..default()
            },
            extension: default(),
        })),
        Transform::from_translation(mid).looking_at(end, Vec3::Y),
        Beam { ttl: 0.12 },
    ));

    if let Some(entity) = hit_target {
        commands.entity(entity).despawn();
        let (label, points) = judge(&clock);
        score.frags += 1;
        score.points += points;
        commands.spawn((
            Text::new(format!("{label} +{points}")),
            TextFont {
                font_size: if points >= 4 { 22.0 } else { 15.0 },
                ..default()
            },
            TextColor(if points >= 4 {
                Color::srgb(1.0, 0.9, 0.4)
            } else if points == 2 {
                Color::srgb(0.53, 0.81, 0.80)
            } else {
                Color::srgba(0.7, 0.7, 0.7, 0.8)
            }),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(56.0),
                margin: UiRect {
                    left: Val::Px(-60.0),
                    ..default()
                },
                ..default()
            },
            GlobalZIndex(1),
            JudgmentText { ttl: 0.8 },
        ));
    }
}

fn fade_beams(mut commands: Commands, time: Res<Time>, mut beams: Query<(Entity, &mut Beam)>) {
    for (entity, mut beam) in &mut beams {
        beam.ttl -= time.delta_secs();
        if beam.ttl <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn fade_judgments(
    mut commands: Commands,
    time: Res<Time>,
    mut texts: Query<(Entity, &mut JudgmentText, &mut Node)>,
) {
    for (entity, mut jt, mut node) in &mut texts {
        jt.ttl -= time.delta_secs();
        // drift upward as it fades
        if let Val::Percent(top) = node.top {
            node.top = Val::Percent(top - 6.0 * time.delta_secs());
        }
        if jt.ttl <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn update_score_text(score: Res<Score>, mut texts: Query<&mut Text, With<ScoreText>>) {
    if !score.is_changed() {
        return;
    }
    for mut text in &mut texts {
        text.0 = format!("score {}  ({} frags)", score.points, score.frags);
    }
}
