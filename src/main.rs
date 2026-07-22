use bevy::prelude::*;

/// Phase 0 scaffold: a low-poly dream shard spinning in teal fog.
/// Proves the native + wasm render pipeline before any gameplay exists.
fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.004, 0.055, 0.06)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "moe transit arcade".into(),
                canvas: Some("#mta-canvas".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, spin)
        .run();
}

#[derive(Component)]
struct Spinner;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.2, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        DistanceFog {
            color: Color::srgb(0.004, 0.055, 0.06),
            falloff: FogFalloff::Linear {
                start: 3.0,
                end: 14.0,
            },
            ..default()
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));

    // the shard: ico(1) keeps it chunky on purpose
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().ico(1).expect("ico subdivision"))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.075, 0.478, 0.498),
            perceptual_roughness: 0.9,
            metallic: 0.1,
            ..default()
        })),
        Spinner,
    ));

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.016, 0.11, 0.115),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -1.4, 0.0),
    ));
}

fn spin(time: Res<Time>, mut query: Query<&mut Transform, With<Spinner>>) {
    for mut transform in &mut query {
        transform.rotate_y(0.6 * time.delta_secs());
        transform.rotate_x(0.25 * time.delta_secs());
    }
}
