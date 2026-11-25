//! What several pages share, aware of the app's domain (the `ui` layer is not).

mod hero_item;
mod item_menu;
mod media_grid;
mod media_info;
mod media_row;
mod music_player;
mod page_header;
mod play_choice;
mod settings;

pub use hero_item::{hero_item, HeroKind};
pub use item_menu::{item_menu_host, open_item_menu};
pub use media_info::{cast_row, media_info};
pub use media_grid::media_grid;
pub use media_row::{media_row, row_skeletons};
pub use music_player::{music_host, play_buttons};
pub use page_header::page_header;
pub use play_choice::{cast_part, can_cast, open_cast_sheet, play_choice_host, play_with_choice};
pub use settings::{open_settings, settings_host};
