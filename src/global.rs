use std::sync::OnceLock;

static GAME_ID: OnceLock<Option<String>> = OnceLock::new();

pub const FF7R2_GAME_ID: &str = "End";
pub fn get_game_id(default: Option<String>) -> &'static Option<String> {
    GAME_ID.get_or_init(|| { default })
}