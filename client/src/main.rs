use bevy::asset::RenderAssetUsages;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::window::PrimaryWindow;
use bevy_renet::client_connected;
use bevy_renet::netcode::{
    ClientAuthentication, NetcodeClientPlugin, NetcodeClientTransport, NetcodeErrorEvent,
};
use bevy_renet::renet::{ConnectionConfig, DefaultChannel};
use bevy_renet::{RenetClient, RenetClientPlugin};
use shared::{
    chunk_index, ChunkKey, ClientMessage, DiscoveryMessage, InputFlags, PlayerSnapshot, ServerMessage,
    ServerSummary, CHUNK_SIZE, DEFAULT_DISCOVERY_PORT, DEFAULT_GAME_PORT, GRAVITY, PLAYER_HEIGHT,
    PLAYER_WIDTH, PROTOCOL_ID,
};
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum AppScreen {
    #[default]
    MainMenu,
    JoinByIp,
    ServerBrowser,
    Connecting,
    InGame,
    Disconnected,
}

#[derive(Resource, Default)]
struct MenuState {
    player_name: String,
    ip_address: String,
    focused: FocusedField,
    disconnect_reason: String,
    reconnect_target: Option<SocketAddr>,
}

#[derive(Resource, Default)]
struct ConnectionState {
    current_target: Option<SocketAddr>,
    join_sent: bool,
    local_client_id: Option<u64>,
    connected_server_name: String,
    motd: String,
}

#[derive(Resource, Default)]
struct UiState {
    pause_open: bool,
    settings_open: bool,
    player_list_open: bool,
    chat_open: bool,
    tick: u32,
}

#[derive(Resource, Default)]
struct ChatState {
    lines: VecDeque<String>,
    current_input: String,
}

#[derive(Resource, Default)]
struct WorldCache {
    chunks: HashMap<ChunkKey, Entity>,
    players: HashMap<u64, Entity>,
}

#[derive(Resource, Default)]
struct ChunkDataCache(HashMap<ChunkKey, Vec<u8>>);

#[derive(Resource)]
struct DiscoveryInbox(Mutex<Receiver<DiscoveredServer>>);

#[derive(Clone)]
struct DiscoveredServer {
    addr: SocketAddr,
    summary: ServerSummary,
}

#[derive(Resource, Default)]
struct BrowserState {
    servers: Vec<DiscoveredServer>,
}

#[derive(Resource, Default)]
struct HudEntities {
    fps: Option<Entity>,
    status: Option<Entity>,
    chat: Option<Entity>,
    player_list: Option<Entity>,
    overlay: Option<Entity>,
    pause_panel: Option<Entity>,
}

#[derive(Component)]
struct WorldRoot;

#[derive(Component)]
struct PlayerEntity {
    id: u64,
}

#[derive(Component)]
struct LocalPlayer;

#[derive(Component)]
struct TargetPosition(Vec3);

#[derive(Component, Default)]
struct PhysicsBody {
    velocity: Vec3,
    on_ground: bool,
}

#[derive(Component)]
struct FollowCamera;

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct ScreenRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
enum FocusedField {
    #[default]
    None,
    PlayerName,
    IpAddress,
    Chat,
}

#[derive(Component, Clone, Copy)]
enum UiAction {
    OpenJoinByIp,
    OpenServerBrowser,
    ConnectByIp,
    ConnectDiscovered(usize),
    BackToMenu,
    ResumeGame,
    ToggleSettings,
    Disconnect,
    Retry,
    Quit,
}

fn main() {
    let discovery_inbox = DiscoveryInbox(Mutex::new(spawn_discovery_thread()));

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Voxel Rust Client".to_string(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(RenetClientPlugin)
        .add_plugins(NetcodeClientPlugin)
        .init_state::<AppScreen>()
        .insert_resource(discovery_inbox)
        .init_resource::<BrowserState>()
        .init_resource::<MenuState>()
        .init_resource::<ConnectionState>()
        .init_resource::<UiState>()
        .init_resource::<ChatState>()
        .init_resource::<WorldCache>()
        .init_resource::<ChunkDataCache>()
        .init_resource::<HudEntities>()
        .add_systems(Startup, setup_cameras)
        .add_systems(Update, button_system)
        .add_systems(Update, collect_discovery_results)
        .add_systems(Update, text_input_system)
        .add_systems(Update, connecting_state_system.run_if(in_state(AppScreen::Connecting)))
        .add_systems(Update, receive_network_messages.run_if(resource_exists::<RenetClient>))
        .add_systems(Update, send_player_input.run_if(client_connected))
        .add_systems(Update, local_player_physics.run_if(in_state(AppScreen::InGame)))
        .add_systems(Update, update_player_visuals.run_if(in_state(AppScreen::InGame)))
        .add_systems(Update, update_camera.run_if(in_state(AppScreen::InGame)))
        .add_systems(Update, toggle_runtime_panels.run_if(in_state(AppScreen::InGame)))
        .add_systems(Update, update_hud.run_if(in_state(AppScreen::InGame)))
        .add_systems(OnEnter(AppScreen::MainMenu), spawn_main_menu)
        .add_systems(OnEnter(AppScreen::JoinByIp), spawn_join_by_ip_screen)
        .add_systems(OnEnter(AppScreen::ServerBrowser), spawn_server_browser)
        .add_systems(OnEnter(AppScreen::Connecting), spawn_connecting_screen)
        .add_systems(OnEnter(AppScreen::Disconnected), spawn_disconnected_screen)
        .add_systems(OnEnter(AppScreen::InGame), setup_world)
        .add_systems(OnExit(AppScreen::MainMenu), despawn_screen)
        .add_systems(OnExit(AppScreen::JoinByIp), despawn_screen)
        .add_systems(OnExit(AppScreen::ServerBrowser), despawn_screen)
        .add_systems(OnExit(AppScreen::Connecting), despawn_screen)
        .add_systems(OnExit(AppScreen::Disconnected), despawn_screen)
        .add_systems(OnExit(AppScreen::InGame), teardown_ingame)
        .add_observer(handle_netcode_error)
        .run();
}

