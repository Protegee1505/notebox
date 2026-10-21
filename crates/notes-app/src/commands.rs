//! Command palette entries and their actions.

use dioxus::prelude::*;
use notes_store::NoteView;

use crate::state::{AppContext, Screen, Theme};

/// One action the palette and shortcuts can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Create an empty note.
    NewNote,
    /// Write the open note to disk.
    Save,
    /// Open find-in-note.
    Find,
    /// Switch cards and list.
    ToggleLayout,
    /// Show the rendered note.
    TogglePreview,
    /// Show the open note's version timeline.
    History,
    /// Follow the system theme.
    ThemeSystem,
    /// Light theme.
    ThemeLight,
    /// Dark theme.
    ThemeDark,
    /// Show active notes.
    ViewNotes,
    /// Show archived notes.
    ViewArchive,
    /// Show the trash.
    ViewTrash,
    /// Open preferences.
    Settings,
    /// Open the account form.
    SignIn,
    /// Show all keyboard shortcuts.
    Shortcuts,
    /// Download a zip of every note.
    Export,
    /// Pick a zip written by Export and import it.
    Restore,
    /// Open the trash and its empty action.
    EmptyTrash,
}

impl Command {
    /// Every command, in palette order.
    pub const ALL: [Command; 18] = [
        Command::NewNote,
        Command::Save,
        Command::Find,
        Command::TogglePreview,
        Command::History,
        Command::ToggleLayout,
        Command::ViewNotes,
        Command::ViewArchive,
        Command::ViewTrash,
        Command::Settings,
        Command::SignIn,
        Command::Export,
        Command::Restore,
        Command::Shortcuts,
        Command::ThemeSystem,
        Command::ThemeLight,
        Command::ThemeDark,
        Command::EmptyTrash,
    ];

    /// Text shown in the palette.
    pub fn label(self) -> &'static str {
        match self {
            Command::NewNote => "New note",
            Command::Save => "Save now",
            Command::Find => "Find in note",
            Command::ToggleLayout => "Switch cards and list",
            Command::TogglePreview => "Show rendered note",
            Command::History => "Note history",
            Command::ThemeSystem => "Theme: follow system",
            Command::ThemeLight => "Theme: light",
            Command::ThemeDark => "Theme: dark",
            Command::ViewNotes => "Go to notes",
            Command::ViewArchive => "Go to archive",
            Command::ViewTrash => "Go to trash",
            Command::Settings => "Open settings",
            Command::SignIn => "Sign in to sync",
            Command::Shortcuts => "Keyboard shortcuts",
            Command::Export => "Export all notes",
            Command::Restore => "Restore from backup",
            Command::EmptyTrash => "Empty trash",
        }
    }

    /// A short hint shown at the right of the row.
    pub fn hint(self) -> &'static str {
        match self {
            Command::NewNote => "Ctrl N",
            Command::Save => "Ctrl S",
            Command::Find => "Ctrl F",
            Command::ToggleLayout => "",
            Command::TogglePreview => "Ctrl E",
            Command::History => "",
            Command::ThemeSystem => "",
            Command::ThemeLight => "",
            Command::ThemeDark => "",
            Command::ViewNotes => "",
            Command::ViewArchive => "",
            Command::ViewTrash => "",
            Command::Settings => "",
            Command::SignIn => "",
            Command::Shortcuts => "?",
            Command::Export => "",
            Command::Restore => "",
            Command::EmptyTrash => "",
        }
    }

    /// Icon name for the row.
    pub fn icon(self) -> &'static str {
        match self {
            Command::NewNote => "plus",
            Command::Save => "check",
            Command::Find => "search",
            Command::ToggleLayout => "grid",
            Command::TogglePreview => "eye",
            Command::History => "clock",
            Command::ThemeSystem => "monitor",
            Command::ThemeLight => "sun",
            Command::ThemeDark => "moon",
            Command::ViewNotes => "note",
            Command::ViewArchive => "archive",
            Command::ViewTrash => "trash",
            Command::Settings => "settings",
            Command::SignIn => "user",
            Command::Shortcuts => "command",
            Command::Export => "download",
            Command::Restore => "upload",
            Command::EmptyTrash => "trash",
        }
    }

    /// Extra words the palette should match.
    pub fn keywords(self) -> &'static str {
        match self {
            Command::NewNote => "create add",
            Command::Save => "write persist",
            Command::Find => "search in note",
            Command::ToggleLayout => "grid cards rows",
            Command::TogglePreview => "render markdown read",
            Command::History => "versions timeline diff restore past",
            Command::ThemeSystem => "appearance follow system",
            Command::ThemeLight => "appearance light",
            Command::ThemeDark => "appearance dark",
            Command::ViewNotes => "all active",
            Command::ViewArchive => "archived",
            Command::ViewTrash => "deleted",
            Command::Settings => "preferences account notebooks storage",
            Command::SignIn => "login account sync",
            Command::Shortcuts => "keys help",
            Command::Export => "backup zip download",
            Command::Restore => "backup zip import undo recovery",
            Command::EmptyTrash => "delete forever",
        }
    }

    /// Run the command against the app.
    pub fn run(self, context: &AppContext) {
        context.close_palette();
        match self {
            Command::NewNote => context.new_note(),
            Command::Save => {
                context.save_now();
                context.toast_success("Saved");
            }
            Command::Find => {
                if !context.state.peek().find_open {
                    context.toggle_find();
                }
            }
            Command::ToggleLayout => context.toggle_layout(),
            Command::TogglePreview => context.toggle_preview(),
            Command::History => context.open_history(),
            Command::ThemeSystem => context.set_theme(Theme::System),
            Command::ThemeLight => context.set_theme(Theme::Light),
            Command::ThemeDark => context.set_theme(Theme::Dark),
            Command::ViewNotes => context.set_view(NoteView::Active),
            Command::ViewArchive => context.set_view(NoteView::Archived),
            Command::ViewTrash => context.set_view(NoteView::Trash),
            Command::Settings => context.open_settings(),
            Command::SignIn => context.go(Screen::Login),
            Command::Shortcuts => context.show_shortcuts(),
            Command::Export => context.export_notes(),
            Command::Restore => {
                context.open_settings();
                let _ = document::eval(
                    "setTimeout(() => document.getElementById('restore-backup-input')?.click(), 80);",
                );
            }
            Command::EmptyTrash => {
                context.set_view(NoteView::Trash);
                if context.state.peek().notes.is_empty() {
                    context.toast_info("The trash is empty");
                } else {
                    context.state.write_unchecked().confirm_empty_trash = true;
                    context.toast_info("Confirm emptying the trash");
                }
            }
        }
    }
}

/// Commands matching what the user typed, in palette order.
pub fn filtered(query: &str) -> Vec<Command> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Command::ALL.to_vec();
    }
    Command::ALL
        .into_iter()
        .filter(|command| {
            command.label().to_lowercase().contains(&query)
                || command.keywords().to_lowercase().contains(&query)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{filtered, Command};

    #[test]
    fn an_empty_query_lists_every_command() {
        assert_eq!(filtered("").len(), Command::ALL.len());
    }

    #[test]
    fn keywords_find_a_command_by_what_it_does() {
        let found = filtered("backup");
        assert!(found.contains(&Command::Export));
        assert!(found.contains(&Command::Restore));
        let found = filtered("restore");
        assert!(found.contains(&Command::Restore));
        let found = filtered("dark");
        assert_eq!(found, vec![Command::ThemeDark]);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(filtered("NEW NOTE"), vec![Command::NewNote]);
    }
}
