use bitflags::bitflags;
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};

// --- World ---
pub const PROTOCOL_ID: u64 = 1001;
pub const PORT: u16 = 5000;
pub const CHUNK_SIZE: usize = 16;
pub const TICK_RATE: u64 = 60;

// --- Player ---
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const PLAYER_WIDTH: f32 = 0.6;
pub const GRAVITY: f32 = 32.0;
pub const JUMP_FORCE: f32 = 9.0;

// --- Key for identifying chunks in space ---
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

// --- Input Handling (Bitflags are bandwidth efficient) ---
bitflags! {
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
    pub struct InputFlags: u8 {
        const FORWARD = 0b0001;
        const BACKWARD = 0b0010;
        const LEFT = 0b0100;
        const RIGHT = 0b1000;
    }
}

// --- Network Protocol ---
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Input { tick: u32, flags: InputFlags },
    RequestChunk { key: ChunkKey },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMessage {
    ChunkData { key: ChunkKey, data: Box<[u8]> },
    PlayerSpawn { id: u64, translation: [f32; 3] },
    PlayerDespawn { id: u64 },
    StateSnapshot { tick: u32, states: Vec<(u64, [f32; 3])> },
}