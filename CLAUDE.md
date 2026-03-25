# Testing Rule

Before considering any task done, run `cargo test` and make sure all tests pass. Do not skip this step.

# New Feature Testing Rule

When implementing a new feature that mutates `App` state, write at least one test using `App::new_headless()`. This applies to any method that adds, removes, or modifies App struct fields (objects, waveforms, audio_clips, regions, components, etc.). Does NOT apply to pure UI/rendering features. Tests go in `src/tests/` as new files or added to existing ones. At minimum: one happy-path test proving the feature works.

# Changelog Rule

After completing each task, prepend a short entry to the top of `CHANGELOG.md` in the project root describing what was done. Each entry should include today's date and a brief description. Format: `- YYYY-MM-DD: description`. Keep it really simple, one line max. Also add codebase length to the end, example (14.3k loc). Do not type date every time — only when it changed and it's another day.

To count lines of code, run: `find src -name '*.rs' | xargs wc -l | tail -1`

<!-- GSD:project-start source:PROJECT.md -->
## Project

**Layers — Browser Panel Enhancements**

A Rust-based DAW (Digital Audio Workstation) with native macOS support, WGPU rendering, and VST3 plugin integration. This milestone focuses on making the Layers panel in the browser more interactive: entity colors, drag reordering, per-clip VST effects, and double-click to open plugin GUIs.

**Core Value:** The Layers panel is the primary way to see and navigate all entities in a project — it must feel live, accurate, and interactive.

### Constraints

- **Tech stack**: Rust, WGPU (immediate-mode rendering), winit event loop — no UI framework, all rendering is manual `InstanceRaw` quads
- **Native only**: VST3 and GUI opening are macOS/native only (`#[cfg(feature = "native")]`)
- **No audio DSP**: Per-clip effect audio routing is out of scope this milestone
<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->
## Technology Stack

## Languages
- Rust 1.94.0 (Edition 2021) - Application core, audio engine, UI, networking
- C++ (Objective-C++) - VST3 GUI integration on macOS (Apple frameworks)
- JavaScript/TypeScript - WASM target support via web-sys bindings
- CMake - Build system for C++ VST3 components
## Runtime
- Rust 1.94.0 (stable)
- Native: macOS/Darwin (aarch64 and x86_64), Linux, Windows via Rust ecosystem
- Web: WebAssembly (wasm32 target)
- Cargo (Rust package manager)
- Lockfile: `Cargo.lock` (present, committed)
## Frameworks
- `winit` 0.30 - Window management and event handling (cross-platform)
- `wgpu` 24 - GPU rendering and graphics API abstraction
- `glyphon` 0.8 - Text rendering
- `cpal` 0.15 (native feature) - Cross-platform audio I/O
- `symphonia` 0.5 (native feature) - Audio decoding (MP3, WAV, OGG, FLAC, ISO MP4, AAC)
- `hound` 3.5 (native feature) - WAV file reading/writing
- `serde` 1 (derive macros) - Serialization framework
- `serde_json` 1 - JSON serialization
- `uuid` 1 (v4, serde, js features) - UUID generation and serialization
- `tokio` 1 (sync, macros, time, rt-multi-thread features) - Async runtime for networking and background tasks
- `surrealdb` 3 (native feature, kv-rocksdb, protocol-ws) - Embedded document database with RocksDB backend
- Supports both local (RocksDB) and remote (WebSocket) connections
- `indexmap` 2 (serde feature) - Ordered hash map
- `bytemuck` 1 (derive feature) - Safe transmute for GPU vertex data
- `log` 0.4 - Logging facade
- `muda` 0.17 (native) - Native menu handling
- `rfd` 0.15 (native) - Native file/folder dialogs (uses system dialogs)
- `dirs` 6 (native) - Platform directory locations
- `env_logger` 0.11 (native) - Logger implementation with environment control
- `futures-util` 0.3 (native) - Async utilities
- `pollster` 0.4 (native) - Blocking on async operations
- `rack` 0.4 - macOS application framework bindings
- `vst3-gui` (local crate) - Custom VST3 GUI integration binding
- `wasm-bindgen` 0.2 - Rust/JavaScript interop
- `wasm-bindgen-futures` 0.4 - Async support for WASM
- `web-sys` 0.3 - JavaScript DOM/Canvas/Event bindings
- `console_log` 1 - Browser console logging
- `console_error_panic_hook` 0.1 - Better panic messages in WASM
- `web-time` 1 - WASM-compatible time primitives
## Key Dependencies
- `surrealdb` 3 - Provides both local embedded database (RocksDB) and remote WebSocket sync capability. Essential for state persistence and real-time collaboration
- `cpal` 0.15 - Cross-platform audio device management and I/O. Core to audio playback
- `tokio` 1 - Async runtime enabling WebSocket connections, file I/O, and background tasks without blocking
- `wgpu` 24 - GPU-accelerated rendering, handles all visual output
- `winit` 0.30 - Event loop and window management, provides foundation for all native platforms
- `symphonia` 0.5 - Audio format decoding, enables support for multiple audio file formats
- `muda` 0.17 - Native menus on macOS, provides native application menu integration
## Configuration
- Feature flags in `Cargo.toml`:
- Platform-specific conditionals:
- Main build: `Cargo.toml` at project root
- VST3 GUI build: `vst3-gui/build.rs` (CMake-based C++ compilation)
- Multi-target release build via `Makefile`:
- VST3 SDK auto-cloned from GitHub (`https://github.com/steinbergmedia/vst3sdk.git`) if not present
- CMake configuration with Clang and Objective-C support
- Link against system frameworks: AppKit, Cocoa, CoreFoundation
## Binary Targets
- `layers` (`src/main.rs`) - Primary application (audio editor/DAW)
- `surreal_server` (`src/bin/surreal_server.rs`) - Standalone SurrealDB server launcher (optional)
- `test_vst3` (`src/bin/test_vst3.rs`) - VST3 plugin testing utility (macOS only)
## Platform Requirements
- Rust 1.94.0+ (2021 edition)
- For native builds: C++ compiler, CMake
- For macOS: Xcode or clang, system frameworks (AppKit, Cocoa, CoreFoundation)
- For VST3 support: Git (to clone SDK)
- Optional: SurrealDB CLI (`surreal` binary) for standalone server testing
- Deployment targets:
- Optional: SurrealDB server instance for remote collaboration features
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

