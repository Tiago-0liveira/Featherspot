# Detail Header Actions & De-duplication Plan

## 1. Problem Statement
When navigating to an Artist, Playlist, or Album detail view in the CLI, the interface suffered from several visual and interaction redundancies:
1. **Redundant Information at the Top**:
   - The top row of the content header displayed the item's title (via `Route::label()`) and the subtitle (artist name, release year, track count).
   - The second row displayed the entity icon and repeated the title.
   - The third row repeated the subtitle verbatim.
   - The track list border below the header repeated the item title for a third time.
2. **Context Actions Cluttering Track Rows**:
   - "Play the album" and "Open in Spotify" were injected as artificial items into `page.sections[0]` under an "Actions" section header.
   - This forced users to scroll past action items before reaching actual music tracks, and prevented immediate `Enter`-to-play on track 1.
3. **Missing Shuffle for Contexts**:
   - There was no dedicated action or button to start an album, artist, or playlist with shuffle enabled.
4. **Keybind Placement**:
   - Keyboard navigation hints were sandwiched in the middle of the header right after the duplicated subtitle, before empty padding lines.

---

## 2. Architecture & Design Plan

### Phase 1: State & Service Model Decoupling
- **Decouple Context Actions from Track List**:
  - Remove `Section { title: "Actions", items: actions }` from `apply_detail` in `service.rs`.
  - Store `uri: Option<String>` and `external_url: Option<String>` directly on `PageState`.
  - `page.sections[0]` now cleanly contains the primary content (Tracks or Songs), followed by secondary sections (e.g., Releases) if available.
  - Track indices in the UI now correspond 1:1 with actual tracks.

### Phase 2: Content Header Visual Layout & De-duplication
- **Header Line-by-Line Structure** (within standard 10-line detail header):
  - **Line 0 (Badge)**: Prominent entity type tag (`Album`, `Playlist`, or `Artist`) in peach bold style (`PEACH`), without repeating the title or subtitle.
  - **Line 1 (Title)**: Type icon (`▣`, `≡`, or `◉`) paired with bold white title (`state.page.title`), bounded to 1 row to prevent visual displacement of action buttons.
  - **Line 2 (Metadata)**: Single, non-duplicated metadata line (`state.page.subtitle`), e.g., `by Radiohead · 1997 · 12 tracks`.
  - **Line 3**: Blank vertical separation line.
  - **Line 4 (Action Buttons)**:
    - Rendered via dedicated single-row `Paragraph` widgets directly to explicit rectangles (`cur_x + width`), ensuring 100% pixel-perfect alignment with `HitMap` targets and avoiding paragraph whitespace trimming anomalies.
    - `[ ▶ Play album ]` (or `[ ▶ Play artist ]` / `[ ▶ Play playlist ]`)
    - `[ ⇄ Play with shuffle ]` (compacts to `[ ⇄ Shuffle ]` on narrower terminals)
    - `[ ↗ Open in Spotify ]` (derived automatically for albums, playlists, and artists via `SpotifyUri::web_url`)
  - **Line 5**: Blank vertical separation line.
  - **Line 6 (Keybinds)**: Moved to the bottom of the header section:
    - `" Enter plays · p play · S shuffle · o spotify · a queues · i inspects · right-click actions "`
- **List Block Border Title**:
  - For detail routes, change the list border from repeating the album/playlist title to `" Tracks "` (or `" Songs "` for artists), providing clear visual hierarchy between the entity header and its track list.

### Phase 3: Interaction & Event Flow
- **Mouse Interaction**:
  - Register hit targets `HitTarget::DetailPlay`, `HitTarget::DetailShuffle`, and `HitTarget::DetailOpenSpotify` spanning the exact screen coordinates of each button.
  - Clicking `DetailPlay` dispatches `Action::PlayDetailContext` (`Effect::PlayTrack { uri, device_id }`).
  - Clicking `DetailShuffle` dispatches `Action::PlayDetailContextWithShuffle` (`Effect::PlayTrack` + `Effect::Shuffle(true)`).
  - Clicking `DetailOpenSpotify` dispatches `Action::OpenDetailSpotify` (`Effect::OpenExternal(url)`).
- **Keyboard Shortcuts**:
  - In detail routes (`Route::Album | Route::Playlist | Route::Artist`), bind:
    - `p` / `P`: Play detail context
    - `S` / Shift+S (matching both `KeyCode::Char('S')` and `KeyCode::Char('s')` with `KeyModifiers::SHIFT`): Play detail context with shuffle
    - `o` / `O`: Open context in Spotify
    - Lowercase `s` without SHIFT continues to toggle global playback shuffle.
  - In the track list, pressing `Enter` on item 0 plays Track 1 immediately via `Effect::PlayContext { uri, position: 0 }`.

---

## 3. Verification Matrix
- **Unit Tests**:
  - `apply_detail_stores_context_metadata_and_omits_action_section`: Confirms `uri` and `external_url` are populated and `page.sections` contains only tracks.
  - `detail_context_actions_dispatch_via_mouse_and_keyboard`: Verifies mouse clicks on hit targets and keyboard keys `p`, `S`, `Shift+s`, lowercase `s`, and `o` dispatch expected effects.
  - `detail_page_header_renders_buttons_and_eliminates_duplicated_info`: Renders ratatui test terminal and asserts exact cell-by-cell hit targets, button strings, tracks header, and layout geometry.
  - `detail_page_long_title_preserves_button_geometry`: Confirms long titles (80+ characters) do not push down button rows or desynchronize mouse coordinates.
  - `artist_detail_header_renders_songs_and_artist_play_labels`: Confirms artist pages render "Songs", "Play artist", and derived Spotify web URL.
- **Linting & Compatibility**:
  - `cargo clippy --workspace` passes cleanly with 0 warnings.
  - Full workspace test suite (`cargo test`) passes across all crates.
