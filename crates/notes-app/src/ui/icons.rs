//! Small inline icon set, so the app needs no icon font or network request.
//!
//! Shapes follow the Feather/Lucide 24 px grid and inherit the current text
//! colour. CSS sets the size.

use dioxus::prelude::*;

/// One icon by name.
#[component]
pub fn Icon(name: &'static str) -> Element {
    rsx! {
        svg {
            class: "icon",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.8",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            match name {
                "plus" => rsx! {
                    path { d: "M12 5v14" }
                    path { d: "M5 12h14" }
                },
                "menu" => rsx! {
                    path { d: "M4 6h16" }
                    path { d: "M4 12h16" }
                    path { d: "M4 18h16" }
                },
                "search" => rsx! {
                    circle { cx: "11", cy: "11", r: "7" }
                    path { d: "M21 21l-4.35-4.35" }
                },
                "settings" => rsx! {
                    path { d: "M4 6h9" }
                    path { d: "M17 6h3" }
                    circle { cx: "15", cy: "6", r: "2" }
                    path { d: "M4 12h3" }
                    path { d: "M11 12h9" }
                    circle { cx: "9", cy: "12", r: "2" }
                    path { d: "M4 18h9" }
                    path { d: "M17 18h3" }
                    circle { cx: "15", cy: "18", r: "2" }
                },
                "sun" => rsx! {
                    circle { cx: "12", cy: "12", r: "4" }
                    path { d: "M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" }
                },
                "moon" => rsx! {
                    path { d: "M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" }
                },
                "monitor" => rsx! {
                    rect { x: "2", y: "3", width: "20", height: "14", rx: "2" }
                    path { d: "M8 21h8M12 17v4" }
                },
                "grid" => rsx! {
                    rect { x: "3", y: "3", width: "7", height: "7", rx: "1.5" }
                    rect { x: "14", y: "3", width: "7", height: "7", rx: "1.5" }
                    rect { x: "3", y: "14", width: "7", height: "7", rx: "1.5" }
                    rect { x: "14", y: "14", width: "7", height: "7", rx: "1.5" }
                },
                "list" => rsx! {
                    path { d: "M8 6h13M8 12h13M8 18h13" }
                    path { d: "M3 6h.01M3 12h.01M3 18h.01" }
                },
                "arrow-left" => rsx! {
                    path { d: "M19 12H5" }
                    path { d: "M12 19l-7-7 7-7" }
                },
                "chevron-down" => rsx! {
                    path { d: "M6 9l6 6 6-6" }
                },
                "chevron-left" => rsx! {
                    path { d: "M15 18l-6-6 6-6" }
                },
                "chevron-right" => rsx! {
                    path { d: "M9 18l6-6-6-6" }
                },
                "pin" => rsx! {
                    path { d: "M12 17v5" }
                    path { d: "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z" }
                },
                "archive" => rsx! {
                    path { d: "M21 8v13H3V8" }
                    path { d: "M1 3h22v5H1z" }
                    path { d: "M10 12h4" }
                },
                "trash" => rsx! {
                    path { d: "M3 6h18" }
                    path { d: "M8 6V4h8v2" }
                    path { d: "M19 6l-1 14H6L5 6" }
                    path { d: "M10 11v6M14 11v6" }
                },
                "palette" => rsx! {
                    path { d: "M12 2a10 10 0 1 0 0 20 2 2 0 0 0 2-2v-2a2 2 0 0 1 2-2h2a2 2 0 0 0 2-2 10 10 0 0 0-8-8z" }
                    circle { cx: "13.5", cy: "6.5", r: "1" }
                    circle { cx: "17.5", cy: "10.5", r: "1" }
                    circle { cx: "8.5", cy: "7.5", r: "1" }
                    circle { cx: "6.5", cy: "12.5", r: "1" }
                },
                "bold" => rsx! {
                    path { d: "M6 4h8a4 4 0 0 1 0 8H6z" }
                    path { d: "M6 12h9a4 4 0 0 1 0 8H6z" }
                },
                "italic" => rsx! {
                    path { d: "M19 4h-9M14 20H5M15 4L9 20" }
                },
                "strike" => rsx! {
                    path { d: "M16 4H9a3 3 0 0 0-2.83 4" }
                    path { d: "M14 12a4 4 0 0 1 0 8H6" }
                    path { d: "M4 12h16" }
                },
                "heading" => rsx! {
                    path { d: "M6 4v16M18 4v16M6 12h12" }
                },
                "checklist" => rsx! {
                    path { d: "M9 11l3 3L22 4" }
                    path { d: "M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" }
                },
                "bullet" => rsx! {
                    path { d: "M8 6h13M8 12h13M8 18h13" }
                    circle { cx: "3.5", cy: "6", r: "1" }
                    circle { cx: "3.5", cy: "12", r: "1" }
                    circle { cx: "3.5", cy: "18", r: "1" }
                },
                "numbered" => rsx! {
                    path { d: "M10 6h11M10 12h11M10 18h11" }
                    path { d: "M4 6h1v4M4 10h2M6 18H4c0-1 2-2 2-3s-1-1.5-2-1" }
                },
                "quote" => rsx! {
                    path { d: "M6 17h3l2-4V7H5v6h3z" }
                    path { d: "M14 17h3l2-4V7h-6v6h3z" }
                },
                "code" => rsx! {
                    path { d: "M8 6l-6 6 6 6M16 6l6 6-6 6" }
                },
                "code-block" => rsx! {
                    rect { x: "3", y: "4", width: "18", height: "16", rx: "2" }
                    path { d: "M9 9l-2 3 2 3M15 9l2 3-2 3" }
                },
                "link" => rsx! {
                    path { d: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" }
                    path { d: "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" }
                },
                "eye" => rsx! {
                    path { d: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" }
                    circle { cx: "12", cy: "12", r: "3" }
                },
                "edit" => rsx! {
                    path { d: "M12 20h9" }
                    path { d: "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z" }
                },
                "undo" => rsx! {
                    path { d: "M9 14 4 9l5-5" }
                    path { d: "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11" }
                },
                "redo" => rsx! {
                    path { d: "m15 14 5-5-5-5" }
                    path { d: "M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13" }
                },
                "x" => rsx! {
                    path { d: "M18 6L6 18M6 6l12 12" }
                },
                "tag" => rsx! {
                    path { d: "M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" }
                    circle { cx: "7", cy: "7", r: "1" }
                },
                "book" => rsx! {
                    path { d: "M4 19.5A2.5 2.5 0 0 1 6.5 17H20" }
                    path { d: "M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" }
                },
                "check" => rsx! {
                    path { d: "M20 6L9 17l-5-5" }
                },
                "command" => rsx! {
                    path { d: "M18 3a3 3 0 0 0-3 3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 0 0 3-3z" }
                },
                "download" => rsx! {
                    path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
                    path { d: "M7 10l5 5 5-5M12 15V3" }
                },
                "upload" => rsx! {
                    path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
                    path { d: "M7 8l5-5 5 5M12 3v12" }
                },
                "refresh" => rsx! {
                    path { d: "M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" }
                    path { d: "M3 3v5h5" }
                    path { d: "M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" }
                    path { d: "M16 16h5v5" }
                },
                "logout" => rsx! {
                    path { d: "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" }
                    path { d: "M16 17l5-5-5-5" }
                    path { d: "M21 12H9" }
                },
                "user" => rsx! {
                    circle { cx: "12", cy: "7", r: "4" }
                    path { d: "M5.5 21a6.5 6.5 0 0 1 13 0" }
                },
                "note" => rsx! {
                    path { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" }
                    path { d: "M14 2v6h6M8 13h8M8 17h5" }
                },
                "more" => rsx! {
                    circle { cx: "5", cy: "12", r: "1" }
                    circle { cx: "12", cy: "12", r: "1" }
                    circle { cx: "19", cy: "12", r: "1" }
                },
                "clock" => rsx! {
                    circle { cx: "12", cy: "12", r: "9" }
                    path { d: "M12 7v5l3 2" }
                },
                "database" => rsx! {
                    ellipse { cx: "12", cy: "5", rx: "8", ry: "3" }
                    path { d: "M4 5v6c0 1.66 3.58 3 8 3s8-1.34 8-3V5" }
                    path { d: "M4 11v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6" }
                },
                "image" => rsx! {
                    rect { x: "3", y: "4", width: "18", height: "16", rx: "2" }
                    circle { cx: "9", cy: "10", r: "2" }
                    path { d: "M21 15l-4.5-4.5L7 20" }
                },
                "sparkle" => rsx! {
                    path { d: "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" }
                    path { d: "M19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z" }
                },
                _ => rsx! {
                    circle { cx: "12", cy: "12", r: "9" }
                },
            }
        }
    }
}
