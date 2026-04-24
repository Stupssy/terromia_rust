use bevy::prelude::*;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
pub enum GameState {
    #[default]
    Menu,
    Connecting,
    InGame,
}

pub struct StatePlugin;

impl Plugin for StatePlugin {
    fn build(&self, _app: &mut App) {
        // We already init_state in main, but this plugin can hold
        // specific loading screen logic later.
    }
}