fn setup_cameras(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-12.0, 18.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
        FollowCamera,
    ));
}

fn spawn_main_menu(mut commands: Commands, menu_state: Res<MenuState>) {
    commands.spawn(screen_root()).with_children(|parent| {
        spawn_panel(parent, "Voxel Rust", Some("Standalone client"), |parent| {
            spawn_button(parent, "Join by IP", UiAction::OpenJoinByIp);
            spawn_button(parent, "LAN Browser", UiAction::OpenServerBrowser);
            parent.spawn(text_line(format!(
                "Default player name: {}",
                display_or_placeholder(&menu_state.player_name, "Player")
            )));
            spawn_button(parent, "Quit", UiAction::Quit);
        });
    });
}

fn spawn_join_by_ip_screen(mut commands: Commands, menu_state: Res<MenuState>) {
    commands.spawn(screen_root()).with_children(|parent| {
        spawn_panel(parent, "Join by IP", Some("Enter a LAN or direct server address"), |parent| {
            parent.spawn(text_line(format!(
                "Player Name [{}]: {}",
                focus_marker(menu_state.focused == FocusedField::PlayerName),
                display_or_placeholder(&menu_state.player_name, "Player")
            )));
            parent.spawn(button_line("Edit Name".to_string(), UiAction::OpenJoinByIp));
            parent.spawn(text_line(format!(
                "Server IP:Port [{}]: {}",
                focus_marker(menu_state.focused == FocusedField::IpAddress),
                display_or_placeholder(&menu_state.ip_address, "127.0.0.1:5000")
            )));
            spawn_button(parent, "Connect", UiAction::ConnectByIp);
            spawn_button(parent, "Back", UiAction::BackToMenu);
            parent.spawn(text_line(
                "Press Tab to switch fields. Type directly once focused.".to_string(),
            ));
        });
    });
}

fn spawn_server_browser(mut commands: Commands, browser: Res<BrowserState>) {
    commands.spawn(screen_root()).with_children(|parent| {
        spawn_panel(parent, "LAN Browser", Some("Auto-discovers servers on the local network"), |parent| {
            if browser.servers.is_empty() {
                parent.spawn(text_line("No servers discovered yet. Waiting for broadcast replies...".to_string()));
            } else {
                for (index, server) in browser.servers.iter().enumerate() {
                    spawn_button(
                        parent,
                        format!(
                            "{} [{} / {}] - {} ({})",
                            server.summary.server_name,
                            server.summary.current_players,
                            server.summary.max_players,
                            server.summary.motd,
                            server.addr
                        ),
                        UiAction::ConnectDiscovered(index),
                    );
                }
            }
            spawn_button(parent, "Back", UiAction::BackToMenu);
        });
    });
}

