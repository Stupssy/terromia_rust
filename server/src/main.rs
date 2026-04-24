use clap::Parser;
use glam::Vec3;
use log::{info, warn};
use noise::{NoiseFn, Perlin};
use renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use shared::{
    chunk_index, ChunkKey, ClientMessage, DiscoveryMessage, InputFlags, PlayerSnapshot,
    ServerConfigData, ServerMessage, ServerSummary, CHUNK_SIZE, PLAYER_HEIGHT, PROTOCOL_ID,
    TICK_RATE,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

#[derive(Parser, Debug)]
#[command(author, version, about = "Dedicated voxel server")]
struct Cli {
    #[arg(long, default_value = "server.toml")]
    config: PathBuf,
    #[arg(long)]
    bind: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    discovery_port: Option<u16>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    max_clients: Option<usize>,
}

#[derive(Debug)]
enum AdminCommand {
    Status,
    List,
    Say(String),
    Kick(u64),
    Stop,
    Help,
}

#[derive(Clone)]
struct Player {
    id: u64,
    name: String,
    translation: Vec3,
    last_input: InputFlags,
}

struct World {
    perlin: Perlin,
    chunks: HashMap<ChunkKey, Vec<u8>>,
    players: HashMap<u64, Player>,
    view_distance: i32,
}

impl World {
    fn new(config: &ServerConfigData) -> Self {
        Self {
            perlin: Perlin::new(config.world_seed),
            chunks: HashMap::new(),
            players: HashMap::new(),
            view_distance: config.view_distance,
        }
    }

    fn spawn_position(&self) -> Vec3 {
        let y = self.height_at(0.0, 0.0) + PLAYER_HEIGHT + 0.5;
        Vec3::new(0.0, y, 0.0)
    }

    fn height_at(&self, x: f32, z: f32) -> f32 {
        (self.perlin.get([x as f64 * 0.04, z as f64 * 0.04]) * 6.0 + 10.0) as f32
    }

    fn ensure_chunks_for_player(&mut self, player_pos: Vec3) -> Vec<(ChunkKey, Vec<u8>)> {
        let mut generated = Vec::new();
        for x in -self.view_distance..=self.view_distance {
            for z in -self.view_distance..=self.view_distance {
                let offset = Vec3::new(
                    x as f32 * CHUNK_SIZE as f32,
                    0.0,
                    z as f32 * CHUNK_SIZE as f32,
                );
                let key = ChunkKey::from_world_pos(player_pos + offset);
                if !self.chunks.contains_key(&key) {
                    let data = generate_chunk_data(&self.perlin, key);
                    self.chunks.insert(key, data.clone());
                    generated.push((key, data));
                }
            }
        }
        generated
    }

    fn update(&mut self, dt: f32) -> Vec<PlayerSnapshot> {
        let perlin = self.perlin;
        for player in self.players.values_mut() {
            let move_speed = 8.0 * dt;
            if player.last_input.contains(InputFlags::FORWARD) {
                player.translation.z -= move_speed;
            }
            if player.last_input.contains(InputFlags::BACKWARD) {
                player.translation.z += move_speed;
            }
            if player.last_input.contains(InputFlags::LEFT) {
                player.translation.x -= move_speed;
            }
            if player.last_input.contains(InputFlags::RIGHT) {
                player.translation.x += move_speed;
            }

            player.translation.y =
                (perlin.get([player.translation.x as f64 * 0.04, player.translation.z as f64 * 0.04])
                    * 6.0
                    + 10.0) as f32
                    + PLAYER_HEIGHT
                    + 0.5;
        }

        self.players
            .values()
            .map(|player| PlayerSnapshot {
                id: player.id,
                name: player.name.clone(),
                translation: player.translation.to_array(),
            })
            .collect()
    }
}

struct DiscoveryService {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl DiscoveryService {
    fn start(
        discovery_port: u16,
        summary: Arc<Mutex<ServerSummary>>,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, discovery_port))?;
        socket.set_read_timeout(Some(Duration::from_millis(250)))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut buffer = [0_u8; 2048];
            while !thread_stop.load(Ordering::Relaxed) {
                match socket.recv_from(&mut buffer) {
                    Ok((size, source)) => {
                        let Ok(message) = bincode::deserialize::<DiscoveryMessage>(&buffer[..size]) else {
                            continue;
                        };
                        if let DiscoveryMessage::Probe { protocol_id } = message {
                            if protocol_id != PROTOCOL_ID {
                                continue;
                            }
                            let response = {
                                let guard = summary.lock().expect("discovery summary poisoned");
                                DiscoveryMessage::Announce(guard.clone())
                            };
                            let Ok(payload) = bincode::serialize(&response) else {
                                continue;
                            };
                            let _ = socket.send_to(&payload, source);
                        }
                    }
                    Err(err)
                        if err.kind() == io::ErrorKind::WouldBlock
                            || err.kind() == io::ErrorKind::TimedOut => {}
                    Err(err) => {
                        warn!("Discovery service error: {err}");
                    }
                }
            }
        });

        Ok(Self { stop, handle })
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()?;

    let cli = Cli::parse();
    let config = load_config(&cli)?;
    info!(
        "Starting '{}' on {}:{} (discovery {})",
        config.server_name, config.bind_ip, config.game_port, config.discovery_port
    );

    let bind_ip: IpAddr = config.bind_ip.parse()?;
    let public_addr = SocketAddr::new(bind_ip, config.game_port);
    let socket = UdpSocket::bind(public_addr)?;
    socket.set_nonblocking(true)?;

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let server_config = ServerConfig {
        current_time,
        max_clients: config.max_clients,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };

    let mut transport = NetcodeServerTransport::new(server_config, socket)?;
    let mut server = RenetServer::new(ConnectionConfig::default());
    let mut world = World::new(&config);

    let summary = Arc::new(Mutex::new(ServerSummary {
        server_name: config.server_name.clone(),
        motd: config.motd.clone(),
        current_players: 0,
        max_players: config.max_clients,
        game_port: config.game_port,
    }));
    let discovery = DiscoveryService::start(config.discovery_port, Arc::clone(&summary))?;
    let admin_rx = spawn_admin_console();

    let mut running = true;
    let mut tick: u32 = 0;
    let tick_duration = Duration::from_secs_f64(1.0 / TICK_RATE as f64);
    let mut last_update = Instant::now();

    while running {
        let frame_start = Instant::now();
        let dt = frame_start.duration_since(last_update);
        last_update = frame_start;

        server.update(dt);
        transport.update(dt, &mut server)?;

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    info!("Client {client_id} connected");
                    let spawn = world.spawn_position();
                    let player = Player {
                        id: client_id,
                        name: format!("Player{client_id}"),
                        translation: spawn,
                        last_input: InputFlags::default(),
                    };
                    world.players.insert(client_id, player.clone());
                    sync_summary(&summary, &server, &config);

                    let welcome = ServerMessage::Welcome {
                        client_id,
                        server_name: config.server_name.clone(),
                        motd: config.motd.clone(),
                        spawn_position: spawn.to_array(),
                    };
                    send_reliable(&mut server, client_id, &welcome);

                    for existing in world.players.values() {
                        let message = ServerMessage::PlayerConnected {
                            id: existing.id,
                            name: existing.name.clone(),
                            translation: existing.translation.to_array(),
                        };
                        send_reliable(&mut server, client_id, &message);
                    }

                    for (key, data) in world.ensure_chunks_for_player(spawn) {
                        send_reliable(
                            &mut server,
                            client_id,
                            &ServerMessage::ChunkData {
                                key,
                                data: data.into_boxed_slice(),
                            },
                        );
                    }
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    info!("Client {client_id} disconnected: {reason:?}");
                    world.players.remove(&client_id);
                    sync_summary(&summary, &server, &config);
                    server.broadcast_message(
                        DefaultChannel::ReliableOrdered,
                        bincode::serialize(&ServerMessage::PlayerDisconnected {
                            id: client_id,
                            reason: Some(format!("{reason:?}")),
                        })?,
                    );
                }
            }
        }

        for client_id in server.clients_id() {
            while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
            {
                let Ok(message) = bincode::deserialize::<ClientMessage>(&message) else {
                    continue;
                };
                match message {
                    ClientMessage::Join { player_name } => {
                        if let Some(player) = world.players.get_mut(&client_id) {
                            player.name = sanitize_name(player_name);
                            let joined = ServerMessage::PlayerConnected {
                                id: player.id,
                                name: player.name.clone(),
                                translation: player.translation.to_array(),
                            };
                            server.broadcast_message(
                                DefaultChannel::ReliableOrdered,
                                bincode::serialize(&joined)?,
                            );
                            server.broadcast_message(
                                DefaultChannel::ReliableOrdered,
                                bincode::serialize(&ServerMessage::ServerNotice {
                                    message: format!("{} joined the server", player.name),
                                })?,
                            );
                        }
                    }
                    ClientMessage::Input { flags, .. } => {
                        let player_translation = if let Some(player) = world.players.get_mut(&client_id) {
                            player.last_input = flags;
                            Some(player.translation)
                        } else {
                            None
                        };
                        if let Some(player_translation) = player_translation {
                            for (key, data) in world.ensure_chunks_for_player(player_translation) {
                                server.broadcast_message(
                                    DefaultChannel::ReliableOrdered,
                                    bincode::serialize(&ServerMessage::ChunkData {
                                        key,
                                        data: data.into_boxed_slice(),
                                    })?,
                                );
                            }
                        }
                    }
                    ClientMessage::Chat { message } => {
                        if let Some(player) = world.players.get(&client_id) {
                            let message = ServerMessage::Chat {
                                from: player.name.clone(),
                                message: message.trim().chars().take(180).collect(),
                            };
                            server.broadcast_message(
                                DefaultChannel::ReliableOrdered,
                                bincode::serialize(&message)?,
                            );
                        }
                    }
                }
            }
        }

        while let Ok(command) = admin_rx.try_recv() {
            running = handle_admin_command(command, &mut server, &world);
            if !running {
                break;
            }
        }

        tick = tick.wrapping_add(1);
        let snapshot = ServerMessage::StateSnapshot {
            tick,
            players: world.update(dt.as_secs_f32()),
        };
        server.broadcast_message(
            DefaultChannel::Unreliable,
            bincode::serialize(&snapshot)?,
        );

        transport.send_packets(&mut server);

        let elapsed = frame_start.elapsed();
        if elapsed < tick_duration {
            thread::sleep(tick_duration - elapsed);
        }
    }

    discovery.stop();
    info!("Server stopped");
    Ok(())
}

