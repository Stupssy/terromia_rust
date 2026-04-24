use glam::Vec3;
use noise::{NoiseFn, Perlin};
use renet::{ConnectionConfig, RenetServer, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use shared::{ChunkKey, ClientMessage, InputFlags, ServerMessage, PORT, PROTOCOL_ID, CHUNK_SIZE};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

// --- Domain: Player ---
struct Player {
    id: u64,
    translation: Vec3,
}

// --- Domain: World ---
struct World {
    chunks: HashMap<ChunkKey, bool>, // Just track existence for demo; real app stores data
    players: HashMap<u64, Player>,
    perlin: Perlin,
}

impl World {
    fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            players: HashMap::new(),
            perlin: Perlin::new(42),
        }
    }

    // Returns (player_states, chunks_to_send)
    fn update(&mut self, dt: f32, inputs: HashMap<u64, InputFlags>) -> (Vec<(u64, [f32; 3])>, Vec<(ChunkKey, Vec<u8>)>) {
        // 1. Physics
        for (&id, &flags) in inputs.iter() {
            if let Some(player) = self.players.get_mut(&id) {
                let speed = 15.0 * dt;
                if flags.contains(InputFlags::FORWARD) { player.translation.z -= speed; }
                if flags.contains(InputFlags::BACKWARD) { player.translation.z += speed; }
                if flags.contains(InputFlags::LEFT) { player.translation.x -= speed; }
                if flags.contains(InputFlags::RIGHT) { player.translation.x += speed; }
            }
        }

        // 2. Chunk Generation Check
        let mut chunks_to_send = Vec::new();
        for player in self.players.values() {
            let view_dist = 2;
            for x in -view_dist..=view_dist {
                for z in -view_dist..=view_dist {
                    let key = ChunkKey::from_world_pos(player.translation + Vec3::new(x as f32 * 16.0, 0.0, z as f32 * 16.0));
                    if !self.chunks.contains_key(&key) {
                        self.chunks.insert(key, true);
                        let data = generate_chunk_data(&self.perlin, key);
                        chunks_to_send.push((key, data));
                    }
                }
            }
        }

        // 3. State Snapshot
        let states = self.players.values()
            .map(|p| (p.id, p.translation.to_array()))
            .collect();

        (states, chunks_to_send)
    }
}

// Heavy computation moved to a helper function (simulating offloading)
fn generate_chunk_data(perlin: &Perlin, key: ChunkKey) -> Vec<u8> {
    let mut data = Vec::with_capacity(CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE);
    let world_pos = key.0 * CHUNK_SIZE as i32;

    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let wx = (world_pos.x + x as i32) as f64;
                let wz = (world_pos.z + z as i32) as f64;
                
                let height = (perlin.get([wx * 0.05, wz * 0.05]) * 10.0 + 15.0) as i32;
                
                let block = if y < height as usize { 1 } else { 0 };
                data.push(block);
            }
        }
    }
    data
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::SimpleLogger::new().init().unwrap();
    log::info!("Server starting on port {}", PORT);

    // Network Setup
    let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), PORT);
    let socket = UdpSocket::bind(public_addr)?;
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let server_config = ServerConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };
    let mut transport = NetcodeServerTransport::new(server_config, socket)?;
    let mut server = RenetServer::new(ConnectionConfig::default());
    
    // Game State
    let mut world = World::new();
    let mut last_update = Instant::now();
    let mut current_tick: u32 = 0;

    loop {
        let now = Instant::now();
        let dt = now - last_update;
        last_update = now;

        // 1. Network Update
        server.update(dt);
        transport.update(dt, &mut server)?;
        
        // 2. Handle Connection Events
        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    log::info!("Client {} connected", client_id);
                    let translation = Vec3::new(0.0, 20.0, 0.0);
                    world.players.insert(client_id, Player { id: client_id, translation });
                    
                    let msg = ServerMessage::PlayerSpawn { id: client_id, translation: translation.to_array() };
                    let bytes = bincode::serialize(&msg).unwrap();
                    server.broadcast_message(0, bytes);
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    log::info!("Client {} disconnected: {:?}", client_id, reason);
                    world.players.remove(&client_id);
                    let msg = ServerMessage::PlayerDespawn { id: client_id };
                    let bytes = bincode::serialize(&msg).unwrap();
                    server.broadcast_message(0, bytes);
                }
            }
        }

        // 3. Process Inputs & Requests
        let mut inputs = HashMap::new();
        for &client_id in server.clients_id().iter() {
            while let Some(message) = server.receive_message(client_id, 0) {
                if let Ok(msg) = bincode::deserialize::<ClientMessage>(&message) {
                    match msg {
                        ClientMessage::Input { tick: _, flags } => {
                            inputs.insert(client_id, flags);
                        }
                        ClientMessage::RequestChunk { key } => {
                            // In this simple loop, we generate synchronously.
                            // For heavy loads, move this to a channel/spawn_blocking.
                            let data = generate_chunk_data(&world.perlin, key);
                            let msg = ServerMessage::ChunkData { key, data: data.into_boxed_slice() };
                            let bytes = bincode::serialize(&msg).unwrap();
                            server.send_message(client_id, 0, bytes);
                        }
                    }
                }
            }
        }

        // 4. World Update
        current_tick += 1;
        let (states, new_chunks) = world.update(dt.as_secs_f32(), inputs);

        // Send Chunks
        for (key, data) in new_chunks {
            let msg = ServerMessage::ChunkData { key, data: data.into_boxed_slice() };
            let bytes = bincode::serialize(&msg).unwrap();
            server.broadcast_message(0, bytes); // Broadcast to all for caching
        }

        // 5. Broadcast State Snapshot
        let msg = ServerMessage::StateSnapshot { tick: current_tick, states };
        let bytes = bincode::serialize(&msg).unwrap();
        server.broadcast_message(1, bytes); // Channel 1: Unreliable

        // 6. Send Packets
        transport.send_packets(&mut server);
        
        // Sleep for tick rate
        let tick_duration = Duration::from_millis(1000 / 60);
        if dt < tick_duration {
            std::thread::sleep(tick_duration - dt);
        }
    }
}