## Naming Patterns
- Lowercase with underscores: `entity_id.rs`, `midi_keyboard.rs`, `storage_roundtrip.rs`
- Module directories use descriptive names: `src/tests/`, `src/ui/`, `src/events/`, `src/storage/`
- Binaries named explicitly in Cargo.toml: `src/bin/test_vst3.rs`, `src/bin/surreal_server.rs`
- Private helpers use lowercase snake_case: `make_waveform()`, `now_ms()`, `local_user_id()`
- Public functions use lowercase snake_case: `new_id()`, `variant_name()`, `invert()`, `commit_op()`
- Helper factory functions prefixed with `make_` in tests: `make_object()`, `make_waveform()`, `make_audio_clip()`
- Grid utility functions are descriptive and functional: `snap_to_grid()`, `clip_height()`, `pixels_per_beat()`
- Array/tuple coordinates use explicit types: `position: [f32; 2]`, `size: [f32; 2]`, `color: [f32; 4]`
- Single-letter variables for loop indices: `i`, `start`, `end`
- Descriptive names for measurements: `fade_in_px`, `sample_bpm`, `pitch_semitones`
- ID variables suffixed with `_id`: `mc_id`, `pb_id`, `id0`, `id1`, `ir_id`
- Type aliases for domain concepts: `pub type UserId = EntityId;`
- Enums in PascalCase with descriptive variants: `WarpMode::Off`, `WarpMode::RePitch`, `WarpMode::Semitone`
- Struct field naming reflects domain: `velocity_lane_height`, `fade_in_curve`, `sample_offset_px`
- Constants in UPPERCASE_SNAKE_CASE: `MIDI_CLIP_DEFAULT_HEIGHT`, `PEAK_BLOCK_SIZE`, `BEAT_SUBDIVISIONS`, `DEFAULT_BPM`
## Code Style
- Rust Edition 2021 (see `Cargo.toml` line 4)
- Standard Rust style (no custom rustfmt.toml detected)
- Line continuations used for long function calls with multiple parameters
- Array initializers on single lines for small collections: `[1.0; 4]`
- Standard library imports first: `use std::...`
- External crate imports grouped: `use bytemuck::{Pod, Zeroable};`
- Internal module imports ordered alphabetically by crate component: `use crate::grid::...`, `use crate::automation::...`
- Conditional imports using `#[cfg]` attributes: `#[cfg(feature = "native")]`
- Module declarations at top of `main.rs` with conditional feature gates
- `#[cfg(test)]` gates for test modules
- Public exports via `pub(crate) use` for commonly used types: `pub(crate) use gpu::{...}`, `pub(crate) use ui::transport::{...}`
## Comments and Documentation
- Section headers marked with separator: `// --- CanvasObject ---`, `// -----------` lines
- Inline comments before important logic: `// TODO: refactor velocity lane rendering before re-enabling`
- Doc comments on public functions and modules: `/// Local user ID for single-user mode`
- Comments explain "why", not "what": `// Use a fixed UUID for the local user — this will be replaced with actual user IDs in Phase 3.`
- Public functions documented with `///` comments
- Parameter and return documentation included: `pub fn new(position: [f32; 2], settings: &Settings) -> Self`
- Returns described inline where non-obvious: `pub fn peak_in_range(&self, sample_start: usize, sample_end: usize) -> f32`
## Function Design
- Helper functions in tests typically 20-40 lines (factory methods, setup)
- Core logic functions 30-150 lines (e.g., `apply()` in `operations.rs`)
- Utility functions 5-15 lines (e.g., `pixels_per_beat()`, `snap_to_grid()`)
- Settings/config passed as references: `settings: &Settings`
- Multiple coordinate values use array types: `position: [f32; 2]` instead of separate x, y params
- Related values grouped in structs (not as multiple parameters)
- Builder pattern not used; direct construction with all fields
- Option<T> for potentially missing values (e.g., `first_selected_mc()` returns `Option<EntityId>`)
- Direct values for computed/derived data
- No Result types observed; panics via `.unwrap()` on assertions
- Conversion functions explicit: `f32_slice_to_u8()`, `u8_slice_to_f32()`
## Module Design
- `pub(crate) use` for internal but frequently used types
- `pub mod theme` for public API modules
- Conditional exports with `#[cfg(...)]` for platform-specific code
- Type aliases exported as `pub type` at module level
- Constants at module top: `pub const MIDI_CLIP_DEFAULT_HEIGHT: f32 = 540.0;`
- Struct definitions next: `pub struct MidiNote { ... }`
- Impl blocks for types and trait implementations below
- Helper functions at end of file or in separate test module
- Private by default (no visibility modifier)
- `pub fn` for public API functions
- `pub(crate)` for internal-use functions accessed from multiple modules
- Fields within structs typically public (direct access pattern, no getters/setters)
## Error Handling
- Assertions via `assert!()` and `assert_eq!()` in tests
- `.unwrap()` for expected-to-succeed operations (e.g., parsing stored data)
- `.unwrap_or_default()` for fallback values (e.g., `duration_since(...).unwrap_or_default()`)
- `.unwrap_or(None)` / `.unwrap_or(false)` for conditional fallbacks
- `.expect()` with message context on critical paths: `.expect("should open '{}' headlessly")`
- Panic via `panic!()` macro: `panic!("Expected DeleteObject")`
- No custom error types; string panic messages used
- Input validation via pattern matching (e.g., checking MIDI pitch ranges 0-127)
- Bounds checks before operations: `if self.peaks.is_empty() || sample_start >= sample_end { return 0.0; }`
- Type invariants maintained through struct field constraints
## Testing Conventions
- All tests in `src/tests/` directory as separate modules
- Not co-located with implementation; dedicated test files per domain
- Test file structure: `src/tests/state_mutations.rs`, `src/tests/operations.rs`, `src/tests/midi_and_instruments.rs`
- Factory functions create test data (e.g., `make_waveform()`, `make_object()`)
- Helpers prefixed with `make_` for object creation
- Helper functions prefixed with `first_selected_*()` for extracting from collections
- Platform-specific helpers gated with `#[cfg(...)]`
- Direct `assert_eq!()` for value comparisons
- `assert!()` for boolean conditions
- `.expect()` with context messages on Option results
- Pattern matching for enum assertions: `match &result { Operation::DeleteObject { id, .. } => assert_eq!(*id, expected_id), _ => panic!("...") }`
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

