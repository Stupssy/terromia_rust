use bitflags::bitflags;
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_ID: u64 = 1001;
pub const DEFAULT_GAME_PORT: u16 = 5000;
pub const DEFAULT_DISCOVERY_PORT: u16 = 5001;
pub const CHUNK_SIZE: usize = 16;
pub const TICK_RATE: u64 = 60;
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const PLAYER_WIDTH: f32 = 0.6;
pub const GRAVITY: f32 = 32.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigData {
    pub bind_ip: String,
    pub game_port: u16,
    pub discovery_port: u16,
    pub server_name: String,
    pub motd: String,
    pub max_clients: usize,
    pub view_distance: i32,
    pub world_seed: u32,
}

impl Default for ServerConfigData {
    fn default() -> Self {
        Self {
            bind_ip: "0.0.0.0".to_string(),
            game_port: DEFAULT_GAME_PORT,
            discovery_port: DEFAULT_DISCOVERY_PORT,
            server_name: "Voxel Rust Server".to_string(),
            motd: "LAN test server".to_string(),
            max_clients: 16,
            view_distance: 3,
            world_seed: 42,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSummary {
    pub server_name: String,
    pub motd: String,
    pub current_players: usize,
    pub max_players: usize,
    pub game_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMessage {
    Probe { protocol_id: u64 },
    Announce(ServerSummary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkKey(pub IVec3);

impl ChunkKey {
    pub fn from_world_pos(pos: Vec3) -> Self {
        let s = CHUNK_SIZE as f32;
        Self(IVec3::new(
            (pos.x / s).floor() as i32,
            (pos.y / s).floor() as i32,
            (pos.z / s).floor() as i32,
        ))
    }

    pub fn world_pos(&self) -> Vec3 {
        self.0.as_vec3() * CHUNK_SIZE as f32
    }
}

pub fn chunk_index(x: usize, y: usize, z: usize) -> usize {
    x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
}

bitflags! {
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct InputFlags: u8 {
        const FORWARD = 0b00001;
        const BACKWARD = 0b00010;
        const LEFT = 0b00100;
        const RIGHT = 0b01000;
        const JUMP = 0b10000;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub name: String,
    pub translation: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Join { player_name: String },
    Input { tick: u32, flags: InputFlags },
    Chat { message: String },
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        client_id: u64,
        server_name: String,
        motd: String,
        spawn_position: [f32; 3],
    },
    PlayerConnected {
        id: u64,
        name: String,
        translation: [f32; 3],
    },
    PlayerDisconnected {
        id: u64,
        reason: Option<String>,
    },
    Chat {
        from: String,
        message: String,
    },
    StateSnapshot {
        tick: u32,
        players: Vec<PlayerSnapshot>,
    },
    ChunkData {
        key: ChunkKey,
        data: Box<[u8]>,
    },
    ServerNotice {
        message: String,
    },
    Disconnect {
        reason: String,
    },
}
