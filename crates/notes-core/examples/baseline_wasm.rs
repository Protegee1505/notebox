//! Size baseline: the same shape of work without the note document or Loro.
//!
//! Build beside `loro_wasm` and subtract the compressed sizes to estimate the
//! CRDT cost.

fn main() {
    let mut text = String::from("Notebox");
    text.push_str(" note");
    std::hint::black_box(&text);
}