fn spawn_connecting_screen(mut commands: Commands, connection: Res<ConnectionState>) {
    commands.spawn(screen_root()).with_children(|parent| {
        spawn_panel(parent, "Connecting", None, |parent| {
            parent.spawn(text_line(format!(
                "Target: {}",
                connection
                    .current_target
                    .map(|addr| addr.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )));
            parent.spawn(text_line("Waiting for server handshake...".to_string()));
            spawn_button(parent, "Back", UiAction::BackToMenu);
        });
    });
}

fn spawn_disconnected_screen(mut commands: Commands, menu: Res<MenuState>) {
    commands.spawn(screen_root()).with_children(|parent| {
        spawn_panel(parent, "Disconnected", None, |parent| {
            parent.spawn(text_line(menu.disconnect_reason.clone()));
            if menu.reconnect_target.is_some() {
                spawn_button(parent, "Retry", UiAction::Retry);
            }
            spawn_button(parent, "Back", UiAction::BackToMenu);
        });
    });
}

fn setup_world(
    mut commands: Commands,
    mut camera: Single<&mut Visibility, With<FollowCamera>>,
    mut hud: ResMut<HudEntities>,
) {
    **camera = Visibility::Visible;

    let world_root = commands
        .spawn((WorldRoot, Name::new("WorldRoot")))
        .id();
    commands.entity(world_root).with_children(|parent| {
        parent.spawn((
            DirectionalLight {
                illuminance: 12_000.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.8, 0.0)),
        ));
    });

    let hud_root = commands
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            ScreenRoot,
        ))
        .id();

    let fps = commands
        .spawn((
            Text::new("FPS: --"),
            Node {
                position_type: PositionType::Absolute,
                top: px(12.0),
                left: px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();
    let status = commands
        .spawn((
            Text::new("Connecting..."),
            Node {
                position_type: PositionType::Absolute,
                top: px(36.0),
                left: px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();
    let chat = commands
        .spawn((
            Text::new(""),
            Node {
                position_type: PositionType::Absolute,
                left: px(12.0),
                bottom: px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
        ))
        .id();
    let player_list = commands
        .spawn((
            Text::new(""),
            Node {
                position_type: PositionType::Absolute,
                top: px(12.0),
                right: px(12.0),
                display: Display::None,
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();
    let overlay = commands
        .spawn((
            Text::new(""),
            Node {
                position_type: PositionType::Absolute,
                left: percent(35.0),
                top: percent(25.0),
                display: Display::None,
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();
    let pause_panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(24.0),
                bottom: px(24.0),
                width: px(240.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                padding: UiRect::all(px(12.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.14, 0.18, 0.95)),
            BorderColor::all(Color::srgb(0.3, 0.4, 0.55)),
        ))
        .with_children(|parent| {
            parent.spawn(text_line("Pause Menu".to_string()));
            spawn_button(parent, "Resume", UiAction::ResumeGame);
            spawn_button(parent, "Settings", UiAction::ToggleSettings);
            spawn_button(parent, "Disconnect", UiAction::Disconnect);
        })
        .id();

    commands
        .entity(hud_root)
        .add_children(&[fps, status, chat, player_list, overlay, pause_panel]);
    hud.fps = Some(fps);
    hud.status = Some(status);
    hud.chat = Some(chat);
    hud.player_list = Some(player_list);
    hud.overlay = Some(overlay);
    hud.pause_panel = Some(pause_panel);
}

fn despawn_screen(mut commands: Commands, query: Query<Entity, With<ScreenRoot>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

fn teardown_ingame(
    mut commands: Commands,
    world_roots: Query<Entity, With<WorldRoot>>,
    player_entities: Query<Entity, With<PlayerEntity>>,
    screen_entities: Query<Entity, With<ScreenRoot>>,
    mut camera: Single<&mut Visibility, With<FollowCamera>>,
    mut cache: ResMut<WorldCache>,
    mut chunk_data: ResMut<ChunkDataCache>,
    mut hud: ResMut<HudEntities>,
    mut ui_state: ResMut<UiState>,
) {
    **camera = Visibility::Hidden;
    for entity in &world_roots {
        commands.entity(entity).despawn();
    }
    for entity in &player_entities {
        commands.entity(entity).despawn();
    }
    for entity in &screen_entities {
        commands.entity(entity).despawn();
    }
    for entity in cache.chunks.values() {
        commands.entity(*entity).despawn();
    }
    cache.players.clear();
    cache.chunks.clear();
    chunk_data.0.clear();
    *hud = HudEntities::default();
    ui_state.pause_open = false;
    ui_state.settings_open = false;
    ui_state.player_list_open = false;
    ui_state.chat_open = false;
}

fn button_system(
    mut commands: Commands,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut interaction_query: Query<(&Interaction, &UiAction), (Changed<Interaction>, With<Button>)>,
    mut menu_state: ResMut<MenuState>,
    browser: Res<BrowserState>,
    mut connection: ResMut<ConnectionState>,
    mut ui_state: ResMut<UiState>,
    mut quit: MessageWriter<AppExit>,
) {
    for (interaction, action) in &mut interaction_query {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match *action {
            UiAction::OpenJoinByIp => {
                menu_state.focused = match menu_state.focused {
                    FocusedField::PlayerName => FocusedField::IpAddress,
                    _ => FocusedField::PlayerName,
                };
                next_screen.set(AppScreen::JoinByIp);
            }
            UiAction::OpenServerBrowser => {
                next_screen.set(AppScreen::ServerBrowser);
            }
            UiAction::ConnectByIp => {
                let addr = parse_target(&menu_state.ip_address);
                if let Some(addr) = addr {
                    begin_connection(
                        &mut commands,
                        &mut next_screen,
                        &mut menu_state,
                        &mut connection,
                        addr,
                    );
                } else {
                    menu_state.disconnect_reason = "Invalid IP address. Use host:port".to_string();
                    next_screen.set(AppScreen::Disconnected);
                }
            }
            UiAction::ConnectDiscovered(index) => {
                if let Some(server) = browser.servers.get(index) {
                    begin_connection(
                        &mut commands,
                        &mut next_screen,
                        &mut menu_state,
                        &mut connection,
                        server.addr,
                    );
                }
            }
            UiAction::BackToMenu => {
                disconnect_and_clear_network(&mut commands);
                ui_state.pause_open = false;
                ui_state.settings_open = false;
                ui_state.chat_open = false;
                next_screen.set(AppScreen::MainMenu);
            }
            UiAction::ResumeGame => {
                ui_state.pause_open = false;
                ui_state.settings_open = false;
            }
            UiAction::ToggleSettings => {
                ui_state.settings_open = !ui_state.settings_open;
            }
            UiAction::Disconnect => {
                disconnect_and_clear_network(&mut commands);
                menu_state.disconnect_reason = "Disconnected from server".to_string();
                next_screen.set(AppScreen::Disconnected);
            }
            UiAction::Retry => {
                if let Some(target) = menu_state.reconnect_target {
                    begin_connection(
                        &mut commands,
                        &mut next_screen,
                        &mut menu_state,
                        &mut connection,
                        target,
                    );
                }
            }
            UiAction::Quit => {
                quit.write(AppExit::Success);
            }
        }
    }
}

fn begin_connection(
    commands: &mut Commands,
    next_screen: &mut ResMut<NextState<AppScreen>>,
    menu_state: &mut ResMut<MenuState>,
    connection: &mut ResMut<ConnectionState>,
    addr: SocketAddr,
) {
    disconnect_and_clear_network(commands);

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let client_id = current_time.as_millis() as u64;
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(socket) => socket,
        Err(err) => {
            menu_state.disconnect_reason = format!("Failed to bind client socket: {err}");
            next_screen.set(AppScreen::Disconnected);
            return;
        }
    };
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr: addr,
        user_data: None,
    };
    let transport = match NetcodeClientTransport::new(current_time, authentication, socket) {
        Ok(transport) => transport,
        Err(err) => {
            menu_state.disconnect_reason = format!("Failed to create client transport: {err}");
            next_screen.set(AppScreen::Disconnected);
            return;
        }
    };

    commands.insert_resource(RenetClient::new(ConnectionConfig::default()));
    commands.insert_resource(transport);

    connection.current_target = Some(addr);
    connection.join_sent = false;
    connection.local_client_id = None;
    connection.connected_server_name.clear();
    connection.motd.clear();
    menu_state.reconnect_target = Some(addr);
    next_screen.set(AppScreen::Connecting);
}

fn connecting_state_system(
    mut client: ResMut<RenetClient>,
    mut connection: ResMut<ConnectionState>,
    menu_state: Res<MenuState>,
) {
    if client.is_connected() && !connection.join_sent {
        let player_name = display_or_placeholder(&menu_state.player_name, "Player");
        let join = ClientMessage::Join {
            player_name: player_name.to_string(),
        };
        if let Ok(bytes) = bincode::serialize(&join) {
            client.send_message(DefaultChannel::ReliableOrdered, bytes);
            connection.join_sent = true;
        }
    }
}

fn receive_network_messages(
    mut commands: Commands,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut client: ResMut<RenetClient>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<WorldCache>,
    mut chunk_data_cache: ResMut<ChunkDataCache>,
    mut connection: ResMut<ConnectionState>,
    mut menu: ResMut<MenuState>,
    mut chat: ResMut<ChatState>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let Ok(message) = bincode::deserialize::<ServerMessage>(&message) else {
            continue;
        };
        match message {
            ServerMessage::Welcome {
                client_id,
                server_name,
                motd,
                ..
            } => {
                connection.local_client_id = Some(client_id);
                connection.connected_server_name = server_name;
                connection.motd = motd;
                next_screen.set(AppScreen::InGame);
            }
            ServerMessage::PlayerConnected {
                id,
                name,
                translation,
            } => {
                let translation = Vec3::from(translation);
                if let Some(entity) = cache.players.get(&id).copied() {
                    commands.entity(entity).insert(TargetPosition(translation));
                } else {
                    let material = if Some(id) == connection.local_client_id {
                        materials.add(Color::srgb(0.9, 0.2, 0.2))
                    } else {
                        materials.add(Color::srgb(0.2, 0.5, 0.9))
                    };
                    let mut entity = commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(0.8, 1.8, 0.8))),
                        MeshMaterial3d(material),
                        Transform::from_translation(translation),
                        PlayerEntity { id },
                        TargetPosition(translation),
                        Name::new(name.clone()),
                    ));
                    if Some(id) == connection.local_client_id {
                        entity.insert((LocalPlayer, PhysicsBody::default()));
                    }
                    cache.players.insert(id, entity.id());
                }
                push_chat_line(&mut chat, format!("{name} connected"));
            }
            ServerMessage::PlayerDisconnected { id, reason } => {
                if let Some(entity) = cache.players.remove(&id) {
                    commands.entity(entity).despawn();
                }
                if let Some(reason) = reason {
                    push_chat_line(&mut chat, format!("Player {id} left: {reason}"));
                }
            }
            ServerMessage::Chat { from, message } => {
                push_chat_line(&mut chat, format!("{from}: {message}"));
            }
            ServerMessage::ChunkData { key, data } => {
                let chunk_vec = data.into_vec();
                chunk_data_cache.0.insert(key, chunk_vec.clone());
                let mesh = generate_chunk_mesh(&chunk_vec, key);
                let entity = commands
                    .spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.7, 0.3))),
                        Transform::from_translation(key.world_pos()),
                    ))
                    .id();
                if let Some(old) = cache.chunks.insert(key, entity) {
                    commands.entity(old).despawn();
                }
            }
            ServerMessage::ServerNotice { message } => {
                push_chat_line(&mut chat, message);
            }
            ServerMessage::Disconnect { reason } => {
                menu.disconnect_reason = reason;
                disconnect_and_clear_network(&mut commands);
                next_screen.set(AppScreen::Disconnected);
            }
            ServerMessage::StateSnapshot { .. } => {}
        }
    }

    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let Ok(ServerMessage::StateSnapshot { tick, players }) =
            bincode::deserialize::<ServerMessage>(&message)
        else {
            continue;
        };
        connection_local_snapshot_update(&mut commands, &mut cache, players, tick);
    }
}

