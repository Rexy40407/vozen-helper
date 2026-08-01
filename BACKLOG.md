# Vozen Helper Backlog

## Approved initiatives

### 1. Levels & Rank Card Studio

- **Status:** implemented — verification passed
- **Source:** `docs/feature-scout-2026-07-31.md` and owner approval on 2026-07-31
- **Priority:** P2
- **Effort:** M
- **Dependencies:** existing XP, level, `/rank` and leaderboard data; local panel

Build a beginner-friendly visual editor for the server's rank card, inspired by
the customization flow observed in MEE6. The panel must show a live preview and
the generated card must be used by `/rank`.

#### MVP scope

- Live preview containing avatar, username, rank, level, current XP, target XP and progress bar.
- Curated font selector.
- Main/accent colour, text colour and progress-bar colour controls.
- Background colour and overlay-opacity controls.
- Preset background gallery with 10 curated banners.
- Solid-colour mode with a safe server palette; member-provided uploads and URLs are not supported.
- Avatar ring/frame styling.
- Contrast and legibility validation.
- Restore defaults, save draft and publish actions.
- One server-wide default template.
- Rendered image output for `/rank`.

#### Explicitly deferred

- Per-user card templates.
- Animated backgrounds.
- Economy- or premium-unlocked cosmetics.
- Achievement/badge inventory and seasonal frames.
- Multiple layout families.

#### Definition of done

1. A server administrator can open the studio and understand the current template without documentation.
2. Every supported change is reflected in the preview before publishing.
3. Invalid, oversized or unreadable images are rejected with an actionable explanation.
4. Publishing updates the server template atomically and preserves the last valid version on failure.
5. `/rank` renders the published template with real member XP, level and ranking data.
6. Resetting to defaults and previewing a draft never changes the live server template.
7. The flow is covered by focused panel, API and rendering tests before release.

#### Verification

- `npm.cmd run build` in `panel/` passed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` passed.
- `cargo test --workspace --locked --offline` passed.

## Pending prioritization

The remaining Feature Scout recommendations are not approved by this entry and
must be separately prioritized before implementation.
