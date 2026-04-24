use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockKey {
    Air,
    Grass,
    Dirt,
    Stone,
    Wood,
    Leaves,
    Brick,
    Water,
    Sand,
    Gravel,
    Glass,
    LogOak,
    PlanksOak,
    Snow,
    Ice,
    OreCoal,
    OreIron,
    OreGold,
    OreDiamond,
    Sandstone,
    Clay,
    Cactus,
    Obsidian,
    Bedrock,
    SchematicBlock,
    Mud,
    Granite,
    Marble,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlockDefinition {
    pub id: u32,
    pub key: BlockKey,
    pub name: &'static str,
    pub color: Option<u32>,
    pub solid: bool,
    pub transparent: bool,
    pub opaque: bool,
    pub alpha: f32,
    pub breakable: bool,
    pub is_entity: bool,
}

pub const BLOCK_DEFINITIONS: &[BlockDefinition] = &[
    BlockDefinition { id: 0, key: BlockKey::Air, name: "Air", color: None, solid: false, transparent: true, opaque: false, alpha: 0.0, breakable: false, is_entity: false },
    BlockDefinition { id: 1, key: BlockKey::Grass, name: "Grass Block", color: Some(0x567d46), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 2, key: BlockKey::Dirt, name: "Dirt", color: Some(0x3b291d), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 3, key: BlockKey::Stone, name: "Stone", color: Some(0x7a7a7a), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 4, key: BlockKey::Wood, name: "Wood Planks", color: Some(0x5c4033), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 5, key: BlockKey::Leaves, name: "Leaves", color: Some(0x2d5a27), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 6, key: BlockKey::Brick, name: "Bricks", color: Some(0x8f4836), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 7, key: BlockKey::Water, name: "Water", color: Some(0x40a4df), solid: false, transparent: true, opaque: false, alpha: 0.6, breakable: false, is_entity: false },
    BlockDefinition { id: 8, key: BlockKey::Sand, name: "Sand", color: Some(0xd6c28d), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 9, key: BlockKey::Gravel, name: "Gravel", color: Some(0x85817e), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 10, key: BlockKey::Glass, name: "Glass", color: Some(0xc1f2fa), solid: true, transparent: true, opaque: false, alpha: 0.3, breakable: true, is_entity: false },
    BlockDefinition { id: 11, key: BlockKey::LogOak, name: "Oak Log", color: Some(0x3b271d), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 12, key: BlockKey::PlanksOak, name: "Oak Planks", color: Some(0xa2824e), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 13, key: BlockKey::Snow, name: "Snow", color: Some(0xfcfcfc), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 14, key: BlockKey::Ice, name: "Ice", color: Some(0xaaddf2), solid: true, transparent: true, opaque: false, alpha: 0.4, breakable: true, is_entity: false },
    BlockDefinition { id: 15, key: BlockKey::OreCoal, name: "Coal Ore", color: Some(0x363636), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 16, key: BlockKey::OreIron, name: "Iron Ore", color: Some(0x8c7c72), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 17, key: BlockKey::OreGold, name: "Gold Ore", color: Some(0xe6bd34), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 18, key: BlockKey::OreDiamond, name: "Diamond Ore", color: Some(0x4eedd8), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 19, key: BlockKey::Sandstone, name: "Sandstone", color: Some(0xd4c28f), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 20, key: BlockKey::Clay, name: "Clay", color: Some(0x9aa2b3), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 21, key: BlockKey::Cactus, name: "Cactus", color: Some(0x2f6e24), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 22, key: BlockKey::Obsidian, name: "Obsidian", color: Some(0x14101e), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 23, key: BlockKey::Bedrock, name: "Bedrock", color: Some(0x222222), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: false, is_entity: false },
    BlockDefinition { id: 24, key: BlockKey::SchematicBlock, name: "Schematic Block", color: Some(0xaa00ff), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: true },
    BlockDefinition { id: 25, key: BlockKey::Mud, name: "Mud", color: Some(0x5c4033), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 26, key: BlockKey::Granite, name: "Granite", color: Some(0x8b8b8b), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
    BlockDefinition { id: 27, key: BlockKey::Marble, name: "Marble", color: Some(0xf5f5f5), solid: true, transparent: false, opaque: true, alpha: 1.0, breakable: true, is_entity: false },
];

pub fn blocks() -> &'static HashMap<BlockKey, u32> {
    use once_cell::sync::Lazy;
    static BLOCKS: Lazy<HashMap<BlockKey, u32>> = Lazy::new(|| {
        BLOCK_DEFINITIONS.iter().map(|def| (def.key, def.id)).collect()
    });
    &BLOCKS
}

pub fn solid_map() -> &'static HashMap<u32, bool> {
    use once_cell::sync::Lazy;
    static SOLID_MAP: Lazy<HashMap<u32, bool>> = Lazy::new(|| {
        BLOCK_DEFINITIONS.iter().map(|def| (def.id, def.solid)).collect()
    });
    &SOLID_MAP
}

pub fn is_solid(id: u32) -> bool {
    solid_map().get(&id).copied().unwrap_or(false)
}

pub fn get_block_by_id(id: u32) -> Option<&'static BlockDefinition> {
    BLOCK_DEFINITIONS.iter().find(|def| def.id == id)
}

pub fn get_block_by_key(key: BlockKey) -> Option<&'static BlockDefinition> {
    BLOCK_DEFINITIONS.iter().find(|def| def.key == key)
}
