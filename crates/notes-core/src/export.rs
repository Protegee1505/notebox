//! Portable export and import: a zip of plain Markdown plus a JSON manifest.
//!
//! The export is meant to be readable without this app: every note is a `.md`
//! file a person can open, and the manifest carries the metadata the Markdown
//! cannot. Attachments are written next to the notes under their own name.
//! Import accepts archives this build wrote; history is not part of the
//! format, because the sync log and the local snapshots already hold it.

use std::collections::HashSet;
use std::io::{Read, Seek, Write};

use notes_proto::{NoteId, Notebook};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::attachments::{attachment_url, content_hash, ATTACHMENTS_DIR};
use crate::error::ExportError;
use crate::NoteMeta;

/// Marker written into every manifest.
pub const EXPORT_FORMAT: &str = "notebox-export";

/// Export format version written by this build.
pub const EXPORT_VERSION: u16 = 1;

/// Directory inside the archive that holds the Markdown files.
pub const NOTES_DIR: &str = "notes";

/// Manifest path inside the archive.
pub const MANIFEST_PATH: &str = "manifest.json";

/// Refuse archives with more entries than this.
const MAX_ENTRIES: usize = 100_000;

/// Refuse archives whose contents grow past this after decompression.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

/// One note inside a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportNote {
    /// Stable id, preserved on import.
    pub id: NoteId,
    /// Metadata as it was at export time.
    #[serde(default)]
    pub meta: NoteMeta,
    /// Markdown body. The first line is the title.
    pub text: String,
}

/// One attachment inside a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportAttachment {
    /// Note the attachment belongs to, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<NoteId>,
    /// File name inside the `attachments/` directory.
    pub name: String,
    /// MIME type, for example `image/webp`.
    pub mime: String,
    /// Raw bytes. Not part of the JSON manifest; written as its own entry.
    #[serde(skip)]
    pub data: Vec<u8>,
}

/// Everything one export holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportBundle {
    /// Unix seconds when the export was written.
    pub exported_at: i64,
    /// Notebook list.
    pub notebooks: Vec<Notebook>,
    /// Notes, trashed ones included.
    pub notes: Vec<ExportNote>,
    /// Attachment bytes.
    pub attachments: Vec<ExportAttachment>,
}

/// JSON manifest written as `manifest.json`.
#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u16,
    exported_at: i64,
    notebooks: Vec<Notebook>,
    notes: Vec<ManifestNote>,
    attachments: Vec<ManifestAttachment>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestNote {
    id: NoteId,
    file: String,
    meta: NoteMeta,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestAttachment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note_id: Option<NoteId>,
    /// Display name as the user typed it, sanitized only for the entry path.
    #[serde(default)]
    name: String,
    file: String,
    mime: String,
}

/// Write a bundle as a zip.
pub fn write_export_zip<W: Write + Seek>(
    bundle: &ExportBundle,
    writer: W,
) -> Result<(), ExportError> {
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut manifest_notes = Vec::with_capacity(bundle.notes.len());
    let mut manifest_attachments = Vec::with_capacity(bundle.attachments.len());
    let mut written_attachments = HashSet::new();

    for note in &bundle.notes {
        let file = note_file_name(note.id);
        zip.start_file(&file, options)?;
        zip.write_all(note.text.as_bytes())?;
        manifest_notes.push(ManifestNote {
            id: note.id,
            file,
            meta: note.meta.clone(),
        });
    }

    for attachment in &bundle.attachments {
        let name = display_file_name(&attachment.name);
        let file = attachment_file_name(&attachment.mime, &attachment.data);
        // Equal bytes share one entry; the manifest still lists each
        // attachment, so import restores both rows.
        if written_attachments.insert(file.clone()) {
            zip.start_file(&file, options)?;
            zip.write_all(&attachment.data)?;
        }
        manifest_attachments.push(ManifestAttachment {
            note_id: attachment.note_id,
            name,
            file,
            mime: attachment.mime.clone(),
        });
    }

    let manifest = Manifest {
        format: EXPORT_FORMAT.to_string(),
        version: EXPORT_VERSION,
        exported_at: bundle.exported_at,
        notebooks: bundle.notebooks.clone(),
        notes: manifest_notes,
        attachments: manifest_attachments,
    };
    zip.start_file(MANIFEST_PATH, options)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    zip.finish()?;
    Ok(())
}

