//! The note model: a Loro document plus the metadata the list view needs.
//!
//! The Markdown body lives in the `text` container, everything else in the
//! `meta` map. Both are part of the same CRDT, so a title change and a body
//! edit from two devices merge like any other pair of edits. Metadata fields
//! are last-writer-wins per field, which is what a notebook, tag list or
//! trashed flag wants to be.

use loro::{LoroMap, LoroValue};
use notes_proto::{NoteId, NotebookId};
use serde::{Deserialize, Serialize};

use crate::{NoteDocument, NoteError, TextReplacement};

/// Root container that holds the note metadata map.
pub const META_CONTAINER: &str = "meta";

/// A title longer than this is truncated with an ellipsis.
pub const TITLE_MAX_CHARS: usize = 200;

/// A preview longer than this is truncated with an ellipsis.
pub const PREVIEW_MAX_CHARS: usize = 300;

const KEY_NOTEBOOK: &str = "notebook_id";
const KEY_TAGS: &str = "tags";
const KEY_PINNED: &str = "pinned";
const KEY_ARCHIVED: &str = "archived";
const KEY_COLOR: &str = "color";
const KEY_CREATED_AT: &str = "created_at";
const KEY_TRASHED_AT: &str = "trashed_at";

/// Metadata stored in the note's `meta` map.
///
/// Absent fields read as their default, so a note written by an older build
/// still loads. Times are Unix seconds; a trashed note keeps the time it was
/// trashed, which is what the trash view sorts by.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteMeta {
    /// Notebook the note belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook_id: Option<NotebookId>,
    /// Tags, trimmed and deduplicated, in the order the user added them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Pinned notes sort first.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Archived notes leave the main list but stay searchable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    /// Card color token chosen by the user, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Unix seconds when the note was created. Zero when unknown.
    #[serde(default)]
    pub created_at: i64,
    /// Unix seconds when the note was moved to the trash, if it is there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<i64>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl NoteMeta {
    /// Whether the note sits in the trash.
    pub fn is_trashed(&self) -> bool {
        self.trashed_at.is_some()
    }
}

/// A note: stable id, one CRDT document, and the metadata around it.
#[derive(Debug)]
pub struct Note {
    id: NoteId,
    doc: NoteDocument,
}

impl Note {
    /// Create an empty note that records `created_at`.
    pub fn new(id: NoteId, created_at: i64) -> Result<Self, NoteError> {
        let note = Self {
            id,
            doc: NoteDocument::new(),
        };
        note.write_meta_defaults(created_at)?;
        if created_at != 0 {
            note.doc.set_next_commit_timestamp(created_at);
        }
        note.doc.commit();
        Ok(note)
    }

    /// An empty note with no body, metadata or history.
    ///
    /// This is the target for a note another device created: writing the local
    /// metadata defaults first would add operations that never reach the
    /// server, and every later edit on this device would depend on them, so no
    /// other device could apply that edit.
    pub fn empty(id: NoteId) -> Self {
        Self {
            id,
            doc: NoteDocument::new(),
        }
    }

    /// Restore a note from a stored snapshot.
    pub fn from_snapshot(id: NoteId, snapshot: &[u8]) -> Result<Self, NoteError> {
        Ok(Self {
            id,
            doc: NoteDocument::from_snapshot(snapshot)?,
        })
    }

    /// Build a note whose body is `text` and whose metadata is `meta`.
    pub fn from_text(id: NoteId, text: &str, meta: &NoteMeta) -> Result<Self, NoteError> {
        let note = Self::new(id, meta.created_at)?;
        if !text.is_empty() {
            note.doc.insert(0, text)?;
        }
        note.set_meta(meta)?;
        note.doc.commit();
        Ok(note)
    }

    /// This note's stable id.
    pub fn id(&self) -> NoteId {
        self.id
    }

    /// The CRDT document, for the store and the sync engine.
    pub fn document(&self) -> &NoteDocument {
        &self.doc
    }

