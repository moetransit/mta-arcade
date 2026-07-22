//! Phase 4a: p2p netplay over matchbox + GGRS rollback.
//!
//! Opt-in via URL hash: `#net=ws://127.0.0.1:3536/mta?next=2` — the game
//! then runs the deterministic sim (crates/sim) for BOTH players instead of
//! the solo bevy_ahoy controller. Dev loop: run `matchbox_server` locally,
//! open two tabs. No public signaling infra yet (roadmap option 3).

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::{
    camera::RenderTarget,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use bevy_ggrs::{prelude::*, LocalInputs, LocalPlayers};
use bevy_matchbox::prelude::*;
use mta_sim::{
    arena::{graybox, Aabb, SPAWNS},
    movement::{BTN_BACK, BTN_FIRE, BTN_FWD, BTN_JUMP, BTN_LEFT, BTN_RIGHT, DT, EYE_HEIGHT},
    NetInput, PlayerState,
};

type Config = GgrsConfig<NetInput, PeerId>;

/// Netplay is requested via `#net=<signaling-room-url>` in the page URL.
#[allow(clippy::needless_return)] // early return is load-bearing across the cfg split
pub fn requested() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let hash = web_sys::window()?.location().hash().ok()?;
        return hash.strip_prefix("#net=").map(|s| s.to_string());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::args().find_map(|a| a.strip_prefix("--net=").map(|s| s.to_string()))
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, States)]
enum NetState {
    #[default]
    Lobby,
    InGame,
}

#[derive(Resource)]
struct RoomUrl(String);

#[derive(Resource)]
struct NetArena(Vec<Aabb>);

/// Look angles accumulated from the mouse each render frame; quantized into
/// the tick input stream by `read_local_inputs`.
#[derive(Resource, Default)]
struct Look {
    yaw: f32,
    pitch: f32,
}

#[derive(Component, Clone)]
struct Sim(PlayerState);

#[derive(Component)]
struct NetPlayer {
    handle: usize,
}

#[derive(Component)]
struct LobbyText;

/// Sim-driven local player for warming up while the lobby waits.
#[derive(Component)]
struct PracticePlayer;

pub fn run(room_url: String) -> AppExit {
    App::new()
        .insert_resource(ClearColor(crate::DEEP_TEAL))
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "moe transit arcade :: netplay".into(),
                    canvas: Some("#mta-canvas".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            }),
            avian3d::prelude::PhysicsPlugins::default(),
            MaterialPlugin::<crate::PsxMaterial>::default(),
            crate::vibe::VibePlugin,
            crate::gun::GunPlugin,
            GgrsPlugin::<Config>::default(),
        ))
        .rollback_component_with_clone::<Sim>()
        .add_systems(ReadInputs, read_local_inputs)
        .insert_resource(RoomUrl(room_url))
        .insert_resource(NetArena(graybox()))
        .init_resource::<Look>()
        .init_state::<NetState>()
        .add_systems(
            Startup,
            (crate::setup_render_target, crate::setup_arena, setup_net).chain(),
        )
        .add_systems(
            Update,
            (lobby, practice_move, practice_camera).run_if(in_state(NetState::Lobby)),
        )
        .add_systems(
            OnEnter(NetState::InGame),
            (end_practice, crate::gun::cleanup_practice, spawn_players),
        )
        .add_systems(GgrsSchedule, advance_sim)
        .add_systems(
            Update,
            (
                capture_cursor.run_if(bevy::input::common_conditions::input_just_pressed(
                    MouseButton::Left,
                )),
                release_cursor.run_if(bevy::input::common_conditions::input_just_pressed(
                    KeyCode::Escape,
                )),
                mouse_look,
                (sync_bodies, camera_follow).run_if(in_state(NetState::InGame)),
            ),
        )
        .run()
}