fn load_config(cli: &Cli) -> Result<ServerConfigData, Box<dyn std::error::Error>> {
    let mut config = if cli.config.exists() {
        toml::from_str::<ServerConfigData>(&fs::read_to_string(&cli.config)?)?
    } else {
        let default = ServerConfigData::default();
        fs::write(&cli.config, toml::to_string_pretty(&default)?)?;
        default
    };

    if let Some(bind) = &cli.bind {
        config.bind_ip = bind.clone();
    }
    if let Some(port) = cli.port {
        config.game_port = port;
    }
    if let Some(discovery_port) = cli.discovery_port {
        config.discovery_port = discovery_port;
    }
    if let Some(name) = &cli.name {
        config.server_name = name.clone();
    }
    if let Some(max_clients) = cli.max_clients {
        config.max_clients = max_clients;
    }

    Ok(config)
}

fn generate_chunk_data(perlin: &Perlin, key: ChunkKey) -> Vec<u8> {
    let mut data = vec![0_u8; CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE];
    let base = key.0 * CHUNK_SIZE as i32;

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let wx = (base.x + x as i32) as f64;
            let wz = (base.z + z as i32) as f64;
            let height = (perlin.get([wx * 0.04, wz * 0.04]) * 6.0 + 10.0) as i32;

            for y in 0..CHUNK_SIZE {
                let wy = base.y + y as i32;
                if wy <= height {
                    data[chunk_index(x, y, z)] = 1;
                }
            }
        }
    }

    data
}