fn connection_local_snapshot_update(
    commands: &mut Commands,
    cache: &mut WorldCache,
    players: Vec<PlayerSnapshot>,
    _tick: u32,
) {
    for snapshot in players {
        if let Some(entity) = cache.players.get(&snapshot.id).copied() {
            commands
                .entity(entity)
                .insert(TargetPosition(Vec3::from(snapshot.translation)));
        }
    }
}

fn send_player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut client: ResMut<RenetClient>,
    mut ui_state: ResMut<UiState>,
    chat: Res<ChatState>,
) {
    if ui_state.pause_open || ui_state.chat_open {
        return;
    }

    let mut flags = InputFlags::default();
    if keys.pressed(KeyCode::KeyW) {
        flags |= InputFlags::FORWARD;
    }
    if keys.pressed(KeyCode::KeyS) {
        flags |= InputFlags::BACKWARD;
    }
    if keys.pressed(KeyCode::KeyA) {
        flags |= InputFlags::LEFT;
    }
    if keys.pressed(KeyCode::KeyD) {
        flags |= InputFlags::RIGHT;
    }
    if keys.just_pressed(KeyCode::Space) {
        flags |= InputFlags::JUMP;
    }

    ui_state.tick = ui_state.tick.wrapping_add(1);
    let message = ClientMessage::Input {
        tick: ui_state.tick,
        flags,
    };
    if let Ok(bytes) = bincode::serialize(&message) {
        client.send_message(DefaultChannel::ReliableOrdered, bytes);
    }

    if !chat.current_input.is_empty() {
        let _ = &chat.current_input;
    }
}

