# ADR-0019: Restore the last workspace and confirm trashing

- **Status:** accepted
- **Date:** 2026-10-22
- **Phase:** P8 polish
- **Owner:** app

## Question

Should reopening Notebox return to where the owner left off, and should a
single trash click destroy the note's place in the active list without a
second step?

## Context

Preferences already persist the theme, layout and offline choice in a small
local JSON file (native) or `localStorage` (web). They are device choices, not
synced note data, so adding session state does not touch the sync protocol or
the D1 schema.

The app previously selected the first note and always opened Notes, even when
the owner had been reading another note or adjusting Settings. The trash action
also moved the open note immediately; the safety net was the Trash view, but
the list position and editor context changed before the owner could undo the
action.

## Decision

1. Persist the open note id, Notes/Settings screen, Active/Archived/Trash view,
   phone list-or-editor pane, and wide-gallery flag in the existing local
   preferences.
2. Restore those values after storage opens. A missing or unreadable note falls
   back to the first note in the restored view. Login is never restored; the
   first-run flow always wins. Search text and notebook/tag filters stay out of
   the saved session.
3. Add a **Confirm before deleting** preference, default **on**, that guards
   moving one note to Trash. The confirmation is an inline row under the editor
   header with Cancel focused; Escape and Cancel both dismiss it. Empty Trash
   keeps its own separate confirmation.

## Evidence

- `prefs` round-trip test covers all new fields and a legacy JSON document; the
  legacy document loads with the new defaults.
- `cargo nextest` covers `next_theme`; `web-persist.mjs` (22/22) checks the last
  note and Settings screen restoration, the one-click theme and that switching
  delete confirmation off skips the prompt. `web-a11y.mjs` (73/73) checks the
  inline confirmation, Cancel focus/return, the static avatar and the setting
  toggle's pressed state.
- The free-tier model is unchanged by these preferences: they never reach the
  Worker.

## Consequences

The preferences file grows with optional fields, so older builds ignore them
and newer builds fall back to safe defaults. A saved note that was permanently
erased or belongs to a removed account simply does not reopen.