fn send_reliable(server: &mut RenetServer, client_id: u64, message: &ServerMessage) {
    if let Ok(payload) = bincode::serialize(message) {
        server.send_message(client_id, DefaultChannel::ReliableOrdered, payload);
    }
}

fn sync_summary(
    summary: &Arc<Mutex<ServerSummary>>,
    server: &RenetServer,
    config: &ServerConfigData,
) {
    if let Ok(mut guard) = summary.lock() {
        guard.current_players = server.clients_id().len();
        guard.max_players = config.max_clients;
        guard.server_name = config.server_name.clone();
        guard.motd = config.motd.clone();
        guard.game_port = config.game_port;
    }
}

fn spawn_admin_console() -> Receiver<AdminCommand> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let trimmed = line.trim();
            let command = if trimmed.eq_ignore_ascii_case("status") {
                Some(AdminCommand::Status)
            } else if trimmed.eq_ignore_ascii_case("list") {
                Some(AdminCommand::List)
            } else if trimmed.eq_ignore_ascii_case("stop") {
                Some(AdminCommand::Stop)
            } else if trimmed.eq_ignore_ascii_case("help") {
                Some(AdminCommand::Help)
            } else if let Some(message) = trimmed.strip_prefix("say ") {
                Some(AdminCommand::Say(message.to_string()))
            } else if let Some(id) = trimmed.strip_prefix("kick ").and_then(|id| id.parse().ok()) {
                Some(AdminCommand::Kick(id))
            } else {
                None
            };

            if let Some(command) = command {
                if tx.send(command).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

fn handle_admin_command(command: AdminCommand, server: &mut RenetServer, world: &World) -> bool {
    match command {
        AdminCommand::Status => {
            info!("{} players connected", server.clients_id().len());
        }
        AdminCommand::List => {
            for player in world.players.values() {
                info!(
                    "Player {} '{}' at ({:.1}, {:.1}, {:.1})",
                    player.id, player.name, player.translation.x, player.translation.y, player.translation.z
                );
            }
        }
        AdminCommand::Say(message) => {
            let chat = ServerMessage::Chat {
                from: "SERVER".to_string(),
                message,
            };
            if let Ok(payload) = bincode::serialize(&chat) {
                server.broadcast_message(DefaultChannel::ReliableOrdered, payload);
            }
        }
        AdminCommand::Kick(client_id) => {
            warn!("Kicking client {client_id}");
            server.disconnect(client_id);
        }
        AdminCommand::Stop => {
            info!("Shutdown requested");
            return false;
        }
        AdminCommand::Help => {
            info!("Commands: status, list, say <msg>, kick <id>, stop, help");
        }
    }
    true
}

fn sanitize_name(name: String) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .take(20)
        .collect();
    if cleaned.is_empty() {
        "Player".to_string()
    } else {
        cleaned
    }
}