fn setup_net(mut commands: Commands, url: Res<RoomUrl>, target: Res<crate::PsxTarget>) {
    // psx camera (netplay drives it manually; no ahoy)
    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.0.clone().into()),
        Msaa::Off,
        Transform::from_xyz(0.0, 2.0, 8.0),
        DistanceFog {
            color: crate::DEEP_TEAL,
            falloff: FogFalloff::Linear {
                start: 12.0,
                end: 60.0,
            },
            ..default()
        },
    ));

    commands.spawn((
        Text::new("dialing the void..."),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.53, 0.81, 0.80)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(40.0),
            margin: UiRect {
                left: Val::Px(-90.0),
                ..default()
            },
            ..default()
        },
        GlobalZIndex(1),
        LobbyText,
    ));

    info!("connecting to matchbox: {}", url.0);
    commands.insert_resource(MatchboxSocket::new_unreliable(url.0.clone()));

    // warm-up: a sim-driven practice player + the target dream (gun plugin)
    let (pos, yaw) = SPAWNS[0];
    commands.spawn((PracticePlayer, Sim(PlayerState::spawn(pos, yaw))));
}

/// Build a NetInput from live devices — shared by practice and netplay input.
fn build_input(
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    look: &Look,
) -> NetInput {
    let mut b = 0u8;
    if keys.pressed(KeyCode::KeyW) {
        b |= BTN_FWD;
    }
    if keys.pressed(KeyCode::KeyS) {
        b |= BTN_BACK;
    }
    if keys.pressed(KeyCode::KeyA) {
        b |= BTN_LEFT;
    }
    if keys.pressed(KeyCode::KeyD) {
        b |= BTN_RIGHT;
    }
    if keys.pressed(KeyCode::Space) {
        b |= BTN_JUMP;
    }
    if buttons.pressed(MouseButton::Left) {
        b |= BTN_FIRE;
    }
    NetInput {
        buttons: b,
        yaw_mrad: (look.yaw * 1000.0) as i16,
        pitch_mrad: (look.pitch * 1000.0) as i16,
    }
}

/// Advance the practice player on the sim at a fixed 60hz, from live input.
fn practice_move(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    look: Res<Look>,
    arena: Res<NetArena>,
    mut acc: Local<f32>,
    mut player: Query<&mut Sim, With<PracticePlayer>>,
) {
    let Ok(mut sim) = player.single_mut() else {
        return;
    };
    let input = build_input(&keys, &buttons, &look);
    *acc += time.delta_secs().min(0.25);
    while *acc >= DT {
        mta_sim::step(&mut sim.0, &input, &arena.0);
        *acc -= DT;
    }
}

fn practice_camera(
    look: Res<Look>,
    player: Query<&Sim, With<PracticePlayer>>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(sim) = player.single() else {
        return;
    };
    let s = &sim.0;
    for mut tf in &mut camera {
        tf.translation = Vec3::new(s.pos[0], s.pos[1] + EYE_HEIGHT, s.pos[2]);
        tf.rotation = Quat::from_rotation_y(look.yaw) * Quat::from_rotation_x(look.pitch);
    }
}

fn end_practice(
    mut commands: Commands,
    mut enabled: ResMut<crate::gun::GunEnabled>,
    practice: Query<Entity, With<PracticePlayer>>,
) {
    enabled.0 = false;
    for e in &practice {
        commands.entity(e).despawn();
    }
}

fn lobby(
    mut socket: ResMut<MatchboxSocket>,
    mut commands: Commands,
    mut state: ResMut<NextState<NetState>>,
    mut text: Query<&mut Text, With<LobbyText>>,
) {
    let Ok(peer_changes) = socket.try_update_peers() else {
        warn!("signaling socket dropped");
        return;
    };
    for (peer, s) in peer_changes {
        info!("peer {peer}: {s:?}");
    }

    let connected = socket.connected_peers().count();
    for mut t in &mut text {
        t.0 = format!("waiting for opponent... ({}/2 in room)", connected + 1);
    }
    if connected < 1 {
        return;
    }

    let players = socket.players();
    let mut builder = SessionBuilder::<Config>::new()
        .with_num_players(2)
        .with_max_prediction_window(8)
        .with_input_delay(2);
    for (i, player) in players.into_iter().enumerate() {
        builder = builder.add_player(player, i).expect("add player");
    }
    let channel = socket.take_channel(0).expect("channel");
    let session = builder.start_p2p_session(channel).expect("session");
    commands.insert_resource(Session::P2P(session));
    state.set(NetState::InGame);
}

