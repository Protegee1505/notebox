//! Keep keyboard focus inside a modal panel.
//!
//! `aria-modal="true"` tells assistive technology that the rest of the page is
//! inert, but browsers still move focus out of a dialog on Tab. Each modal
//! calls this from its keydown handler so Tab and Shift+Tab cycle through its
//! own controls.

use dioxus::prelude::*;

/// Move focus to the next (or previous) focusable control inside `selector`.
///
/// The selector always comes from this crate, and `backwards` is a boolean, so
/// neither is interpolated from user input.
pub fn trap_tab(selector: &str, backwards: bool) {
    let direction = if backwards { "-1" } else { "1" };
    let script = format!(
        r#"(function () {{
  const root = document.querySelector('{selector}');
  if (!root) return;
  const items = Array.from(root.querySelectorAll(
    'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
  )).filter((element) => element.offsetParent !== null);
  if (items.length === 0) return;
  const current = items.indexOf(document.activeElement);
  const next = current === -1 ? 0 : (current + {direction} + items.length) % items.length;
  items[next].focus();
}})();"#
    );
    let _ = document::eval(&script);
}
