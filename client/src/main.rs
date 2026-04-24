use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::tasks::futures_lite::future;
use bevy_renet::RenetClient;
use bevy_renet::renet::ConnectionConfig;
use shared::{ChunkKey, ServerMessage, CHUNK_SIZE};
use std::collections::HashMap;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::math::primitives::Cuboid;

// --- Modules ---
mod physics;
mod states;
mod diagnostics;

use physics::{PhysicsBody, PhysicsPlugin, ControlledPlayer};
use states::GameState;
use diagnostics::DiagnosticsPlugin;

// --- Resources ---
#[derive(Resource, Deref, DerefMut, Default)]
struct ChunkCache(HashMap<ChunkKey, Entity>);

#[derive(Resource, Deref, DerefMut, Default)]
struct ChunkDataCache(HashMap<ChunkKey, Vec<u8>>);

// --- Components ---
#[derive(Component)]
struct RemotePlayer {
    id: u64,
    target_pos: Vec3,
}

#[derive(Component)]
struct ChunkMeshingTask(Task<Option<(Mesh, ChunkKey, Vec<u8>)>>);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // 1. Add our Custom Plugins
        .add_plugins(PhysicsPlugin)
        .add_plugins(DiagnosticsPlugin)
        .init_resource::<ChunkCache>()
        .init_resource::<ChunkDataCache>()
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.15)))
        // 2. Add States for flow control
        .init_state::<GameState>()
        // 3. Setup Systems
        .add_systems(Startup, setup_network)
        .add_systems(OnEnter(GameState::InGame), setup_game_world)
        // 4. Update Systems (Only run when InGame)
        .add_systems(Update, network_system.run_if(in_state(GameState::InGame)))
        .add_systems(Update, meshing_task_executor.run_if(in_state(GameState::InGame)))
        .add_systems(Update, interpolation_system.run_if(in_state(GameState::InGame)))
        .run();
}

// --- Setup: Network (Runs immediately) ---
fn setup_network(mut commands: Commands) {
    let config = ConnectionConfig::default();
    commands.insert_resource(RenetClient::new(config));
    
    // For a real game, you would start in Menu state
    // commands.insert_resource(NextState(Some(GameState::Menu)));
    // For this template, we jump straight in:
    commands.insert_resource(NextState::Pending(GameState::InGame));
}

// --- Setup: Game World (Runs when state changes to InGame) ---
fn setup_game_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 40.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((
        DirectionalLight { shadows_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.8, 0.0)),
    ));

    // Spawn Local Player (Client Side Prediction ready)
    commands.spawn((
        ControlledPlayer { id: 0 },
        PhysicsBody { velocity: Vec3::ZERO, on_ground: false },
        Mesh3d(meshes.add(Cuboid::new(0.6, 1.8, 0.6))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.0, 0.0), ..default() })),
        Transform::from_xyz(0.0, 20.0, 0.0),
    ));
}

fn spawn_player_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    translation: [f32; 3],
    color: Color,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(0.6, 1.8, 0.6))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
            Transform::from_translation(Vec3::from(translation)),
        ))
        .id()
}

// --- Network System ---
fn network_system(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut remote_players: Query<(Entity, &mut RemotePlayer)>,
) {
    // Channel 0: Reliable
    while let Some(message) = client.receive_message(0) {
        if let Ok(msg) = bincode::deserialize::<ServerMessage>(&message) {
            match msg {
                ServerMessage::ChunkData { key, data } => {
                    spawn_meshing_task(&mut commands, key, data.into_vec());
                }
                ServerMessage::PlayerSpawn { id, translation } => {
                    let entity = spawn_player_entity(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        translation,
                        Color::srgb(0.0, 0.0, 1.0),
                    );
                    commands.entity(entity).insert(RemotePlayer { id, target_pos: Vec3::from(translation) });
                }
                ServerMessage::PlayerDespawn { id } => {
                    for (entity, player) in remote_players.iter() {
                        if player.id == id {
                            commands.entity(entity).despawn();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Channel 1: Unreliable
    while let Some(message) = client.receive_message(1) {
        if let Ok(msg) = bincode::deserialize::<ServerMessage>(&message) {
            if let ServerMessage::StateSnapshot { states, .. } = msg {
                for (id, pos) in states {
                    for (_, mut player) in remote_players.iter_mut() {
                        if player.id == id {
                            player.target_pos = Vec3::from(pos);
                        }
                    }
                }
            }
        }
    }
}

// --- Meshing Logic ---
fn spawn_meshing_task(commands: &mut Commands, key: ChunkKey, data: Vec<u8>) {
    let thread_pool = AsyncComputeTaskPool::get();
    let data_clone = data.clone();
    let task = thread_pool.spawn(async move {
        let mesh = generate_chunk_mesh(&data_clone, key);
        Some((mesh, key, data_clone))
    });
    commands.spawn(ChunkMeshingTask(task));
}

fn meshing_task_executor(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut ChunkMeshingTask)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_cache: ResMut<ChunkCache>,
    mut data_cache: ResMut<ChunkDataCache>,
) {
    for (entity, mut task) in tasks.iter_mut() {
        if let Some(Some((mesh, key, data))) = future::block_on(future::poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            if let Some(old_entity) = visual_cache.insert(key, Entity::PLACEHOLDER) {
                commands.entity(old_entity).despawn();
            }

            let chunk_entity = commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.7, 0.2), ..default() })),
                    Transform::from_translation(key.world_pos()),
                ))
                .id();

            visual_cache.insert(key, chunk_entity);
            data_cache.insert(key, data);
        }
    }
}

fn generate_chunk_mesh(data: &[u8], _key: ChunkKey) -> Mesh {
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut index_offset = 0;

    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let idx = x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE;
                if data[idx] == 0 { continue; }

                let (fx, fy, fz) = (x as f32, y as f32, z as f32);
                // Naive top-face only for brevity
                vertices.extend_from_slice(&[
                    [fx, fy + 1.0, fz], [fx + 1.0, fy + 1.0, fz],
                    [fx + 1.0, fy + 1.0, fz + 1.0], [fx, fy + 1.0, fz + 1.0],
                ]);
                normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
                indices.extend_from_slice(&[
                    index_offset, index_offset + 1, index_offset + 2,
                    index_offset, index_offset + 2, index_offset + 3
                ]);
                index_offset += 4;
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn interpolation_system(
    mut query: Query<(&mut Transform, &RemotePlayer)>,
    time: Res<Time>,
) {
    for (mut transform, player) in query.iter_mut() {
        transform.translation = transform.translation.lerp(player.target_pos, 10.0 * time.delta_secs());
    }
}
