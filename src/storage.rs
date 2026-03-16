use std::path::{Path, PathBuf};

use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use crate::entity_id::{EntityId, new_id};
use crate::settings::{AdaptiveGridSize, FixedGrid, GridMode};
use crate::CanvasObject;

// ---------------------------------------------------------------------------
// Grid mode serialisation helpers
// ---------------------------------------------------------------------------

pub fn grid_mode_to_stored(mode: GridMode) -> (String, String) {
    match mode {
        GridMode::Fixed(fg) => ("fixed".to_string(), fg.label().to_string()),
        GridMode::Adaptive(ag) => ("adaptive".to_string(), ag.label().to_string()),
    }
}

pub fn grid_mode_from_stored(tag: &str, value: &str) -> GridMode {
    match tag {
        "fixed" => {
            let fg = match value {
                "8 Bars" => FixedGrid::Bars8,
                "4 Bars" => FixedGrid::Bars4,
                "2 Bars" => FixedGrid::Bars2,
                "1 Bar" => FixedGrid::Bar1,
                "1/2" => FixedGrid::Half,
                "1/4" => FixedGrid::Quarter,
                "1/8" => FixedGrid::Eighth,
                "1/16" => FixedGrid::Sixteenth,
                "1/32" => FixedGrid::ThirtySecond,
                _ => FixedGrid::Quarter,
            };
            GridMode::Fixed(fg)
        }
        "adaptive" => {
            let ag = match value {
                "Widest" => AdaptiveGridSize::Widest,
                "Wide" => AdaptiveGridSize::Wide,
                "Medium" => AdaptiveGridSize::Medium,
                "Narrow" => AdaptiveGridSize::Narrow,
                "Narrowest" => AdaptiveGridSize::Narrowest,
                _ => AdaptiveGridSize::Medium,
            };
            GridMode::Adaptive(ag)
        }
        _ => GridMode::default(),
    }
}

// ---------------------------------------------------------------------------
// EntityId <-> String helpers for SurrealDB serialisation
// ---------------------------------------------------------------------------

fn entity_id_to_string(id: EntityId) -> String {
    id.to_string()
}

fn entity_id_from_string(s: &str) -> EntityId {
    s.parse::<EntityId>().unwrap_or_else(|_| new_id())
}

// ---------------------------------------------------------------------------
// Per-project schema
// ---------------------------------------------------------------------------

#[derive(Clone, SurrealValue)]
pub struct StoredCanvasObject {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub border_radius: f32,
}

#[derive(Clone, SurrealValue)]
pub struct StoredWaveform {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub border_radius: f32,
    pub filename: String,
    pub fade_in_px: f32,
    pub fade_out_px: f32,
    pub fade_in_curve: f32,
    pub fade_out_curve: f32,
    pub sample_rate: u32,
    pub volume: f32,
    pub disabled: bool,
    pub sample_offset_px: f32,
    pub automation_volume: Vec<[f32; 2]>,
    pub automation_pan: Vec<[f32; 2]>,
}

#[derive(Clone, SurrealValue)]
pub struct StoredEffectRegion {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub plugin_ids: Vec<String>,
    pub plugin_names: Vec<String>,
    pub name: String,
}

#[derive(Clone, SurrealValue)]
pub struct StoredPluginBlock {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub plugin_id: String,
    pub plugin_name: String,
    pub bypass: bool,
    pub state: Vec<u8>,
    pub params: Vec<u8>,
}

#[derive(Clone, SurrealValue)]
pub struct StoredLoopRegion {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub enabled: bool,
}

#[derive(Clone, SurrealValue)]
pub struct StoredComponent {
    pub id: String,
    pub name: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub waveform_ids: Vec<String>,
}

#[derive(Clone, SurrealValue)]
pub struct StoredComponentInstance {
    pub id: String,
    pub component_id: String,
    pub position: [f32; 2],
}

#[derive(Clone, SurrealValue)]
pub struct StoredMidiNote {
    pub pitch: u32,
    pub start_px: f32,
    pub duration_px: f32,
    pub velocity: u32,
}

