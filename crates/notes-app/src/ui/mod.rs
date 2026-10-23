//! Screens and reusable pieces of the shell.

pub mod context_menu;
pub mod editor;
pub mod focus;
pub mod gallery;
pub mod history;
pub mod icons;
pub mod login;
pub mod note_list;
pub mod palette;
pub mod settings;
pub mod shortcuts;
pub mod sidebar;
pub mod toast;
pub mod topbar;
pub mod update;

pub use context_menu::NoteContextMenu;
pub use editor::EditorPane;
pub use gallery::AttachmentGallery;
pub use history::HistoryPanel;
pub use login::LoginScreen;
pub use note_list::NoteList;
pub use palette::CommandPalette;
pub use settings::SettingsPane;
pub use shortcuts::ShortcutSheet;
pub use sidebar::Sidebar;
pub use toast::ToastView;
pub use topbar::TopBar;
pub use update::UpdateBanner;