    /// Use a stable peer id for changes this device makes.
    ///
    /// Loro creates a random peer for every document instance. A store that
    /// reloads a snapshot for each save would therefore make every edit look
    /// like it came from a new device, and the five-minute history grouping
    /// would never merge. Stores call this after loading a snapshot and before
    /// applying an edit. The id must be persisted per device and unique across
    /// devices.
    pub fn set_peer_id(&self, peer: u64) -> Result<(), NoteError> {
        self.doc.set_peer_id(peer)
    }

    /// This replica's peer id.
    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    /// Current Markdown body.
    pub fn text(&self) -> String {
        self.doc.text()
    }

    /// Length of the body in Unicode scalars.
    pub fn char_len(&self) -> usize {
        self.doc.char_len()
    }

    /// Apply a text-box value without committing it.
    pub fn apply_edit<'a>(&self, new_text: &'a str) -> Result<TextReplacement<'a>, NoteError> {
        self.doc.apply_edit(new_text)
    }

    /// Insert at a Unicode scalar position without committing.
    pub fn insert(&self, char_pos: usize, text: &str) -> Result<(), NoteError> {
        self.doc.insert(char_pos, text)
    }

    /// Delete a Unicode scalar range without committing.
    pub fn delete(&self, char_pos: usize, char_len: usize) -> Result<(), NoteError> {
        self.doc.delete(char_pos, char_len)
    }

    /// Replace the body and commit it as one change.
    pub fn replace_text<'a>(&self, new_text: &'a str) -> Result<TextReplacement<'a>, NoteError> {
        let edit = self.doc.apply_edit(new_text)?;
        self.doc.commit();
        Ok(edit)
    }

    /// Group pending operations into one change.
    pub fn commit(&self) {
        self.doc.commit();
    }

    /// Timestamp the next commit explicitly, in Unix seconds.
    pub fn set_next_commit_timestamp(&self, timestamp: i64) {
        self.doc.set_next_commit_timestamp(timestamp);
    }

    /// Commit pending operations with an explicit Unix-seconds timestamp.
    pub fn commit_at(&self, timestamp: i64) {
        self.doc.commit_at(timestamp);
    }

    /// Current metadata.
    pub fn meta(&self) -> NoteMeta {
        let map = self.meta_map();
        NoteMeta {
            notebook_id: read_string(&map, KEY_NOTEBOOK)
                .filter(|value| !value.is_empty())
                .and_then(|value| NotebookId::parse(&value).ok()),
            tags: read_string(&map, KEY_TAGS)
                .and_then(|value| serde_json::from_str(&value).ok())
                .unwrap_or_default(),
            pinned: read_bool(&map, KEY_PINNED).unwrap_or(false),
            archived: read_bool(&map, KEY_ARCHIVED).unwrap_or(false),
            color: read_string(&map, KEY_COLOR).filter(|value| !value.is_empty()),
            created_at: read_i64(&map, KEY_CREATED_AT).unwrap_or(0),
            trashed_at: read_i64(&map, KEY_TRASHED_AT).filter(|value| *value != 0),
        }
    }

    /// Write every metadata field, leaving body history untouched.
    pub fn set_meta(&self, meta: &NoteMeta) -> Result<(), NoteError> {
        let map = self.meta_map();
        map.insert(
            KEY_NOTEBOOK,
            meta.notebook_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        )?;
        map.insert(KEY_TAGS, serde_json::to_string(&meta.tags)?)?;
        map.insert(KEY_PINNED, meta.pinned)?;
        map.insert(KEY_ARCHIVED, meta.archived)?;
        map.insert(KEY_COLOR, meta.color.clone().unwrap_or_default())?;
        map.insert(KEY_CREATED_AT, meta.created_at)?;
        map.insert(KEY_TRASHED_AT, meta.trashed_at.unwrap_or(0))?;
        Ok(())
    }

    /// Replace the tag list, in order, without duplicates.
    pub fn set_tags<I>(&self, tags: I) -> Result<(), NoteError>
    where
        I: IntoIterator<Item = String>,
    {
        let tags = normalize_tags(tags);
        self.meta_map()
            .insert(KEY_TAGS, serde_json::to_string(&tags)?)?;
        Ok(())
    }

    /// Add one tag. Returns whether the list changed.
    pub fn add_tag(&self, tag: &str) -> Result<bool, NoteError> {
        let mut tags = self.meta().tags;
        let tag = tag.trim();
        if tag.is_empty() || tags.iter().any(|existing| existing == tag) {
            return Ok(false);
        }
        tags.push(tag.to_string());
        self.set_tags(tags)?;
        Ok(true)
    }

    /// Remove one tag. Returns whether the list changed.
    pub fn remove_tag(&self, tag: &str) -> Result<bool, NoteError> {
        let tag = tag.trim();
        let tags = self.meta().tags;
        let kept: Vec<String> = tags
            .iter()
            .filter(|existing| existing.as_str() != tag)
            .cloned()
            .collect();
        if kept.len() == tags.len() {
            return Ok(false);
        }
        self.set_tags(kept)?;
        Ok(true)
    }

    /// Move the note into a notebook, or out of every notebook with `None`.
    pub fn set_notebook(&self, notebook_id: Option<NotebookId>) -> Result<(), NoteError> {
        self.set_meta(&NoteMeta {
            notebook_id,
            ..self.meta()
        })
    }

    /// Pin or unpin the note.
    pub fn set_pinned(&self, pinned: bool) -> Result<(), NoteError> {
        self.set_meta(&NoteMeta {
            pinned,
            ..self.meta()
        })
    }

    /// Archive or unarchive the note.
    pub fn set_archived(&self, archived: bool) -> Result<(), NoteError> {
        self.set_meta(&NoteMeta {
            archived,
            ..self.meta()
        })
    }

    /// Set the card color token, or clear it with `None`.
    pub fn set_color(&self, color: Option<&str>) -> Result<(), NoteError> {
        self.set_meta(&NoteMeta {
            color: color.map(str::to_string),
            ..self.meta()
        })
    }

    /// Whether the note sits in the trash.
    pub fn is_trashed(&self) -> bool {
        self.meta().is_trashed()
    }

    /// Move the note to the trash. Returns whether the state changed; a note
    /// already in the trash keeps its original timestamp.
    pub fn trash(&self, at: i64) -> Result<bool, NoteError> {
        if self.is_trashed() {
            return Ok(false);
        }
        self.set_meta(&NoteMeta {
            trashed_at: Some(at),
            ..self.meta()
        })?;
        Ok(true)
    }

    /// Bring the note back from the trash. Returns whether the state changed.
    pub fn restore(&self) -> Result<bool, NoteError> {
        if !self.is_trashed() {
            return Ok(false);
        }
        self.set_meta(&NoteMeta {
            trashed_at: None,
            ..self.meta()
        })?;
        Ok(true)
    }

    /// Title: the first non-empty line, without leading heading markers.
    ///
    /// Markdown images become their alt text, so a pasted screenshot does not
    /// turn its data URL into a card-width title. Empty for an empty note or
    /// a first line that is only an image without alt text; the UI shows a
    /// translated placeholder there.
    pub fn title(&self) -> String {
        let text = self.text();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let without_heading = line.trim_start_matches('#').trim();
            let title = if without_heading.is_empty() {
                line
            } else {
                without_heading
            };
            let title = strip_images(title);
            if title.is_empty() {
                return String::new();
            }
            return truncate_chars(&title, TITLE_MAX_CHARS);
        }
        String::new()
    }

    /// Plain-text preview: everything after the title line, whitespace
    /// collapsed, with the most common Markdown line markers removed.
    pub fn preview(&self) -> String {
        let text = self.text();
        let mut lines = text.lines();
        // Skip blank lines and the line that became the title.
        let mut body = String::new();
        let mut title_seen = false;
        for line in lines.by_ref() {
            let trimmed = line.trim();
            if !title_seen {
                if trimmed.is_empty() {
                    continue;
                }
                title_seen = true;
                continue;
            }
            let plain = strip_line_markers(trimmed);
            let plain = strip_images(plain);
            if plain.is_empty() {
                continue;
            }
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(&plain);
            if body.chars().count() > PREVIEW_MAX_CHARS {
                break;
            }
        }
        truncate_chars(&body, PREVIEW_MAX_CHARS)
    }

    fn meta_map(&self) -> LoroMap {
        self.doc.as_loro().get_map(META_CONTAINER)
    }

    fn write_meta_defaults(&self, created_at: i64) -> Result<(), NoteError> {
        let map = self.meta_map();
        map.insert(KEY_NOTEBOOK, "")?;
        map.insert(KEY_TAGS, "[]")?;
        map.insert(KEY_PINNED, false)?;
        map.insert(KEY_ARCHIVED, false)?;
        map.insert(KEY_COLOR, "")?;
        map.insert(KEY_CREATED_AT, created_at)?;
        map.insert(KEY_TRASHED_AT, 0)?;
        Ok(())
    }
}

