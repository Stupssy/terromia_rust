use bevy::prelude::*;
use bevy_renet::RenetClient;
use shared::{GRAVITY, PLAYER_HEIGHT, PLAYER_WIDTH, InputFlags, ClientMessage};
use crate::ChunkDataCache;
use crate::states::GameState;
use shared::ChunkKey;

#[derive(Component)]
pub struct PhysicsBody {
    pub velocity: Vec3,
    pub on_ground: bool,
}

#[derive(Component)]
pub struct ControlledPlayer {
    pub id: u64,
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, input_to_velocity.run_if(in_state(GameState::InGame)))
            .add_systems(Update, physics_tick.run_if(in_state(GameState::InGame)));
    }
}

fn input_to_velocity(
    keys: Res<ButtonInput<KeyCode>>,
    mut client: ResMut<RenetClient>,
    mut query: Query<(&mut PhysicsBody, &mut Transform), With<ControlledPlayer>>,
) {
    let mut flags = InputFlags::empty();
    let mut move_dir = Vec3::ZERO;

    if keys.pressed(KeyCode::KeyW) { flags |= InputFlags::FORWARD; move_dir.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { flags |= InputFlags::BACKWARD; move_dir.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { flags |= InputFlags::LEFT; move_dir.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { flags |= InputFlags::RIGHT; move_dir.x += 1.0; }

    // Send to Server
    let msg = ClientMessage::Input { tick: 0, flags };
    let bytes = bincode::serialize(&msg).unwrap();
    client.send_message(0, bytes);

    // Apply Local Prediction
    if let Ok((mut body, mut _transform)) = query.single_mut() {
        // Simple speed
        let speed = 10.0;
        body.velocity.x = move_dir.x * speed;
        body.velocity.z = move_dir.z * speed;

        // Jump
        if keys.just_pressed(KeyCode::Space) && body.on_ground {
            body.velocity.y = 8.0;
            body.on_ground = false;
        }
    }
}

fn physics_tick(
    time: Res<Time>,
    data_cache: Res<ChunkDataCache>,
    mut query: Query<(&mut Transform, &mut PhysicsBody)>,
) {
    for (mut transform, mut body) in query.iter_mut() {
        let dt = time.delta_secs();
        
        // Gravity
        body.velocity.y -= GRAVITY * dt;

        // --- Y Axis ---
        let move_y = Vec3::new(0.0, body.velocity.y * dt, 0.0);
        if !check_collision(&data_cache, transform.translation + move_y) {
            transform.translation += move_y;
            body.on_ground = false;
        } else {
            if body.velocity.y < 0.0 { body.on_ground = true; }
            body.velocity.y = 0.0;
        }

        // --- X Axis ---
        let move_x = Vec3::new(body.velocity.x * dt, 0.0, 0.0);
        if !check_collision(&data_cache, transform.translation + move_x) {
            transform.translation += move_x;
        } else {
            body.velocity.x = 0.0;
        }

        // --- Z Axis ---
        let move_z = Vec3::new(0.0, 0.0, body.velocity.z * dt);
        if !check_collision(&data_cache, transform.translation + move_z) {
            transform.translation += move_z;
        } else {
            body.velocity.z = 0.0;
        }
    }
}

fn check_collision(cache: &ChunkDataCache, pos: Vec3) -> bool {
    let half_w = PLAYER_WIDTH / 2.0;
    let h = PLAYER_HEIGHT;
    
    // Check feet corners and head corners
    let corners = [
        [pos.x - half_w, pos.y, pos.z - half_w],
        [pos.x + half_w, pos.y, pos.z - half_w],
        [pos.x - half_w, pos.y, pos.z + half_w],
        [pos.x + half_w, pos.y, pos.z + half_w],
        [pos.x - half_w, pos.y + h, pos.z - half_w],
        [pos.x + half_w, pos.y + h, pos.z - half_w],
        [pos.x - half_w, pos.y + h, pos.z + half_w],
        [pos.x + half_w, pos.y + h, pos.z + half_w],
    ];

    for c in corners {
        if is_block_solid(cache, Vec3::from(c)) {
            return true;
        }
    }
    false
}

fn is_block_solid(cache: &ChunkDataCache, world_pos: Vec3) -> bool {
    let key = ChunkKey::from_world_pos(world_pos);
    
    if let Some(chunk_data) = cache.get(&key) {
        let local_x = (world_pos.x - key.0.x as f32 * 16.0).floor() as usize;
        let local_y = (world_pos.y - key.0.y as f32 * 16.0).floor() as usize;
        let local_z = (world_pos.z - key.0.z as f32 * 16.0).floor() as usize;

        if local_x < 16 && local_y < 16 && local_z < 16 {
            let idx = local_x + local_y * 16 + local_z * 256;
            return chunk_data[idx] != 0;
        }
    }
    false
}
