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
        ))
        .add_input_context::<PlayerInput>()
        .add_systems(
            Startup,
            (setup_render_target, setup_arena, setup_player).chain(),
        )
        .add_systems(
            Update,
            (
                capture_cursor.run_if(input_just_pressed(MouseButton::Left)),
                release_cursor.run_if(input_just_pressed(KeyCode::Escape)),
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
type PsxMaterial = ExtendedMaterial<StandardMaterial, PsxExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
struct PsxExtension {}

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
    ));
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(handle),
    ));
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

fn psx(color: Color, roughness: f32, metallic: f32) -> PsxMaterial {
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
}

fn release_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}