fn spawn_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::PsxMaterial>>,
    text: Query<Entity, With<LobbyText>>,
) {
    for e in &text {
        commands.entity(e).despawn();
    }
    let body = meshes.add(Cuboid::new(0.8, 1.8, 0.8));
    let colors = [Color::srgb(0.53, 0.81, 0.80), Color::srgb(0.84, 0.16, 0.46)];
    for (handle, (pos, yaw)) in SPAWNS.iter().enumerate() {
        commands
            .spawn((
                Mesh3d(body.clone()),
                MeshMaterial3d(materials.add(crate::psx(colors[handle], 0.8, 0.1))),
                Transform::from_translation(Vec3::from_array(*pos)),
                NetPlayer { handle },
                Sim(PlayerState::spawn(*pos, *yaw)),
            ))
            .add_rollback();
    }
    info!("session started: 2 players in the dream");
}

fn read_local_inputs(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    look: Res<Look>,
    local_players: Res<LocalPlayers>,
) {
    let mut map = bevy::platform::collections::HashMap::new();
    for handle in &local_players.0 {
        map.insert(*handle, build_input(&keys, &buttons, &look));
    }
    commands.insert_resource(LocalInputs::<Config>(map));
}

fn advance_sim(
    mut players: Query<(&NetPlayer, &mut Sim)>,
    inputs: Res<PlayerInputs<Config>>,
    arena: Res<NetArena>,
) {
    let mut sorted: Vec<_> = players.iter_mut().collect();
    sorted.sort_by_key(|(p, _)| p.handle);
    for (player, sim) in sorted {
        let (input, _status) = inputs[player.handle];
        mta_sim::step(&mut sim.into_inner().0, &input, &arena.0);
    }
}

fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions>,
    mut look: ResMut<Look>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }
    look.yaw -= motion.delta.x * 0.002;
    look.pitch = (look.pitch - motion.delta.y * 0.002).clamp(-1.5, 1.5);
    // wrap yaw to ±pi so the i16 milliradian encoding never saturates
    use std::f32::consts::PI;
    if look.yaw > PI {
        look.yaw -= 2.0 * PI;
    }
    if look.yaw < -PI {
        look.yaw += 2.0 * PI;
    }
}

fn sync_bodies(
    local_players: Res<LocalPlayers>,
    mut players: Query<(&NetPlayer, &Sim, &mut Transform, &mut Visibility)>,
) {
    for (player, sim, mut tf, mut vis) in &mut players {
        let s = &sim.0;
        tf.translation = Vec3::new(s.pos[0], s.pos[1] + 0.9, s.pos[2]);
        tf.rotation = Quat::from_rotation_y(s.yaw);
        // don't render your own body from inside it
        *vis = if local_players.0.contains(&player.handle) {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}

fn camera_follow(
    local_players: Res<LocalPlayers>,
    look: Res<Look>,
    players: Query<(&NetPlayer, &Sim)>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let Some((_, sim)) = players
        .iter()
        .find(|(p, _)| local_players.0.contains(&p.handle))
    else {
        return;
    };
    let s = &sim.0;
    for mut tf in &mut camera {
        tf.translation = Vec3::new(s.pos[0], s.pos[1] + EYE_HEIGHT, s.pos[2]);
        tf.rotation = Quat::from_rotation_y(look.yaw) * Quat::from_rotation_x(look.pitch);
    }
}

fn capture_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
    crate::vibe::ensure_audio_started();
}

fn release_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}