/// Trim, drop empties and deduplicate tags, keeping first-seen order.
pub fn normalize_tags<I>(tags: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || out.iter().any(|existing| existing == tag) {
            continue;
        }
        out.push(tag.to_string());
    }
    out
}

fn read_string(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)?.get_deep_value() {
        LoroValue::String(value) => Some(value.as_ref().to_owned()),
        _ => None,
    }
}

fn read_bool(map: &LoroMap, key: &str) -> Option<bool> {
    match map.get(key)?.get_deep_value() {
        LoroValue::Bool(value) => Some(value),
        _ => None,
    }
}

fn read_i64(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)?.get_deep_value() {
        LoroValue::I64(value) => Some(value),
        _ => None,
    }
}

/// Cut `value` to `max` Unicode scalars, adding an ellipsis when cut.
fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}

/// Remove `#`, `>`, `-`, `*`, `+`, `1.` and `- [ ]` markers from one line.
fn strip_line_markers(line: &str) -> &str {
    let line = line.trim();
    if line.starts_with("```") || line == "---" || line == "***" {
        return "";
    }
    if let Some(rest) = line.strip_prefix("- [ ]") {
        return rest.trim();
    }
    if let Some(rest) = line
        .strip_prefix('>')
        .or_else(|| line.strip_prefix('-'))
        .or_else(|| line.strip_prefix('*'))
        .or_else(|| line.strip_prefix('+'))
    {
        return rest.trim();
    }
    let after_digits: &str = {
        let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        &line[digits..]
    };
    if let Some(rest) = after_digits.strip_prefix(". ") {
        return rest.trim();
    }
    line.trim_start_matches('#').trim()
}