/// Read a bundle written by [`write_export_zip`].
pub fn read_export_zip<R: Read + Seek>(reader: R) -> Result<ExportBundle, ExportError> {
    let mut archive = ZipArchive::new(reader)?;
    if archive.len() > MAX_ENTRIES {
        return Err(ExportError::TooLarge(format!(
            "{} entries, limit {MAX_ENTRIES}",
            archive.len()
        )));
    }

    check_total_size(&mut archive)?;

    let mut manifest = String::new();
    archive
        .by_name(MANIFEST_PATH)
        .map_err(|_| ExportError::Format(format!("missing {MANIFEST_PATH}")))?
        .read_to_string(&mut manifest)?;
    let manifest: Manifest = serde_json::from_str(&manifest)?;

    if manifest.format != EXPORT_FORMAT {
        return Err(ExportError::Format(manifest.format));
    }
    if manifest.version != EXPORT_VERSION {
        return Err(ExportError::Version(manifest.version));
    }

    let mut notes = Vec::with_capacity(manifest.notes.len());
    for entry in &manifest.notes {
        validate_entry_name(&entry.file)?;
        if entry.file != note_file_name(entry.id) {
            return Err(ExportError::UnsafePath(entry.file.clone()));
        }
        let mut text = String::new();
        archive
            .by_name(&entry.file)
            .map_err(|_| ExportError::MissingEntry(entry.file.clone()))?
            .read_to_string(&mut text)?;
        notes.push(ExportNote {
            id: entry.id,
            meta: entry.meta.clone(),
            text,
        });
    }

    let mut attachments = Vec::with_capacity(manifest.attachments.len());
    for entry in &manifest.attachments {
        validate_entry_name(&entry.file)?;
        if !entry.file.starts_with(&format!("{ATTACHMENTS_DIR}/")) {
            return Err(ExportError::UnsafePath(entry.file.clone()));
        }
        let mut data = Vec::new();
        archive
            .by_name(&entry.file)
            .map_err(|_| ExportError::MissingEntry(entry.file.clone()))?
            .read_to_end(&mut data)?;
        let name = if valid_display_name(&entry.name) {
            entry.name.clone()
        } else {
            entry
                .file
                .rsplit_once('/')
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| entry.file.clone())
        };
        attachments.push(ExportAttachment {
            note_id: entry.note_id,
            name,
            mime: entry.mime.clone(),
            data,
        });
    }

    Ok(ExportBundle {
        exported_at: manifest.exported_at,
        notebooks: manifest.notebooks,
        notes,
        attachments,
    })
}

/// File name a note gets inside the archive.
pub fn note_file_name(id: NoteId) -> String {
    format!("{NOTES_DIR}/{id}.md")
}

/// File name an attachment gets inside the archive.
///
/// Content addressing makes identical bytes share one entry and keeps a user
/// supplied name from colliding with another attachment. The extension comes
/// from the MIME type, so the path is exactly the URL the note uses.
fn attachment_file_name(mime: &str, data: &[u8]) -> String {
    attachment_url(&content_hash(data), mime)
}

/// Reduce a user-supplied path to a plain display name.
fn display_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if base.is_empty() || base == "." || base == ".." {
        "attachment".to_string()
    } else {
        base.to_string()
    }
}

/// Whether a display name from a manifest is a plain file name.
fn valid_display_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

/// Reject absolute paths, parent directory hops and backslashes.
fn validate_entry_name(name: &str) -> Result<(), ExportError> {
    let unsafe_name = name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.contains("..")
        || name.split('/').any(|part| part.is_empty());
    if unsafe_name {
        return Err(ExportError::UnsafePath(name.to_string()));
    }
    Ok(())
}