#[derive(Clone, SurrealValue)]
pub struct StoredMidiClip {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub notes: Vec<StoredMidiNote>,
    pub pitch_low: u32,
    pub pitch_high: u32,
    pub grid_mode_tag: String,
    pub grid_mode_value: String,
    pub triplet_grid: bool,
}

#[derive(Clone, SurrealValue)]
pub struct StoredInstrumentRegion {
    pub id: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub name: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub state: Vec<u8>,
    pub params: Vec<u8>,
}

#[derive(SurrealValue)]
pub struct ProjectState {
    pub version: u32,
    pub name: String,
    pub camera_position: [f32; 2],
    pub camera_zoom: f32,
    pub objects: Vec<StoredCanvasObject>,
    pub waveforms: Vec<StoredWaveform>,
    pub browser_folders: Vec<String>,
    pub browser_width: f32,
    pub browser_visible: bool,
    pub browser_expanded: Vec<String>,
    pub effect_regions: Vec<StoredEffectRegion>,
    pub plugin_blocks: Vec<StoredPluginBlock>,
    pub loop_regions: Vec<StoredLoopRegion>,
    pub components: Vec<StoredComponent>,
    pub component_instances: Vec<StoredComponentInstance>,
    pub bpm: f32,
    pub midi_clips: Vec<StoredMidiClip>,
    pub instrument_regions: Vec<StoredInstrumentRegion>,
}

// ---------------------------------------------------------------------------
// Conversion: IndexMap <-> Vec<Stored*> for save/load
// ---------------------------------------------------------------------------

use indexmap::IndexMap;

/// Convert an IndexMap of CanvasObjects to stored format.
pub fn objects_to_stored(map: &IndexMap<EntityId, CanvasObject>) -> Vec<StoredCanvasObject> {
    map.iter()
        .map(|(id, obj)| StoredCanvasObject {
            id: entity_id_to_string(*id),
            position: obj.position,
            size: obj.size,
            color: obj.color,
            border_radius: obj.border_radius,
        })
        .collect()
}

/// Convert stored objects back to an IndexMap. Old projects without `id` get new UUIDs.
pub fn objects_from_stored(stored: Vec<StoredCanvasObject>) -> IndexMap<EntityId, CanvasObject> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            let obj = CanvasObject {
                position: s.position,
                size: s.size,
                color: s.color,
                border_radius: s.border_radius,
            };
            (id, obj)
        })
        .collect()
}

/// Extract the EntityId and stored data for a waveform vec.
/// Returns (EntityId, StoredWaveform) pairs for downstream processing.
pub fn waveforms_from_stored(stored: Vec<StoredWaveform>) -> Vec<(EntityId, StoredWaveform)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn effect_regions_from_stored(
    stored: Vec<StoredEffectRegion>,
) -> Vec<(EntityId, StoredEffectRegion)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn plugin_blocks_from_stored(
    stored: Vec<StoredPluginBlock>,
) -> Vec<(EntityId, StoredPluginBlock)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn loop_regions_from_stored(
    stored: Vec<StoredLoopRegion>,
) -> Vec<(EntityId, StoredLoopRegion)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn components_from_stored(
    stored: Vec<StoredComponent>,
) -> Vec<(EntityId, StoredComponent)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn component_instances_from_stored(
    stored: Vec<StoredComponentInstance>,
) -> Vec<(EntityId, StoredComponentInstance)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn midi_clips_from_stored(
    stored: Vec<StoredMidiClip>,
) -> Vec<(EntityId, StoredMidiClip)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