fn local_player_physics(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_state: Res<UiState>,
    chunk_data: Res<ChunkDataCache>,
    mut local_player: Query<(&mut Transform, &mut PhysicsBody), With<LocalPlayer>>,
) {
    if ui_state.pause_open || ui_state.chat_open {
        return;
    }

    let Ok((mut transform, mut body)) = local_player.single_mut() else {
        return;
    };

    let mut move_dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        move_dir.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        move_dir.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        move_dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        move_dir.x += 1.0;
    }
    if move_dir.length_squared() > 0.0 {
        move_dir = move_dir.normalize();
    }

    let speed = 8.5;
    body.velocity.x = move_dir.x * speed;
    body.velocity.z = move_dir.z * speed;
    if keys.just_pressed(KeyCode::Space) && body.on_ground {
        body.velocity.y = 10.0;
        body.on_ground = false;
    }

    let dt = time.delta_secs();
    body.velocity.y -= GRAVITY * dt;

    let move_y = Vec3::new(0.0, body.velocity.y * dt, 0.0);
    if !collides_with_world(&chunk_data, transform.translation + move_y) {
        transform.translation += move_y;
        body.on_ground = false;
    } else {
        if body.velocity.y < 0.0 {
            body.on_ground = true;
        }
        body.velocity.y = 0.0;
    }

    let move_x = Vec3::new(body.velocity.x * dt, 0.0, 0.0);
    if !collides_with_world(&chunk_data, transform.translation + move_x) {
        transform.translation += move_x;
    } else {
        body.velocity.x = 0.0;
    }

    let move_z = Vec3::new(0.0, 0.0, body.velocity.z * dt);
    if !collides_with_world(&chunk_data, transform.translation + move_z) {
        transform.translation += move_z;
    } else {
        body.velocity.z = 0.0;
    }
}