/// Sum the uncompressed sizes before reading anything into memory.
fn check_total_size<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), ExportError> {
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        total = total.saturating_add(file.size());
        if total > MAX_TOTAL_BYTES {
            return Err(ExportError::TooLarge(format!(
                "more than {MAX_TOTAL_BYTES} bytes uncompressed"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use notes_proto::{NoteId, Notebook, NotebookId};

    use super::{
        read_export_zip, write_export_zip, ExportAttachment, ExportBundle, ExportNote,
        EXPORT_FORMAT,
    };
    use crate::NoteMeta;

    fn bundle() -> ExportBundle {
        ExportBundle {
            exported_at: 1_700_000_000,
            notebooks: vec![Notebook {
                id: NotebookId::new(),
                name: "Work".to_string(),
                created_at: 100,
            }],
            notes: vec![ExportNote {
                id: NoteId::new(),
                meta: NoteMeta {
                    tags: vec!["a".to_string()],
                    pinned: true,
                    created_at: 200,
                    ..NoteMeta::default()
                },
                text: "# Title\n\nbody 😀".to_string(),
            }],
            attachments: vec![ExportAttachment {
                note_id: None,
                name: "photo one.webp".to_string(),
                mime: "image/webp".to_string(),
                data: vec![1, 2, 3, 4],
            }],
        }
    }

    #[test]
    fn round_trips_notes_notebooks_and_attachments() {
        let bundle = bundle();
        let mut bytes = Vec::new();
        write_export_zip(&bundle, Cursor::new(&mut bytes)).expect("write");
        let restored = read_export_zip(Cursor::new(&bytes)).expect("read");
        assert_eq!(restored, bundle);
    }

    #[test]
    fn archive_contains_plain_markdown_and_a_manifest() {
        let bundle = bundle();
        let mut bytes = Vec::new();
        write_export_zip(&bundle, Cursor::new(&mut bytes)).expect("write");
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
        assert!(archive.by_name("manifest.json").is_ok());
        let note = bundle.notes[0].clone();
        let mut text = String::new();
        use std::io::Read;
        archive
            .by_name(&super::note_file_name(note.id))
            .expect("note file")
            .read_to_string(&mut text)
            .expect("read");
        assert_eq!(text, note.text);
        let mut manifest = String::new();
        archive
            .by_name("manifest.json")
            .expect("manifest")
            .read_to_string(&mut manifest)
            .expect("read");
        assert!(manifest.contains(EXPORT_FORMAT));
        assert!(manifest.contains("photo one.webp"));
    }

    #[test]
    fn a_foreign_zip_is_rejected_by_name() {
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
            zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
                .expect("start");
            use std::io::Write;
            zip.write_all(b"hi").expect("write");
            zip.finish().expect("finish");
        }
        let error = read_export_zip(Cursor::new(&bytes)).expect_err("must fail");
        assert!(error.to_string().contains("manifest.json"));
    }

    #[test]
    fn attachments_are_content_addressed_and_do_not_collide() {
        let bundle = ExportBundle {
            attachments: vec![
                ExportAttachment {
                    note_id: None,
                    name: "photo.webp".to_string(),
                    mime: "image/webp".to_string(),
                    data: vec![1, 2, 3],
                },
                ExportAttachment {
                    note_id: None,
                    name: "photo.webp".to_string(),
                    mime: "image/webp".to_string(),
                    data: vec![4, 5, 6],
                },
                ExportAttachment {
                    note_id: None,
                    name: "copy.webp".to_string(),
                    mime: "image/webp".to_string(),
                    data: vec![1, 2, 3],
                },
            ],
            ..ExportBundle::default()
        };
        let mut bytes = Vec::new();
        write_export_zip(&bundle, Cursor::new(&mut bytes)).expect("write");

        let archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
        assert_eq!(archive.len(), 3, "two unique blobs plus the manifest");

        let restored = read_export_zip(Cursor::new(&bytes)).expect("read");
        let data: Vec<_> = restored
            .attachments
            .iter()
            .map(|attachment| attachment.data.clone())
            .collect();
        assert_eq!(data, vec![vec![1, 2, 3], vec![4, 5, 6], vec![1, 2, 3]]);
    }

    #[test]
    fn unsafe_attachment_names_do_not_escape_the_archive() {
        let bundle = ExportBundle {
            attachments: vec![ExportAttachment {
                note_id: None,
                name: "../../evil.sh".to_string(),
                mime: "text/plain".to_string(),
                data: vec![0],
            }],
            ..ExportBundle::default()
        };
        let mut bytes = Vec::new();
        write_export_zip(&bundle, Cursor::new(&mut bytes)).expect("write");
        let restored = read_export_zip(Cursor::new(&bytes)).expect("read");
        assert_eq!(restored.attachments[0].name, "evil.sh");
        assert_eq!(restored.attachments[0].data, vec![0]);
    }
}