## Pattern Overview
- **Monolithic stateful App struct** holding all mutable state (6020 lines in `src/main.rs`)
- **Winit event loop** driving all interactions (keyboard, mouse, drag, redraw)
- **WGPU GPU rendering** with immediate-mode instances and waveform vertices
- **Operation log** for undo/redo and network synchronization via `operations.rs`
- **Multi-target**: Native (macOS with VST3) and Web (WASM)
- **Layer-based entity model** with instruments, MIDI clips, waveforms, effects, components
## Layers
- Purpose: Hold all mutable state and coordinate state mutations
- Location: `src/main.rs` (App struct, 6020 lines)
- Contains: Entity maps (IndexMap by EntityId UUID), UI state, drag state, rendering cache
- Depends on: All other modules
- Used by: Event handlers, rendering pipeline, network sync
- Purpose: Immutable domain models for audio/musical objects
- Location: `src/instruments.rs`, `src/midi.rs`, `src/effects.rs`, `src/component.rs`, `src/regions.rs`, `src/grid.rs`, `src/automation.rs`
- Contains: Data structures for InstrumentRegion, MidiClip, EffectRegion, ComponentDef, LoopRegion, ExportRegion
- Depends on: `entity_id.rs`, serde, std libs
- Used by: App state, operations, rendering
- Purpose: Define invertible operations for undo/redo and network sync; track operation history
- Location: `src/operations.rs` (756 lines), `src/history.rs`
- Contains: Operation enum (Create/Update/Delete for each entity type), CommittedOp struct with user_id/timestamp/seq
- Depends on: All entity types, entity_id
- Used by: App for undo/redo, Network for remote sync
- Purpose: Convert winit events into application mutations
- Location: `src/events/` (mod.rs, mouse.rs, keyboard.rs, cursor.rs, scroll.rs, redraw.rs)
- Contains: ApplicationHandler impl for winit, event routing and hit testing logic
- Depends on: App, grid, hit_testing, operations
- Used by: Winit event loop
- Purpose: Convert App state into GPU instances and waveform vertices for drawing
- Location: `src/ui/rendering.rs` (907 lines), `src/gpu.rs`
- Contains: RenderContext struct, build_instances(), build_waveform_vertices(), GPU pipeline
- Depends on: All entity types, theme, settings
- Used by: Event loop for redraw, App for dirty tracking
- Purpose: Map screen coordinates to entities and interactive regions
- Location: `src/ui/hit_testing.rs`
- Contains: hit_test(), hit_test_waveform_edge(), hit_test_automation_point(), canonical_rect(), rects_overlap()
- Depends on: Entity types, camera
- Used by: Mouse events, drag handlers
- Purpose: Render specialized UI panels and windows
- Location: `src/ui/` (browser.rs, palette.rs, settings_window.rs, tooltip.rs, toast.rs, plugin_editor.rs, right_window.rs, transport.rs, waveform.rs, value_entry.rs, context_menu.rs)
- Contains: SampleBrowser, CommandPalette, SettingsWindow, PluginEditorWindow, ToastManager, TooltipState, etc.
- Depends on: Entity types, rendering, theme
- Used by: App for UI state and event handling
- Purpose: Save/load projects locally or to remote database
- Location: `src/storage/` (mod.rs, models.rs, local.rs, remote.rs, conversions.rs, helpers.rs)
- Contains: ProjectState model, Storage (local file), RemoteStorage (SurrealDB), model conversions
- Depends on: All entity types, serde
- Used by: App for project I/O
- Purpose: Load audio files, generate waveform peaks, playback via CPAL
- Location: `src/audio.rs` (native only), `src/ui/waveform.rs` (waveform peaks/display)
- Contains: AudioEngine, AudioRecorder, audio file loading via symphonia
- Depends on: CPAL, symphonia, hound
- Used by: App for recording/playback
- Purpose: Load VST3 plugins, scan registry, instantiate on demand
- Location: `src/plugins.rs`, `src/effects.rs` (EffectRegion, PluginBlock), `src/instruments.rs` (InstrumentRegion)
- Contains: PluginRegistry, PluginInfo cache, plugin scanning logic
- Depends on: rack crate (VST3 wrapper), native-only
- Used by: App for instrument/effect instantiation
- Purpose: Remote sync of operations via WebSocket and distributed user state
- Location: `src/network.rs`, `src/surreal_client.rs`, `src/user.rs`
- Contains: NetworkManager, network modes (offline/connected), remote user tracking
- Depends on: tokio, surrealdb
- Used by: App for remote ops and user cursor sync
- Purpose: Musical grid calculations, snap logic, BPM-aware sizing
- Location: `src/grid.rs`
- Contains: Adaptive/fixed grid modes, snap functions, beat subdivision, tempo scaling
- Depends on: Settings
- Used by: Operations, rendering, drag handlers
- Purpose: Color scheme, UI layout dimensions, user preferences
- Location: `src/theme.rs`, `src/settings.rs`
- Contains: Theme struct with colors, Settings with grid mode, snap toggles, audio settings
- Depends on: serde
- Used by: Rendering, UI components
## Data Flow
- All state in `App` struct: entities in IndexMap<EntityId, Type>
- Entities never deleted from map directly; only via Operation::Delete
- Deleted entities marked with optional fields (e.g., audio_clips sparse)
- Undo: pop from `op_undo_stack`, invert operation, push to `op_redo_stack`
- Redo: pop from `op_redo_stack`, apply operation, push back to `op_undo_stack`
## Key Abstractions
- Purpose: UUID-based unique identifier for any entity
- Examples: `src/entity_id.rs` (type alias to uuid::Uuid)
- Pattern: UUID v4 generated per entity, immutable, used as HashMap key
- Purpose: Represent any mutation as a before/after pair for undo/redo/sync
- Examples: `Operation::CreateWaveform { id, data, audio_clip }`, `Operation::UpdateObject { id, before, after }`
- Pattern: Enum with named variants per entity type; CommittedOp wraps with user_id, seq, timestamp
- Purpose: Identify which entity (and its sub-part) was clicked/hovered
- Examples: `HitTarget::Waveform(id)`, `HitTarget::PluginBlock(id)`, `HitTarget::MidiNote(clip_id, note_idx)`
- Pattern: Enum variant per entity type; used to deref into correct IndexMap
- Purpose: Track multi-state drag operation from mouse down to release
- Examples: `DragState::ResizingWaveform { wf_id, initial_position_x, before, ... }`, `DragState::MovingSelection { offsets, ... }`
- Pattern: Enum variant per drag action type; holds snapshots of before-state for undo
- Purpose: Audio clip position, size, fade curves, sample BPM, pitch, volume
- Location: `src/ui/waveform.rs`
- Pattern: Stored in `App.waveforms` map; paired with optional `AudioClipData` in `App.audio_clips`
- Purpose: Hierarchical organization of entities for browser panel display
- Location: `src/layers.rs`
- Pattern: `LayerNode` tree built from instrument regions (with MIDI children), waveforms, effect regions (with plugin block children)
## Entry Points
- Location: `src/main.rs` fn main()
- Triggers: Application launch
- Responsibilities: Create event loop, initialize audio/storage, call App::new_native(), run event loop
- Location: `src/main.rs` fn main() (non-native cfg)
- Triggers: HTML canvas container with id="canvas-container"
- Responsibilities: Initialize Gpu via wasm_bindgen, create App::new_web(), set up winit canvas integration
- Location: `src/bin/test_vst3.rs`
- Triggers: Cargo run --bin test_vst3
- Responsibilities: VST3 plugin instantiation and GUI testing on macOS
- Location: `src/bin/surreal_server.rs`
- Triggers: Cargo run --bin surreal_server
- Responsibilities: In-process SurrealDB instance for local collaborative testing
## Error Handling
- Audio file decode failures → `PendingAudioLoad::Failed { wf_id }` → placeholder waveform removed
- Plugin instantiation failures → region marked with `pending_state`/`pending_params`, no error toast (silent)
- Operation apply failures → logged but state inconsistency possible (no rollback)
- Network connection loss → `NetworkMode::Offline`, reconnect loop with exponential backoff
- Storage failures → operation not committed to undo stack
## Cross-Cutting Concerns
<!-- GSD:architecture-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd:quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd:debug` for investigation and bug fixing
- `/gsd:execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->

<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd:profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
