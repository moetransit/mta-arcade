use avian3d::prelude::*;
use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    input::common_conditions::input_just_pressed,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, TextureFormat},
    shader::ShaderRef,
    window::{CursorGrabMode, CursorOptions},
};
use bevy_ahoy::prelude::*;
use bevy_enhanced_input::prelude::*;

mod gun;
mod vibe;

/// Phase 1: quake movement (bevy_ahoy) in a graybox dream arena, rendered PS1-style:
/// 426x240 internal target, nearest-upscaled, vertex-snapped geometry.
/// Click to grab the mouse, Esc to release. WASD + Space, air-strafe welcome.
fn main() -> AppExit {
    App::new()
        .insert_resource(ClearColor(DEEP_TEAL))
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "moe transit arcade".into(),
                    canvas: Some("#mta-canvas".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            }),
            PhysicsPlugins::default(),
            EnhancedInputPlugin,
            AhoyPlugins::default(),
            MaterialPlugin::<PsxMaterial>::default(),
            vibe::VibePlugin,
            gun::GunPlugin,
        ))
        .add_input_context::<PlayerInput>()
        .add_systems(
            Startup,
            (
                setup_render_target,
                setup_arena,
                setup_player,
                setup_now_playing,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                capture_cursor.run_if(input_just_pressed(MouseButton::Left)),
                release_cursor.run_if(input_just_pressed(KeyCode::Escape)),
                vibe_visuals,
                show_now_playing,
                update_iidx,
                tap_calibration.run_if(input_just_pressed(KeyCode::KeyT)),
            ),
        )
        .run()
}

const DEEP_TEAL: Color = Color::srgb(0.004, 0.055, 0.06);
const ARENA_TEAL: Color = Color::srgb(0.075, 0.478, 0.498);
const FLOOR_TEAL: Color = Color::srgb(0.016, 0.11, 0.115);

/// Internal PS1 framebuffer resolution (16:9-ish 240p).
const INTERNAL_WIDTH: u32 = 426;
const INTERNAL_HEIGHT: u32 = 240;

/// StandardMaterial + PS1 vertex snapping.
pub type PsxMaterial = ExtendedMaterial<StandardMaterial, PsxExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct PsxExtension {
    /// x: bass, y: lowmid, z: highmid, w: treble — fed by the vibe layer
    #[uniform(100)]
    pub bands: Vec4,
}

impl MaterialExtension for PsxExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/psx.wgsl".into()
    }
}

#[derive(Resource, Clone)]
struct PsxTarget(Handle<Image>);

#[derive(Component, Default)]
struct PlayerInput;

fn setup_render_target(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // no view-format reinterpretation: WebGL2 lacks VIEW_FORMATS support
    let mut image = Image::new_target_texture(
        INTERNAL_WIDTH,
        INTERNAL_HEIGHT,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    commands.insert_resource(PsxTarget(handle.clone()));

    // fullscreen chunky upscale of the internal framebuffer
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
        IsDefaultUiCamera,
    ));
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(handle),
        GlobalZIndex(0),
    ));
}

#[derive(Component)]
struct NowPlayingLabel;

/// The IIDX clock: a lane where notes cross the judgment line exactly on
/// each analyzed beat — an eyeball test of beat-grid accuracy vs your ears.
#[derive(Component)]
struct IidxNote(usize);

#[derive(Component)]
struct IidxLine;

#[derive(Component)]
struct CalLabel;

const IIDX_LANE_W: f32 = 200.0;
const IIDX_LINE_X: f32 = 32.0;
const IIDX_PX_PER_SEC: f32 = 110.0;
const IIDX_NOTE_POOL: usize = 8;

fn setup_now_playing(mut commands: Commands, now: Res<vibe::NowPlaying>) {
    commands.spawn((
        Text::new(format!("♪ {} — {}", now.artist, now.title)),
        TextFont {
            font_size: 13.0,
            ..default()
        },
        TextColor(Color::srgb(0.53, 0.81, 0.80)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            bottom: Val::Px(8.0),
            ..default()
        },
        Visibility::Hidden,
        GlobalZIndex(1),
        NowPlayingLabel,
    ));

    // iidx clock lane, bottom-right
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                bottom: Val::Px(8.0),
                width: Val::Px(IIDX_LANE_W),
                height: Val::Px(30.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.05, 0.05, 0.65)),
            GlobalZIndex(1),
        ))
        .with_children(|lane| {
            lane.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(IIDX_LINE_X),
                    top: Val::Px(2.0),
                    width: Val::Px(2.0),
                    height: Val::Px(26.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(1.0, 1.0, 1.0)),
                IidxLine,
            ));
            lane.spawn((
                Text::new("T = tap to beat"),
                TextFont {
                    font_size: 9.0,
                    ..default()
                },
                TextColor(Color::srgba(0.53, 0.81, 0.80, 0.7)),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(4.0),
                    top: Val::Px(2.0),
                    ..default()
                },
                CalLabel,
            ));
            for i in 0..IIDX_NOTE_POOL {
                lane.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(-10.0),
                        top: Val::Px(5.0),
                        width: Val::Px(4.0),
                        height: Val::Px(20.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.53, 0.81, 0.80)),
                    Visibility::Hidden,
                    IidxNote(i),
                ));
            }
        });
}

