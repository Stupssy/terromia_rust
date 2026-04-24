use crate::CameraState;
use crate::ChunkDataCache;
use crate::LocalPlayer;
use crate::UiState;
use bevy::prelude::*;
use shared::ChunkKey;
use shared::{GRAVITY, PLAYER_HEIGHT, PLAYER_WIDTH, is_solid};

#[derive(Component)]
pub struct PhysicsBody {
    pub velocity: Vec3,
    pub on_ground: bool,
}

impl Default for PhysicsBody {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            on_ground: false,
        }
    }
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, _app: &mut App) {
        // Systems are added in main.rs with proper state conditions
    }
}

pub fn apply_local_prediction(
    keys: Res<ButtonInput<KeyCode>>,
    camera_state: Res<CameraState>,
    ui_state: Res<UiState>,
    mut query: Query<(&mut PhysicsBody, &mut Transform), With<LocalPlayer>>,
) {
    if ui_state.pause_open || ui_state.chat_open {
        return;
    }

    let Ok((mut body, mut _transform)) = query.single_mut() else {
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

    let yaw_rotation = Quat::from_axis_angle(Vec3::Y, camera_state.yaw);
    let wish_dir = yaw_rotation * move_dir;

    let speed = 8.5;
    body.velocity.x = wish_dir.x * speed;
    body.velocity.z = wish_dir.z * speed;

    if keys.just_pressed(KeyCode::Space) && body.on_ground {
        body.velocity.y = 10.0;
        body.on_ground = false;
    }
}

pub fn physics_tick(
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
            if body.velocity.y < 0.0 {
                body.on_ground = true;
            }
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

pub fn is_block_solid(cache: &ChunkDataCache, world_pos: Vec3) -> bool {
    let key = ChunkKey::from_world_pos(world_pos);

    if let Some(chunk_data) = cache.0.get(&key) {
        let local_x = (world_pos.x - key.0.x as f32 * 16.0).floor() as usize;
        let local_y = (world_pos.y - key.0.y as f32 * 16.0).floor() as usize;
        let local_z = (world_pos.z - key.0.z as f32 * 16.0).floor() as usize;

        if local_x < 16 && local_y < 16 && local_z < 16 {
            let idx = local_x + local_y * 16 + local_z * 256;
            let block_id = chunk_data[idx] as u32;
            return is_solid(block_id);
        }
    }
    false
}