fn update_player_visuals(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &TargetPosition)>,
) {
    for (mut transform, target) in &mut query {
        transform.translation = transform
            .translation
            .lerp(target.0, (time.delta_secs() * 10.0).clamp(0.0, 1.0));
    }
}

fn update_camera(
    mut camera: Single<&mut Transform, (With<FollowCamera>, Without<LocalPlayer>)>,
    local_player: Query<&Transform, With<LocalPlayer>>,
) {
    if let Ok(player) = local_player.single() {
        let target = player.translation + Vec3::new(-10.0, 14.0, 12.0);
        camera.translation = camera.translation.lerp(target, 0.15);
        camera.look_at(player.translation, Vec3::Y);
    }
}

fn toggle_runtime_panels(
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut ui_state: ResMut<UiState>,
    mut menu: ResMut<MenuState>,
    mut chat_state: ResMut<ChatState>,
    mut client: Option<ResMut<RenetClient>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        ui_state.pause_open = !ui_state.pause_open;
        if !ui_state.pause_open {
            ui_state.settings_open = false;
        }
    }

    ui_state.player_list_open = keys.pressed(KeyCode::Tab);

    if keys.just_pressed(KeyCode::Enter) {
        if ui_state.chat_open && !chat_state.current_input.trim().is_empty() {
            if let Some(client) = client.as_deref_mut() {
                let message = ClientMessage::Chat {
                    message: chat_state.current_input.trim().to_string(),
                };
                if let Ok(bytes) = bincode::serialize(&message) {
                    client.send_message(DefaultChannel::ReliableOrdered, bytes);
                }
            }
            chat_state.current_input.clear();
            ui_state.chat_open = false;
            menu.focused = FocusedField::None;
        } else {
            ui_state.chat_open = !ui_state.chat_open;
            menu.focused = if ui_state.chat_open {
                FocusedField::Chat
            } else {
                FocusedField::None
            };
        }
    }

    if !ui_state.chat_open {
        return;
    }

    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match (&event.logical_key, &event.text) {
            (Key::Backspace, _) => {
                chat_state.current_input.pop();
            }
            (Key::Escape, _) => {
                ui_state.chat_open = false;
                menu.focused = FocusedField::None;
            }
            (_, Some(text)) => {
                if text.chars().all(is_printable_char) {
                    chat_state.current_input.push_str(text);
                }
            }
            _ => {}
        }
    }
}