/// Plain text for a one-line label, with Markdown images replaced by their
/// alt text and whitespace collapsed.
///
/// Stored summary titles are written by older builds too, so the UI calls
/// this when it renders them. It keeps old image notes from displaying a
/// data URL after the title rule changes.
pub fn plain_line(line: &str) -> String {
    strip_images(line)
}

/// Replace Markdown images with their alt text so a card never shows a long
/// attachment URL. The note body remains the source of truth; this only
/// affects the plain-text preview.
fn strip_images(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    while let Some(offset) = line[cursor..].find("![") {
        let start = cursor + offset;
        let Some(bracket) = line[start + 2..].find("](") else {
            break;
        };
        let bracket = start + 2 + bracket;
        let Some(paren) = line[bracket + 2..].find(')') else {
            break;
        };
        let paren = bracket + 2 + paren;
        out.push_str(&line[cursor..start]);
        let alt = line[start + 2..bracket].trim();
        if !alt.is_empty() {
            out.push_str(alt);
        }
        cursor = paren + 1;
    }
    out.push_str(&line[cursor..]);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{normalize_tags, Note, NoteMeta, PREVIEW_MAX_CHARS, TITLE_MAX_CHARS};
    use notes_proto::{NoteId, NotebookId};

    fn note(text: &str) -> Note {
        Note::from_text(NoteId::new(), text, &NoteMeta::default()).expect("note")
    }

    #[test]
    fn title_is_the_first_non_empty_line_without_heading_markers() {
        assert_eq!(note("").title(), "");
        assert_eq!(note("\n\n  # Hello world  \nbody").title(), "Hello world");
        assert_eq!(note("plain first\nsecond").title(), "plain first");
        assert_eq!(note("###   \nsecond line").title(), "###");
    }

    #[test]
    fn title_is_truncated_with_an_ellipsis() {
        let long = "x".repeat(TITLE_MAX_CHARS + 50);
        let title = note(&long).title();
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS + 1);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn title_replaces_images_with_their_alt_text() {
        let data_url = format!("![image](data:image/png;base64,{})", "A".repeat(500));
        assert_eq!(note(&data_url).title(), "image");
    }

    #[test]
    fn title_of_an_image_without_alt_text_is_empty() {
        let data_url = format!("![](data:image/png;base64,{})", "A".repeat(500));
        assert_eq!(note(&data_url).title(), "");
    }

    #[test]
    fn preview_skips_the_title_and_line_markers() {
        let note = note("# Title\n\n- [ ] buy milk\n> quote here\n1. and more");
        assert_eq!(note.preview(), "buy milk quote here and more");
    }

    #[test]
    fn preview_replaces_images_with_their_alt_text() {
        let note = note(
            "Trip\n\n![coast](attachments/9f2c.webp)\n\
             ![](attachments/empty.webp)\n\nA slow Saturday.",
        );
        assert_eq!(note.preview(), "coast A slow Saturday.");
    }

    #[test]
    fn preview_keeps_text_when_an_image_is_unclosed() {
        let note = note("Title\n\nBefore ![broken");
        assert_eq!(note.preview(), "Before ![broken");
    }

    #[test]
    fn preview_is_capped() {
        let body = "word ".repeat(100);
        let note = note(&format!("Title\n{body}"));
        assert_eq!(note.preview().chars().count(), PREVIEW_MAX_CHARS + 1);
    }

    #[test]
    fn metadata_round_trips_through_a_snapshot() {
        let id = NoteId::new();
        let notebook = NotebookId::new();
        let note = Note::new(id, 1_700_000_000).expect("note");
        note.set_notebook(Some(notebook)).expect("notebook");
        note.set_tags(vec![
            " Work ".to_string(),
            "work".to_string(),
            "".to_string(),
        ])
        .expect("tags");
        note.set_pinned(true).expect("pin");
        note.set_archived(true).expect("archive");
        note.set_color(Some("amber")).expect("color");
        assert!(note.trash(1_700_000_100).expect("trash"));
        note.commit();

        let snapshot = note.document().snapshot().expect("snapshot");
        let restored = Note::from_snapshot(id, &snapshot).expect("restore");
        let meta = restored.meta();
        assert_eq!(meta.notebook_id, Some(notebook));
        assert_eq!(meta.tags, vec!["Work".to_string(), "work".to_string()]);
        assert!(meta.pinned);
        assert!(meta.archived);
        assert_eq!(meta.color.as_deref(), Some("amber"));
        assert_eq!(meta.created_at, 1_700_000_000);
        assert_eq!(meta.trashed_at, Some(1_700_000_100));
    }

    #[test]
    fn trash_and_restore_are_idempotent() {
        let note = note("hello");
        assert!(note.trash(10).expect("trash"));
        assert!(!note.trash(20).expect("trash again"));
        assert_eq!(note.meta().trashed_at, Some(10));
        assert!(note.restore().expect("restore"));
        assert!(!note.restore().expect("restore again"));
        assert!(!note.is_trashed());
    }

    #[test]
    fn tag_helpers_are_case_sensitive_but_trimmed() {
        let note = note("hello");
        assert!(note.add_tag("  ideas ").expect("add"));
        assert!(!note.add_tag("ideas").expect("add again"));
        assert!(note.add_tag("Ideas").expect("add case"));
        assert_eq!(note.meta().tags, vec!["ideas", "Ideas"]);
        assert!(note.remove_tag("ideas").expect("remove"));
        assert!(!note.remove_tag("ideas").expect("remove again"));
        assert_eq!(note.meta().tags, vec!["Ideas"]);
    }

    #[test]
    fn normalize_tags_drops_duplicates_and_blanks() {
        let tags = normalize_tags(vec![
            " a ".to_string(),
            "a".to_string(),
            "  ".to_string(),
            "b".to_string(),
        ]);
        assert_eq!(tags, vec!["a", "b"]);
    }
}
