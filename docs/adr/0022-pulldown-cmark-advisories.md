# ADR-0022: Patch pulldown-cmark for wikilink advisories

- **Status:** accepted
- **Date:** 2026-11-04
- **Phase:** P8 dependency security
- **Owner:** app + core

## Question

How should the workspace respond when `cargo deny check` fails on the locked
`pulldown-cmark` 0.13.4 while preparing the note-editor layout fix for release?

## Context

RustSec RUSTSEC-2026-0414 and RUSTSEC-2026-0433 describe exponential event
growth and quadratic parsing, respectively, when `ENABLE_WIKILINKS` is used.
Versions 0.13.5 and 0.13.6 fix the two issues. The app's Markdown rendering,
attachment scanning and markup highlighting all start with `Options::empty()`
and enable tables, strikethrough, task lists and footnotes only: the affected
wikilink option is **not** enabled here. The security gate nonetheless checks
all locked dependencies and blocks a deploy when they have active advisories.

The published 0.13.6 registry entry has the same normal dependency list and
Rust minimum version (1.71.1) as 0.13.4. The project requires Rust 1.88 and
pins Dioxus separately at 0.7.10.

## Decision

Raise the workspace requirement and lock `pulldown-cmark` to the 0.13.6 patch.
Use the published crates.io-index checksum in `Cargo.lock`; leave all other
package versions and feature flags unchanged. Do not ignore these advisories in
`deny.toml`. No Markdown parser options, storage format, API or sync path change.

A toolchain-equipped checkout can reproduce the lockfile change with
`cargo update -p pulldown-cmark --precise 0.13.6`; this sandbox cannot run Cargo.

## Evidence and release gate

- The RustSec advisories identify 0.13.6 as patched for both issues.
- The crates.io index records the 0.13.6 checksum as
  `3db2be46835f013dd3f4fa7f5f7402c537787b54ebb6fe2b94bbdd7e2669bb7b`.
- The PR's previous CI run passed the web build (including the new editor
  width test), Rust formatting, Clippy and tests, but failed `cargo deny check`.
- The updated lockfile must pass the full CI gate, including `cargo deny`,
  before this PR can be merged or the guarded deployment script can run.

## Consequences

This is a compatible patch to the shared Markdown parser. It closes the
advisories even though the affected wikilink feature was off, and prevents a
future lockfile refresh from reintroducing 0.13.4. There is no change to
Worker requests, D1 migrations, authentication or Cloudflare costs.