fn update_hud(
    diagnostics: Res<DiagnosticsStore>,
    hud: Res<HudEntities>,
    connection: Res<ConnectionState>,
    ui_state: Res<UiState>,
    chat_state: Res<ChatState>,
    cache: Res<WorldCache>,
    player_names: Query<(&PlayerEntity, &Name)>,
    mut texts: Query<&mut Text>,
    mut nodes: Query<&mut Node>,
) {
    if let Some(entity) = hud.fps {
        if let Ok(mut text) = texts.get_mut(entity) {
            if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()) {
                *text = Text::new(format!("FPS: {:.0}", fps));
            }
        }
    }

    if let Some(entity) = hud.status {
        if let Ok(mut text) = texts.get_mut(entity) {
            *text = Text::new(format!(
                "{} | {} | Players: {}",
                connection.connected_server_name,
                connection.motd,
                cache.players.len()
            ));
        }
    }

    if let Some(entity) = hud.chat {
        if let Ok(mut text) = texts.get_mut(entity) {
            let mut lines: Vec<String> = chat_state.lines.iter().cloned().collect();
            if ui_state.chat_open {
                lines.push(format!("> {}", chat_state.current_input));
            }
            *text = Text::new(lines.join("\n"));
        }
    }

    if let Some(entity) = hud.player_list {
        if let Ok(mut text) = texts.get_mut(entity) {
            let mut players: Vec<String> = player_names
                .iter()
                .map(|(player, name)| format!("{} ({})", name.as_str(), player.id))
                .collect();
            players.sort();
            *text = Text::new(players.join("\n"));
        }
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.display = if ui_state.player_list_open {
                Display::Flex
            } else {
                Display::None
            };
        }
    }

    if let Some(entity) = hud.overlay {
        if let Ok(mut text) = texts.get_mut(entity) {
            let overlay = if ui_state.pause_open {
                if ui_state.settings_open {
                    "Paused\nSettings\n- LAN discovery only in this build\n- Global list placeholder kept for later\nPress Esc to close"
                } else {
                    "Paused\nEsc: Resume\nTab: Player list\nEnter: Chat"
                }
            } else {
                ""
            };
            *text = Text::new(overlay);
        }
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.display = if ui_state.pause_open {
                Display::Flex
            } else {
                Display::None
            };
        }
    }

    if let Some(entity) = hud.pause_panel {
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.display = if ui_state.pause_open {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

fn collect_discovery_results(mut browser: ResMut<BrowserState>, inbox: Res<DiscoveryInbox>) {
    let Ok(receiver) = inbox.0.lock() else {
        return;
    };
    while let Ok(server) = receiver.try_recv() {
        if let Some(existing) = browser.servers.iter_mut().find(|entry| entry.addr == server.addr) {
            *existing = server;
        } else {
            browser.servers.push(server);
            browser.servers.sort_by(|a, b| a.summary.server_name.cmp(&b.summary.server_name));
        }
    }
}

fn text_input_system(
    screen: Res<State<AppScreen>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut menu_state: ResMut<MenuState>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if *screen.get() != AppScreen::JoinByIp {
        return;
    }

    if keys.just_pressed(KeyCode::Tab) {
        menu_state.focused = match menu_state.focused {
            FocusedField::PlayerName => FocusedField::IpAddress,
            _ => FocusedField::PlayerName,
        };
    }

    window.ime_enabled = menu_state.focused != FocusedField::None;

    let target = match menu_state.focused {
        FocusedField::PlayerName => Some(&mut menu_state.player_name),
        FocusedField::IpAddress => Some(&mut menu_state.ip_address),
        _ => None,
    };
    let Some(target) = target else {
        return;
    };

    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match (&event.logical_key, &event.text) {
            (Key::Backspace, _) => {
                target.pop();
            }
            (_, Some(text)) => {
                if text.chars().all(is_printable_char) {
                    target.push_str(text);
                }
            }
            _ => {}
        }
    }
}

fn handle_netcode_error(
    error: On<NetcodeErrorEvent>,
    mut commands: Commands,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut menu: ResMut<MenuState>,
) {
    menu.disconnect_reason = format!("Network error: {}", *error);
    disconnect_and_clear_network(&mut commands);
    next_screen.set(AppScreen::Disconnected);
}

fn disconnect_and_clear_network(commands: &mut Commands) {
    commands.remove_resource::<RenetClient>();
    commands.remove_resource::<NetcodeClientTransport>();
}

fn spawn_discovery_thread() -> Receiver<DiscoveredServer> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
            return;
        };
        let _ = socket.set_broadcast(true);
        let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));
        let probe = bincode::serialize(&DiscoveryMessage::Probe { protocol_id: PROTOCOL_ID })
            .unwrap_or_default();
        let destination = SocketAddr::new(Ipv4Addr::BROADCAST.into(), DEFAULT_DISCOVERY_PORT);
        let mut buffer = [0_u8; 2048];

        loop {
            let _ = socket.send_to(&probe, destination);
            loop {
                match socket.recv_from(&mut buffer) {
                    Ok((size, source)) => {
                        let Ok(DiscoveryMessage::Announce(summary)) =
                            bincode::deserialize::<DiscoveryMessage>(&buffer[..size])
                        else {
                            continue;
                        };
                        let addr = SocketAddr::new(source.ip(), summary.game_port);
                        let _ = tx.send(DiscoveredServer { addr, summary });
                    }
                    Err(err)
                        if err.kind() == std::io::ErrorKind::TimedOut
                            || err.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        break;
                    }
                    Err(_) => break,
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
    rx
}

fn generate_chunk_mesh(data: &[u8], _key: ChunkKey) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut base: u32 = 0;

    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                if data[chunk_index(x, y, z)] == 0 {
                    continue;
                }
                let fx = x as f32;
                let fy = y as f32;
                let fz = z as f32;

                let faces = [
                    (
                        is_air(data, x as isize, y as isize + 1, z as isize),
                        [[fx, fy + 1.0, fz], [fx + 1.0, fy + 1.0, fz], [fx + 1.0, fy + 1.0, fz + 1.0], [fx, fy + 1.0, fz + 1.0]],
                        [0.0, 1.0, 0.0],
                    ),
                    (
                        is_air(data, x as isize, y as isize - 1, z as isize),
                        [[fx, fy, fz + 1.0], [fx + 1.0, fy, fz + 1.0], [fx + 1.0, fy, fz], [fx, fy, fz]],
                        [0.0, -1.0, 0.0],
                    ),
                    (
                        is_air(data, x as isize + 1, y as isize, z as isize),
                        [[fx + 1.0, fy, fz], [fx + 1.0, fy, fz + 1.0], [fx + 1.0, fy + 1.0, fz + 1.0], [fx + 1.0, fy + 1.0, fz]],
                        [1.0, 0.0, 0.0],
                    ),
                    (
                        is_air(data, x as isize - 1, y as isize, z as isize),
                        [[fx, fy, fz + 1.0], [fx, fy, fz], [fx, fy + 1.0, fz], [fx, fy + 1.0, fz + 1.0]],
                        [-1.0, 0.0, 0.0],
                    ),
                    (
                        is_air(data, x as isize, y as isize, z as isize + 1),
                        [[fx + 1.0, fy, fz + 1.0], [fx, fy, fz + 1.0], [fx, fy + 1.0, fz + 1.0], [fx + 1.0, fy + 1.0, fz + 1.0]],
                        [0.0, 0.0, 1.0],
                    ),
                    (
                        is_air(data, x as isize, y as isize, z as isize - 1),
                        [[fx, fy, fz], [fx + 1.0, fy, fz], [fx + 1.0, fy + 1.0, fz], [fx, fy + 1.0, fz]],
                        [0.0, 0.0, -1.0],
                    ),
                ];

                for (visible, face_positions, normal) in faces {
                    if !visible {
                        continue;
                    }
                    positions.extend_from_slice(&face_positions);
                    normals.extend_from_slice(&[normal; 4]);
                    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                    base += 4;
                }
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn is_air(data: &[u8], x: isize, y: isize, z: isize) -> bool {
    if x < 0 || y < 0 || z < 0 {
        return true;
    }
    let x = x as usize;
    let y = y as usize;
    let z = z as usize;
    if x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE {
        return true;
    }
    data[chunk_index(x, y, z)] == 0
}

fn collides_with_world(chunk_data: &ChunkDataCache, position: Vec3) -> bool {
    let half_width = PLAYER_WIDTH * 0.5;
    let corners = [
        Vec3::new(position.x - half_width, position.y, position.z - half_width),
        Vec3::new(position.x + half_width, position.y, position.z - half_width),
        Vec3::new(position.x - half_width, position.y, position.z + half_width),
        Vec3::new(position.x + half_width, position.y, position.z + half_width),
        Vec3::new(position.x - half_width, position.y + PLAYER_HEIGHT, position.z - half_width),
        Vec3::new(position.x + half_width, position.y + PLAYER_HEIGHT, position.z - half_width),
        Vec3::new(position.x - half_width, position.y + PLAYER_HEIGHT, position.z + half_width),
        Vec3::new(position.x + half_width, position.y + PLAYER_HEIGHT, position.z + half_width),
    ];

    corners.into_iter().any(|corner| is_block_solid(chunk_data, corner))
}

fn is_block_solid(chunk_data: &ChunkDataCache, world_pos: Vec3) -> bool {
    let key = ChunkKey::from_world_pos(world_pos);
    let Some(chunk) = chunk_data.0.get(&key) else {
        return false;
    };

    let local = world_pos - key.world_pos();
    if local.x < 0.0 || local.y < 0.0 || local.z < 0.0 {
        return false;
    }

    let x = local.x.floor() as usize;
    let y = local.y.floor() as usize;
    let z = local.z.floor() as usize;
    if x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE {
        return false;
    }

    chunk[chunk_index(x, y, z)] != 0
}

fn spawn_panel(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    subtitle: Option<&str>,
    children: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.08, 0.85)),
            MenuRoot,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        width: px(640.0),
                        min_height: px(320.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(12.0),
                        padding: UiRect::all(px(20.0)),
                        border: UiRect::all(px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.12, 0.14, 0.18, 0.95)),
                    BorderColor::all(Color::srgb(0.3, 0.4, 0.55)),
                ))
                .with_children(|parent| {
                    parent.spawn(text_line(title.to_string()));
                    if let Some(subtitle) = subtitle {
                        parent.spawn(text_line(subtitle.to_string()));
                    }
                    children(parent);
                });
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, label: impl Into<String>, action: UiAction) {
    parent.spawn(button_line(label.into(), action));
}