pub fn instrument_regions_from_stored(
    stored: Vec<StoredInstrumentRegion>,
) -> Vec<(EntityId, StoredInstrumentRegion)> {
    stored
        .into_iter()
        .map(|s| {
            let id = if s.id.is_empty() {
                new_id()
            } else {
                entity_id_from_string(&s.id)
            };
            (id, s)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Audio / peaks stored per waveform
// ---------------------------------------------------------------------------

#[derive(Clone, SurrealValue)]
pub struct StoredAudioData {
    pub waveform_id: String,
    pub left_samples: Vec<u8>,
    pub right_samples: Vec<u8>,
    pub mono_samples: Vec<u8>,
    pub sample_rate: u32,
    pub duration_secs: f32,
}

#[derive(Clone, SurrealValue)]
pub struct StoredPeaks {
    pub waveform_id: String,
    pub block_size: u64,
    pub left_peaks: Vec<u8>,
    pub right_peaks: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Project metadata (project.json in project folder)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectMeta {
    pub name: String,
}

// ---------------------------------------------------------------------------
// Global index DB schema
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, SurrealValue)]
pub struct ProjectIndexEntry {
    pub name: String,
    pub path: String,
    pub is_temp: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

// ---------------------------------------------------------------------------
// f32 <-> u8 helpers
// ---------------------------------------------------------------------------

pub fn f32_slice_to_u8(data: &[f32]) -> Vec<u8> {
    bytemuck::cast_slice(data).to_vec()
}

pub fn u8_slice_to_f32(data: &[u8]) -> Vec<f32> {
    if data.len() % 4 != 0 {
        return Vec::new();
    }
    bytemuck::cast_slice(data).to_vec()
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub struct Storage {
    rt: tokio::runtime::Runtime,
    temp_projects_dir: PathBuf,
    index_db: Surreal<Db>,
    project_db: Option<Surreal<Db>>,
    current_project_path: Option<PathBuf>,
    is_temp: bool,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn open_db(rt: &tokio::runtime::Runtime, path: &Path) -> Option<Surreal<Db>> {
    rt.block_on(async {
        let db = Surreal::new::<RocksDb>(path.to_str()?).await.ok()?;
        db.use_ns("layers").use_db("canvas").await.ok()?;
        Some(db)
    })
}

impl Storage {
    /// Opens the global index DB and prepares the temp projects directory.
    pub fn open(base_path: &Path) -> Option<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(10 * 1024 * 1024)
            .build()
            .ok()?;

        let temp_projects_dir = base_path.join("projects");
        std::fs::create_dir_all(&temp_projects_dir).ok()?;

        let index_db_path = base_path.join("index.db");
        let index_db = open_db(&rt, &index_db_path)?;

        log::info!("Storage opened at {:?}", base_path);
        Some(Storage {
            rt,
            temp_projects_dir,
            index_db,
            project_db: None,
            current_project_path: None,
            is_temp: false,
        })
    }

    // -----------------------------------------------------------------------
    // Project lifecycle
    // -----------------------------------------------------------------------

    /// Opens a per-project DB inside `path/db/`.
    pub fn open_project(&mut self, path: &Path) -> bool {
        self.close_project();
        let db_path = path.join("db");
        std::fs::create_dir_all(&db_path).ok();
        match open_db(&self.rt, &db_path) {
            Some(db) => {
                self.project_db = Some(db);
                self.current_project_path = Some(path.to_path_buf());
                // Check if this is a temp project by looking at the index
                let key = path.to_string_lossy().to_string();
                self.is_temp = self
                    .rt
                    .block_on(async {
                        let entry: Option<ProjectIndexEntry> =
                            self.index_db.select(("projects", &*key)).await.ok()?;
                        entry.map(|e| e.is_temp)
                    })
                    .unwrap_or(false);
                log::info!("Opened project DB at {:?} (temp={})", db_path, self.is_temp);
                true
            }
            None => {
                log::error!("Failed to open project DB at {:?}", db_path);
                false
            }
        }
    }

    /// Drops the current project DB.
    pub fn close_project(&mut self) {
        self.project_db = None;
        self.current_project_path = None;
        self.is_temp = false;
    }

    /// Creates a new temp project under `~/.layers/projects/tmp-<ts>/`.
    pub fn create_temp_project(&mut self) -> Option<PathBuf> {
        let ts = now_ts();
        let dir = self.temp_projects_dir.join(format!("tmp-{ts}"));
        std::fs::create_dir_all(&dir).ok()?;
        if !self.open_project(&dir) {
            return None;
        }
        self.is_temp = true;

        // Add to global index
        let entry = ProjectIndexEntry {
            name: "Untitled".to_string(),
            path: dir.to_string_lossy().to_string(),
            is_temp: true,
            created_at: ts,
            updated_at: ts,
        };
        let key = dir.to_string_lossy().to_string();
        self.upsert_index_entry(&key, entry);
        log::info!("Created temp project at {:?}", dir);
        Some(dir)
    }

    /// Moves/copies the current project folder to `dest`, reopens DB there.
    pub fn save_project_to(&mut self, dest: &Path) -> bool {
        let src = match &self.current_project_path {
            Some(p) => p.clone(),
            None => return false,
        };
        if src == dest {
            return true;
        }

        // Close current DB so RocksDB lock is released
        let was_temp = self.is_temp;
        self.project_db = None;

        // Create dest and copy contents
        if let Err(e) = copy_dir_all(&src, dest) {
            log::error!("Failed to copy project to {:?}: {e}", dest);
            // Try to reopen at old location
            self.open_project(&src);
            self.is_temp = was_temp;
            return false;
        }

        // Remove old temp dir
        if was_temp {
            let _ = std::fs::remove_dir_all(&src);
            // Remove old index entry
            self.delete_index_entry(&src.to_string_lossy());
        }

        // Reopen at new location
        if !self.open_project(dest) {
            log::error!("Failed to reopen project at {:?}", dest);
            return false;
        }
        self.is_temp = false;

        // Update index
        let entry = ProjectIndexEntry {
            name: String::new(), // will be updated on next save_project_state
            path: dest.to_string_lossy().to_string(),
            is_temp: false,
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        let key = dest.to_string_lossy().to_string();
        self.upsert_index_entry(&key, entry);
        true
    }

    pub fn is_temp_project(&self) -> bool {
        self.is_temp
    }

    pub fn current_project_path(&self) -> Option<&Path> {
        self.current_project_path.as_deref()
    }

    // -----------------------------------------------------------------------
    // project.json
    // -----------------------------------------------------------------------

    pub fn write_project_json(&self, name: &str) {
        let path = match &self.current_project_path {
            Some(p) => p.join("project.json"),
            None => return,
        };
        let meta = ProjectMeta {
            name: name.to_string(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write project.json: {e}");
            }
        }
    }

    pub fn read_project_json(path: &Path) -> Option<ProjectMeta> {
        let json_path = path.join("project.json");
        let contents = std::fs::read_to_string(&json_path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    // -----------------------------------------------------------------------
    // Project state (per-project DB)
    // -----------------------------------------------------------------------

    pub fn save_project_state(&self, state: ProjectState) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };

        self.write_project_json(&state.name);

        let result = self.rt.block_on(async {
            let _: Option<ProjectState> = db.upsert(("state", "main")).content(state).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save project state: {e}");
        }

        // Update index entry name + timestamp
        if let Some(path) = &self.current_project_path {
            let key = path.to_string_lossy().to_string();
            self.update_index_timestamp(&key);
        }
    }

    pub fn load_project_state(&self) -> Option<ProjectState> {
        let db = self.project_db.as_ref()?;
        self.rt.block_on(async {
            let state: Option<ProjectState> = db.select(("state", "main")).await.ok()?;
            state
        })
    }

    // -----------------------------------------------------------------------
    // Audio data (per-project DB) — keyed by EntityId string
    // -----------------------------------------------------------------------

    pub fn save_audio(
        &self,
        waveform_id: &str,
        left: &[f32],
        right: &[f32],
        mono: &[f32],
        sample_rate: u32,
        duration_secs: f32,
    ) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let data = StoredAudioData {
            waveform_id: waveform_id.to_string(),
            left_samples: f32_slice_to_u8(left),
            right_samples: f32_slice_to_u8(right),
            mono_samples: f32_slice_to_u8(mono),
            sample_rate,
            duration_secs,
        };
        let result = self.rt.block_on(async {
            let _: Option<StoredAudioData> =
                db.upsert(("audio", waveform_id)).content(data).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save audio data for waveform {waveform_id}: {e}");
        }
    }

    pub fn load_audio(&self, waveform_id: &str) -> Option<StoredAudioData> {
        let db = self.project_db.as_ref()?;
        self.rt.block_on(async {
            let data: Option<StoredAudioData> =
                db.select(("audio", waveform_id)).await.ok()?;
            data
        })
    }

    // -----------------------------------------------------------------------
    // Peaks data (per-project DB) — keyed by EntityId string
    // -----------------------------------------------------------------------

    pub fn save_peaks(
        &self,
        waveform_id: &str,
        block_size: u64,
        left_peaks: &[f32],
        right_peaks: &[f32],
    ) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let data = StoredPeaks {
            waveform_id: waveform_id.to_string(),
            block_size,
            left_peaks: f32_slice_to_u8(left_peaks),
            right_peaks: f32_slice_to_u8(right_peaks),
        };
        let result = self.rt.block_on(async {
            let _: Option<StoredPeaks> =
                db.upsert(("peaks", waveform_id)).content(data).await?;
            Ok::<(), surrealdb::Error>(())
        });
        if let Err(e) = result {
            log::error!("Failed to save peaks for waveform {waveform_id}: {e}");
        }
    }

    pub fn load_peaks(&self, waveform_id: &str) -> Option<StoredPeaks> {
        let db = self.project_db.as_ref()?;
        self.rt.block_on(async {
            let data: Option<StoredPeaks> =
                db.select(("peaks", waveform_id)).await.ok()?;
            data
        })
    }

    /// Clear all audio and peaks records (called before full rewrite on save).
    pub fn clear_audio_and_peaks(&self) {
        let db = match &self.project_db {
            Some(db) => db,
            None => return,
        };
        let _ = self.rt.block_on(async {
            let _: Vec<StoredAudioData> = db.delete("audio").await?;
            let _: Vec<StoredPeaks> = db.delete("peaks").await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    // -----------------------------------------------------------------------
    // Global index
    // -----------------------------------------------------------------------

    pub fn list_projects(&self) -> Vec<ProjectIndexEntry> {
        let mut entries = self.rt
            .block_on(async {
                let entries: Vec<ProjectIndexEntry> =
                    self.index_db.select("projects").await.ok()?;
                Some(entries)
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        entries
    }

    pub fn delete_project(&mut self, path: &str) {
        // Remove from index
        self.delete_index_entry(path);
        // Remove folder
        let _ = std::fs::remove_dir_all(path);
        log::info!("Deleted project at {path}");
    }

    pub fn update_index_name(&self, path: &str, name: &str) {
        let _result = self.rt.block_on(async {
            // Load existing entry, update name
            let existing: Option<ProjectIndexEntry> =
                self.index_db.select(("projects", path)).await.ok()?;
            if let Some(mut entry) = existing {
                entry.name = name.to_string();
                entry.updated_at = now_ts();
                let _: Option<ProjectIndexEntry> = self
                    .index_db
                    .upsert(("projects", path))
                    .content(entry)
                    .await
                    .ok()?;
            }
            Some(())
        });
    }

    // -----------------------------------------------------------------------
    // Index helpers (private)
    // -----------------------------------------------------------------------

    fn upsert_index_entry(&self, key: &str, entry: ProjectIndexEntry) {
        let _ = self.rt.block_on(async {
            let _: Option<ProjectIndexEntry> = self
                .index_db
                .upsert(("projects", key))
                .content(entry)
                .await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    fn delete_index_entry(&self, key: &str) {
        let _ = self.rt.block_on(async {
            let _: Option<ProjectIndexEntry> = self.index_db.delete(("projects", key)).await?;
            Ok::<(), surrealdb::Error>(())
        });
    }

    fn update_index_timestamp(&self, key: &str) {
        let _ = self.rt.block_on(async {
            let existing: Option<ProjectIndexEntry> =
                self.index_db.select(("projects", key)).await.ok()?;
            if let Some(mut entry) = existing {
                entry.updated_at = now_ts();
                let _: Option<ProjectIndexEntry> = self
                    .index_db
                    .upsert(("projects", key))
                    .content(entry)
                    .await
                    .ok()?;
            }
            Some(())
        });
    }
}

// ---------------------------------------------------------------------------
// Utility: recursive dir copy
// ---------------------------------------------------------------------------

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Default base path
// ---------------------------------------------------------------------------

pub fn default_base_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".layers")
}