/// Tap calibration: press T on the beat; the median tap offset becomes a
/// per-device correction applied to the whole beat clock (and persisted).
fn tap_calibration(
    mut clock: ResMut<vibe::BeatClock>,
    mut label: Query<&mut Text, With<CalLabel>>,
) {
    if !clock.playing {
        return;
    }
    let taps = clock.taps.len() + 1;
    if let Some(median) = clock.record_tap() {
        vibe::save_calibration(median);
        for mut text in &mut label {
            text.0 = format!("cal {:+.0}ms ({} taps)", median * 1000.0, taps);
        }
    } else {
        for mut text in &mut label {
            text.0 = format!("tap {taps}/4...");
        }
    }
}

/// Scroll notes right-to-left so each crosses the line at its exact beat time.
fn update_iidx(
    clock: Res<vibe::BeatClock>,
    mut notes: Query<(&IidxNote, &mut Node, &mut Visibility), Without<IidxLine>>,
    mut line: Query<&mut BackgroundColor, With<IidxLine>>,
) {
    if !clock.playing {
        return;
    }
    let now = clock.effective_time();
    let lookahead = ((IIDX_LANE_W - IIDX_LINE_X) / IIDX_PX_PER_SEC) as f64;
    let lookbehind = (IIDX_LINE_X / IIDX_PX_PER_SEC) as f64;

    let start = clock.beat_times.partition_point(|&b| b < now - lookbehind);
    let upcoming: Vec<f64> = clock
        .beat_times
        .iter()
        .copied()
        .skip(start)
        .take_while(|&b| b < now + lookahead)
        .collect();

    for (note, mut node, mut vis) in &mut notes {
        if let Some(&beat) = upcoming.get(note.0) {
            node.left = Val::Px(IIDX_LINE_X + ((beat - now) as f32) * IIDX_PX_PER_SEC - 2.0);
            *vis = Visibility::Visible;
        } else {
            *vis = Visibility::Hidden;
        }
    }

    // line flashes white on the beat, decays to teal
    let pulse = (1.0 - clock.beat_phase()).powi(3);
    for mut bg in &mut line {
        bg.0 = Color::srgb(
            0.53 + 0.47 * pulse,
            0.81 + 0.19 * pulse,
            0.80 + 0.20 * pulse,
        );
    }
}

/// Reveal the now-playing tag once audio actually starts.
fn show_now_playing(
    clock: Res<vibe::BeatClock>,
    mut label: Query<&mut Visibility, With<NowPlayingLabel>>,
) {
    if clock.playing {
        for mut vis in &mut label {
            *vis = Visibility::Visible;
        }
    }
}

fn setup_player(mut commands: Commands, target: Res<PsxTarget>) {
    let player = commands
        .spawn((
            CharacterController {
                // tuning passes 1-2 (playtested): fast base, dashdance-crisp reversals
                // (high accel converges onto the new wish dir fast; high friction
                //  kills stale velocity fast; buffered jumps keep bhop alive)
                speed: 17.5,
                acceleration_hz: 16.0,
                friction_hz: 18.0,
                ..default()
            },
            // cylinder over capsule: parry likes it better (ahoy readme)
            Collider::cylinder(0.4, 1.8),
            Transform::from_xyz(0.0, 3.0, 8.0),
            PlayerInput,
            actions!(PlayerInput[
                (
                    Action::<Movement>::new(),
                    DeadZone::default(),
                    Bindings::spawn((Cardinal::wasd_keys(), Axial::left_stick()))
                ),
                (
                    Action::<Jump>::new(),
                    bindings![KeyCode::Space, GamepadButton::South],
                ),
                (
                    Action::<Crouch>::new(),
                    bindings![KeyCode::ControlLeft, GamepadButton::LeftTrigger2],
                ),
                (
                    Action::<RotateCamera>::new(),
                    Bindings::spawn((
                        Spawn((Binding::mouse_motion(), Scale::splat(0.07))),
                        Axial::right_stick().with((Scale::splat(4.0), DeadZone::default())),
                    ))
                ),
            ]),
        ))
        .id();

    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.0.clone().into()),
        Msaa::Off,
        CharacterControllerCameraOf::new(player),
        DistanceFog {
            color: DEEP_TEAL,
            falloff: FogFalloff::Linear {
                start: 12.0,
                end: 60.0,
            },
            ..default()
        },
    ));
}