fn button_line(label: String, action: UiAction) -> impl Bundle {
    (
        Button,
        Node {
            width: percent(100.0),
            min_height: px(42.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.24, 0.3)),
        action,
        children![text_line(label)],
    )
}

fn text_line(text: String) -> impl Bundle {
    (
        Text::new(text),
        TextColor(Color::WHITE),
        TextFont {
            font_size: 22.0,
            ..default()
        },
    )
}

fn screen_root() -> impl Bundle {
    (
        Node {
            width: percent(100.0),
            height: percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        ScreenRoot,
    )
}

fn parse_target(input: &str) -> Option<SocketAddr> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some(SocketAddr::new(
            Ipv4Addr::LOCALHOST.into(),
            DEFAULT_GAME_PORT,
        ));
    }
    if trimmed.contains(':') {
        trimmed.parse().ok()
    } else {
        format!("{trimmed}:{DEFAULT_GAME_PORT}").parse().ok()
    }
}

fn display_or_placeholder<'a>(value: &'a str, placeholder: &'a str) -> &'a str {
    if value.trim().is_empty() {
        placeholder
    } else {
        value
    }
}

fn focus_marker(active: bool) -> &'static str {
    if active {
        "typing"
    } else {
        "idle"
    }
}

fn push_chat_line(chat: &mut ChatState, message: String) {
    chat.lines.push_back(message);
    while chat.lines.len() > 8 {
        chat.lines.pop_front();
    }
}

fn is_printable_char(chr: char) -> bool {
    let is_private = ('\u{e000}'..='\u{f8ff}').contains(&chr)
        || ('\u{f0000}'..='\u{ffffd}').contains(&chr)
        || ('\u{100000}'..='\u{10fffd}').contains(&chr);
    !is_private && !chr.is_ascii_control()
}