fn setup_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PsxMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));

    let floor_mat = materials.add(psx(FLOOR_TEAL, 1.0, 0.0));
    let block_mat = materials.add(psx(ARENA_TEAL, 0.9, 0.1));

    let mut spawn_box = |size: Vec3, pos: Vec3, rot: Quat, mat: &Handle<PsxMaterial>| {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos).with_rotation(rot),
            RigidBody::Static,
            Collider::cuboid(size.x, size.y, size.z),
        ));
    };

    // floor
    spawn_box(
        Vec3::new(60.0, 1.0, 60.0),
        Vec3::new(0.0, -0.5, 0.0),
        Quat::IDENTITY,
        &floor_mat,
    );

    // graybox props: platforms, stairs, and surf ramps
    spawn_box(
        Vec3::new(6.0, 1.0, 6.0),
        Vec3::new(-8.0, 1.5, -6.0),
        Quat::IDENTITY,
        &block_mat,
    );
    spawn_box(
        Vec3::new(6.0, 1.0, 6.0),
        Vec3::new(-14.0, 3.0, -12.0),
        Quat::IDENTITY,
        &block_mat,
    );
    // stair steps
    for i in 0..6 {
        spawn_box(
            Vec3::new(3.0, 0.3, 1.0),
            Vec3::new(8.0, 0.15 + i as f32 * 0.3, -4.0 - i as f32),
            Quat::IDENTITY,
            &block_mat,
        );
    }
    // surf ramp (steep enough to slide, shallow enough to ride)
    spawn_box(
        Vec3::new(1.0, 14.0, 24.0),
        Vec3::new(16.0, 3.0, 8.0),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_3),
        &block_mat,
    );
    // a few floating dream shards for orientation
    for (x, y, z) in [(-4.0, 6.0, 4.0), (5.0, 9.0, -10.0), (-12.0, 12.0, 10.0)] {
        spawn_box(
            Vec3::splat(1.4),
            Vec3::new(x, y, z),
            Quat::from_euler(EulerRot::XYZ, 0.7, 0.4, 0.2),
            &block_mat,
        );
    }
    // perimeter walls
    for (pos, size) in [
        (Vec3::new(0.0, 6.0, -30.0), Vec3::new(60.0, 12.0, 1.0)),
        (Vec3::new(0.0, 6.0, 30.0), Vec3::new(60.0, 12.0, 1.0)),
        (Vec3::new(-30.0, 6.0, 0.0), Vec3::new(1.0, 12.0, 60.0)),
        (Vec3::new(30.0, 6.0, 0.0), Vec3::new(1.0, 12.0, 60.0)),
    ] {
        spawn_box(size, pos, Quat::IDENTITY, &floor_mat);
    }
}

pub fn psx(color: Color, roughness: f32, metallic: f32) -> PsxMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            metallic,
            ..default()
        },
        extension: PsxExtension::default(),
    }
}

fn capture_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
    // browsers only allow audio to start inside a user gesture
    vibe::ensure_audio_started();
}

/// Cosmetic audio reactivity: materials breathe, fog inhales, light hits on beat.
fn vibe_visuals(
    bands: Res<vibe::AudioBands>,
    clock: Res<vibe::BeatClock>,
    mut materials: ResMut<Assets<PsxMaterial>>,
    mut fogs: Query<&mut DistanceFog>,
    mut lights: Query<&mut DirectionalLight>,
) {
    let b = Vec4::new(bands.bass, bands.lowmid, bands.highmid, bands.treble);
    for (_, mat) in materials.iter_mut() {
        mat.extension.bands = b;
    }
    for mut fog in &mut fogs {
        if let FogFalloff::Linear { start, end } = &mut fog.falloff {
            *start = 12.0 - bands.bass * 7.0;
            *end = 60.0 - bands.bass * 15.0;
        }
    }
    let pulse = if clock.playing {
        (1.0 - clock.beat_phase()).powi(3)
    } else {
        0.0
    };
    for mut light in &mut lights {
        light.illuminance = 6_000.0 * (1.0 + 0.6 * pulse + 0.4 * bands.bass);
    }
}

fn release_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}
