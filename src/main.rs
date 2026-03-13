mod audio;
mod browser;
mod component;
mod context_menu;
mod effects;
mod palette;
mod plugin_editor;
mod settings;
mod storage;
mod waveform;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use audio::{load_audio_file, AudioClipData, AudioEngine, AudioRecorder, PIXELS_PER_SECOND};
pub(crate) use waveform::WaveformObject;
use waveform::{WaveformPeaks, WaveformVertex};
use context_menu::{
    ContextMenu, ContextMenuEntry, MenuContext, CTX_MENU_ITEM_HEIGHT, CTX_MENU_PADDING,
    CTX_MENU_SECTION_HEIGHT, CTX_MENU_SEPARATOR_HEIGHT, CTX_MENU_WIDTH,
};
use settings::GridMode;
use palette::{
    CommandAction, CommandPalette, PaletteMode, PaletteRow, COMMANDS, PALETTE_INPUT_HEIGHT,
    PALETTE_ITEM_HEIGHT, PALETTE_PADDING, PALETTE_SECTION_HEIGHT, PALETTE_WIDTH,
};

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Color as TextColor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use surrealdb::types::SurrealValue;
use wgpu::util::DeviceExt;

use settings::{Settings, SettingsWindow, CATEGORIES};
use storage::{default_db_path, ProjectState, Storage};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    platform::macos::WindowAttributesExtMacOS,
    window::{CursorIcon, Window, WindowId},
};
use muda::{MenuId, Submenu as MudaSubmenu};

// ---------------------------------------------------------------------------
// Shader (WGSL)
// ---------------------------------------------------------------------------

const SHADER_SRC: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,
    @location(2) rect_size: vec2<f32>,
    @location(3) border_radius: f32,
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) obj_pos: vec2<f32>,
    @location(2) obj_size: vec2<f32>,
    @location(3) obj_color: vec4<f32>,
    @location(4) radius: f32,
) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = obj_pos + position * obj_size;
    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 0.0, 1.0);
    out.color = obj_color;
    out.local_pos = position * obj_size;
    out.rect_size = obj_size;
    out.border_radius = radius;
    return out;
}

fn rounded_box_sdf(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = min(in.border_radius, min(in.rect_size.x, in.rect_size.y) * 0.5);
    if (r < 0.01) {
        return in.color;
    }
    let center = in.rect_size * 0.5;
    let p = in.local_pos - center;
    let d = rounded_box_sdf(p, center, r);
    let fw = fwidth(d);
    let alpha = 1.0 - smoothstep(0.0, fw, d);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

const WAVEFORM_SHADER_SRC: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct WfOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) edge_dist: f32,
}

@vertex
fn wf_vs(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) edge: f32,
) -> WfOut {
    var out: WfOut;
    out.clip_position = camera.view_proj * vec4<f32>(pos, 0.0, 1.0);
    out.color = color;
    out.edge_dist = edge;
    return out;
}

@fragment
fn wf_fs(in: WfOut) -> @location(0) vec4<f32> {
    let aa = 1.0 - smoothstep(0.0, 1.0, abs(in.edge_dist));
    return vec4<f32>(in.color.rgb, in.color.a * aa);
}
"#;

// ---------------------------------------------------------------------------
// GPU data types
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
}

const QUAD_VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, 0.0],
    },
    Vertex {
        position: [1.0, 1.0],
    },
    Vertex {
        position: [0.0, 1.0],
    },
];

const QUAD_INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstanceRaw {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub border_radius: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

const MAX_INSTANCES: usize = 16384;

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

pub(crate) struct Camera {
    pub(crate) position: [f32; 2],
    pub(crate) zoom: f32,
}

impl Camera {
    fn new() -> Self {
        Self {
            position: [-100.0, -50.0],
            zoom: 1.0,
        }
    }

    fn view_proj(&self, width: f32, height: f32) -> [[f32; 4]; 4] {
        let z = self.zoom;
        let cx = self.position[0];
        let cy = self.position[1];
        [
            [2.0 * z / width, 0.0, 0.0, 0.0],
            [0.0, -2.0 * z / height, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [
                -2.0 * z * cx / width - 1.0,
                2.0 * z * cy / height + 1.0,
                0.0,
                1.0,
            ],
        ]
    }

    fn screen_to_world(&self, screen: [f32; 2]) -> [f32; 2] {
        [
            screen[0] / self.zoom + self.position[0],
            screen[1] / self.zoom + self.position[1],
        ]
    }

    fn zoom_at(&mut self, screen_pos: [f32; 2], factor: f32) {
        let world = self.screen_to_world(screen_pos);
        self.zoom = (self.zoom * factor).clamp(0.05, 200.0);
        self.position[0] = world[0] - screen_pos[0] / self.zoom;
        self.position[1] = world[1] - screen_pos[1] / self.zoom;
    }
}

fn screen_ortho(width: f32, height: f32) -> [[f32; 4]; 4] {
    [
        [2.0 / width, 0.0, 0.0, 0.0],
        [0.0, -2.0 / height, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
    ]
}

// ---------------------------------------------------------------------------
// Canvas objects
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, SurrealValue)]
pub struct CanvasObject {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub border_radius: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitTarget {
    Object(usize),
    Waveform(usize),
    EffectRegion(usize),
    ComponentDef(usize),
    ComponentInstance(usize),
}

const MAX_UNDO_HISTORY: usize = 50;

#[derive(Clone)]
struct EffectRegionSnapshot {
    position: [f32; 2],
    size: [f32; 2],
    plugin_ids: Vec<String>,
    plugin_names: Vec<String>,
    name: String,
}

#[derive(Clone)]
struct Snapshot {
    objects: Vec<CanvasObject>,
    waveforms: Vec<WaveformObject>,
    audio_clips: Vec<AudioClipData>,
    effect_regions: Vec<EffectRegionSnapshot>,
    components: Vec<component::ComponentDef>,
    component_instances: Vec<component::ComponentInstance>,
}

struct ExportRegion {
    position: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
enum ExportHover {
    None,
    Body,
    RenderPill,
    CornerNW,
    CornerNE,
    CornerSW,
    CornerSE,
}

const EXPORT_REGION_DEFAULT_WIDTH: f32 = 800.0;
const EXPORT_REGION_DEFAULT_HEIGHT: f32 = 300.0;
const EXPORT_FILL_COLOR: [f32; 4] = [0.15, 0.70, 0.55, 0.10];
const EXPORT_BORDER_COLOR: [f32; 4] = [0.20, 0.80, 0.60, 0.50];
const EXPORT_RENDER_PILL_COLOR: [f32; 4] = [0.15, 0.65, 0.50, 0.85];
const EXPORT_RENDER_PILL_W: f32 = 110.0;
const EXPORT_RENDER_PILL_H: f32 = 22.0;

enum DragState {
    None,
    Panning {
        start_mouse: [f32; 2],
        start_camera: [f32; 2],
    },
    Selecting {
        start_world: [f32; 2],
    },
    MovingSelection {
        offsets: Vec<(HitTarget, [f32; 2])>,
    },
    DraggingFromBrowser {
        path: PathBuf,
        filename: String,
    },
    DraggingPlugin {
        plugin_id: String,
        plugin_name: String,
    },
    ResizingBrowser,
    MovingExportRegion {
        offset: [f32; 2],
    },
    ResizingExportRegion {
        anchor: [f32; 2],
        nwse: bool,
    },
    DraggingFade {
        waveform_idx: usize,
        is_fade_in: bool,
    },
    ResizingComponentDef {
        comp_idx: usize,
        anchor: [f32; 2],
        nwse: bool,
    },
    ResizingEffectRegion {
        region_idx: usize,
        anchor: [f32; 2],
        nwse: bool,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum ComponentDefHover {
    None,
    CornerNW(usize),
    CornerNE(usize),
    CornerSW(usize),
    CornerSE(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum EffectRegionHover {
    None,
    CornerNW(usize),
    CornerNE(usize),
    CornerSW(usize),
    CornerSE(usize),
    PluginLabel(usize, usize),
}

#[derive(Clone)]
enum ClipboardItem {
    Object(CanvasObject),
    Waveform(WaveformObject, Option<AudioClipData>),
    EffectRegion(effects::EffectRegion),
    ComponentDef(component::ComponentDef, Vec<(WaveformObject, Option<AudioClipData>)>),
    ComponentInstance(component::ComponentInstance),
}

struct Clipboard {
    items: Vec<ClipboardItem>,
}

impl Clipboard {
    fn new() -> Self {
        Self { items: Vec::new() }
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WAVEFORM_COLORS: &[[f32; 4]] = &[
    [0.40, 0.72, 1.00, 1.0],
    [1.00, 0.55, 0.35, 1.0],
    [0.45, 0.92, 0.55, 1.0],
    [0.92, 0.45, 0.80, 1.0],
    [1.00, 0.85, 0.32, 1.0],
];

const SEL_COLOR: [f32; 4] = [0.35, 0.65, 1.0, 0.8];

// Audio formats supported via symphonia: wav, mp3, ogg, flac, aac
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac", "aac", "m4a", "mp4"];

// ---------------------------------------------------------------------------
// Transport Panel (bottom-center playback status)
// ---------------------------------------------------------------------------

const TRANSPORT_WIDTH: f32 = 210.0;
const TRANSPORT_HEIGHT: f32 = 36.0;
const TRANSPORT_BOTTOM_MARGIN: f32 = 32.0;
const FX_BUTTON_WIDTH: f32 = 42.0;
const FX_BUTTON_GAP: f32 = 10.0;
const EXPORT_BUTTON_WIDTH: f32 = 50.0;
const EXPORT_BUTTON_GAP: f32 = 10.0;

struct TransportPanel;

impl TransportPanel {
    fn panel_rect(screen_w: f32, screen_h: f32, scale: f32) -> ([f32; 2], [f32; 2]) {
        let w = TRANSPORT_WIDTH * scale;
        let h = TRANSPORT_HEIGHT * scale;
        let x = (screen_w - w) * 0.5;
        let y = screen_h - h - TRANSPORT_BOTTOM_MARGIN * scale;
        ([x, y], [w, h])
    }

    fn fx_button_rect(screen_w: f32, screen_h: f32, scale: f32) -> ([f32; 2], [f32; 2]) {
        let (tp_pos, tp_size) = Self::panel_rect(screen_w, screen_h, scale);
        let w = FX_BUTTON_WIDTH * scale;
        let h = tp_size[1];
        let x = tp_pos[0] - w - FX_BUTTON_GAP * scale;
        let y = tp_pos[1];
        ([x, y], [w, h])
    }

    fn hit_fx_button(pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let (rp, rs) = Self::fx_button_rect(screen_w, screen_h, scale);
        point_in_rect(pos, rp, rs)
    }

    fn record_button_rect(screen_w: f32, screen_h: f32, scale: f32) -> ([f32; 2], [f32; 2]) {
        let (pos, size) = Self::panel_rect(screen_w, screen_h, scale);
        let btn_size = 24.0 * scale;
        let btn_x = pos[0] + size[0] - btn_size - 8.0 * scale;
        let btn_y = pos[1] + (size[1] - btn_size) * 0.5;
        ([btn_x, btn_y], [btn_size, btn_size])
    }

    fn build_instances(
        screen_w: f32,
        screen_h: f32,
        scale: f32,
        is_playing: bool,
        is_recording: bool,
    ) -> Vec<InstanceRaw> {
        let mut out = Vec::new();
        let (pos, size) = Self::panel_rect(screen_w, screen_h, scale);

        // background pill
        out.push(InstanceRaw {
            position: pos,
            size,
            color: [0.12, 0.12, 0.16, 0.85],
            border_radius: size[1] * 0.5,
        });

        let icon_x = pos[0] + 14.0 * scale;
        let icon_cy = pos[1] + size[1] * 0.5;

        if is_playing {
            let bar_w = 3.0 * scale;
            let bar_h = 12.0 * scale;
            let gap = 4.0 * scale;
            out.push(InstanceRaw {
                position: [icon_x, icon_cy - bar_h * 0.5],
                size: [bar_w, bar_h],
                color: [1.0, 1.0, 1.0, 0.9],
                border_radius: 1.0 * scale,
            });
            out.push(InstanceRaw {
                position: [icon_x + bar_w + gap, icon_cy - bar_h * 0.5],
                size: [bar_w, bar_h],
                color: [1.0, 1.0, 1.0, 0.9],
                border_radius: 1.0 * scale,
            });
        } else {
            let tri_w = 10.0 * scale;
            let tri_h = 12.0 * scale;
            let steps = (tri_h * 3.0).ceil() as usize;
            let step_h = tri_h / steps as f32;
            let min_w = 1.5 * scale;
            for i in 0..steps {
                let t = (i as f32 + 0.5) / steps as f32;
                let w = (tri_w * (1.0 - (2.0 * t - 1.0).abs())).max(min_w);
                let sy = icon_cy - tri_h * 0.5 + i as f32 * step_h;
                out.push(InstanceRaw {
                    position: [icon_x, sy],
                    size: [w, step_h + 0.5],
                    color: [1.0, 1.0, 1.0, 0.9],
                    border_radius: min_w * 0.5,
                });
            }
        }

        // record button: red circle (brighter when recording)
        let (rbtn_pos, rbtn_size) = Self::record_button_rect(screen_w, screen_h, scale);
        let dot_diameter = 12.0 * scale;
        let dot_x = rbtn_pos[0] + (rbtn_size[0] - dot_diameter) * 0.5;
        let dot_y = rbtn_pos[1] + (rbtn_size[1] - dot_diameter) * 0.5;

        if is_recording {
            // stop icon: rounded red square
            let sq = 10.0 * scale;
            let sq_x = rbtn_pos[0] + (rbtn_size[0] - sq) * 0.5;
            let sq_y = rbtn_pos[1] + (rbtn_size[1] - sq) * 0.5;
            out.push(InstanceRaw {
                position: [sq_x, sq_y],
                size: [sq, sq],
                color: [0.95, 0.2, 0.2, 1.0],
                border_radius: 2.0 * scale,
            });
        } else {
            out.push(InstanceRaw {
                position: [dot_x, dot_y],
                size: [dot_diameter, dot_diameter],
                color: [0.85, 0.25, 0.25, 0.9],
                border_radius: dot_diameter * 0.5,
            });
        }

        out
    }

    fn build_fx_button_instances(screen_w: f32, screen_h: f32, scale: f32) -> Vec<InstanceRaw> {
        let mut out = Vec::new();
        let (pos, size) = Self::fx_button_rect(screen_w, screen_h, scale);

        out.push(InstanceRaw {
            position: pos,
            size,
            color: [0.14, 0.12, 0.20, 0.85],
            border_radius: size[1] * 0.5,
        });

        // "FX" text approximation: F shape + X shape using small bars
        let cx = pos[0] + size[0] * 0.30;
        let cy = pos[1] + size[1] * 0.5;
        let bar = 2.0 * scale;

        // F: vertical bar
        let f_h = 10.0 * scale;
        out.push(InstanceRaw {
            position: [cx - 4.0 * scale, cy - f_h * 0.5],
            size: [bar, f_h],
            color: [0.70, 0.45, 1.00, 0.90],
            border_radius: 0.0,
        });
        // F: top horizontal
        out.push(InstanceRaw {
            position: [cx - 4.0 * scale, cy - f_h * 0.5],
            size: [6.0 * scale, bar],
            color: [0.70, 0.45, 1.00, 0.90],
            border_radius: 0.0,
        });
        // F: middle horizontal
        out.push(InstanceRaw {
            position: [cx - 4.0 * scale, cy - bar * 0.5],
            size: [5.0 * scale, bar],
            color: [0.70, 0.45, 1.00, 0.90],
            border_radius: 0.0,
        });

        // "+" icon
        let plus_cx = pos[0] + size[0] * 0.72;
        let plus_h = 8.0 * scale;
        let plus_w = 8.0 * scale;
        out.push(InstanceRaw {
            position: [plus_cx - plus_w * 0.5, cy - bar * 0.5],
            size: [plus_w, bar],
            color: [0.70, 0.45, 1.00, 0.70],
            border_radius: 0.0,
        });
        out.push(InstanceRaw {
            position: [plus_cx - bar * 0.5, cy - plus_h * 0.5],
            size: [bar, plus_h],
            color: [0.70, 0.45, 1.00, 0.70],
            border_radius: 0.0,
        });

        out
    }

    fn contains(pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let (rp, rs) = Self::panel_rect(screen_w, screen_h, scale);
        point_in_rect(pos, rp, rs)
    }

    fn hit_record_button(pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let (rp, rs) = Self::record_button_rect(screen_w, screen_h, scale);
        point_in_rect(pos, rp, rs)
    }

    fn export_button_rect(screen_w: f32, screen_h: f32, scale: f32) -> ([f32; 2], [f32; 2]) {
        let (tp_pos, tp_size) = Self::panel_rect(screen_w, screen_h, scale);
        let w = EXPORT_BUTTON_WIDTH * scale;
        let h = tp_size[1];
        let x = tp_pos[0] + tp_size[0] + EXPORT_BUTTON_GAP * scale;
        let y = tp_pos[1];
        ([x, y], [w, h])
    }

    fn hit_export_button(pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let (rp, rs) = Self::export_button_rect(screen_w, screen_h, scale);
        point_in_rect(pos, rp, rs)
    }

    fn build_export_button_instances(screen_w: f32, screen_h: f32, scale: f32) -> Vec<InstanceRaw> {
        let mut out = Vec::new();
        let (pos, size) = Self::export_button_rect(screen_w, screen_h, scale);

        out.push(InstanceRaw {
            position: pos,
            size,
            color: [0.10, 0.18, 0.16, 0.85],
            border_radius: size[1] * 0.5,
        });

        let cy = pos[1] + size[1] * 0.5;
        let bar = 2.0 * scale;

        // Arrow-out icon: vertical bar + arrowhead pointing right
        let icon_cx = pos[0] + size[0] * 0.38;
        let arrow_h = 10.0 * scale;
        // Vertical bar
        out.push(InstanceRaw {
            position: [icon_cx - bar * 0.5, cy - arrow_h * 0.5],
            size: [bar, arrow_h],
            color: [0.20, 0.75, 0.60, 0.90],
            border_radius: 0.0,
        });
        // Horizontal bar (arrow shaft)
        let shaft_w = 7.0 * scale;
        out.push(InstanceRaw {
            position: [icon_cx, cy - bar * 0.5],
            size: [shaft_w, bar],
            color: [0.20, 0.75, 0.60, 0.90],
            border_radius: 0.0,
        });
        // Arrowhead: small chevron using two bars
        let tip_x = icon_cx + shaft_w;
        let chev = 4.0 * scale;
        out.push(InstanceRaw {
            position: [tip_x - chev * 0.3, cy - chev * 0.5],
            size: [chev, bar],
            color: [0.20, 0.75, 0.60, 0.90],
            border_radius: 0.0,
        });
        out.push(InstanceRaw {
            position: [tip_x - chev * 0.3, cy + chev * 0.5 - bar],
            size: [chev, bar],
            color: [0.20, 0.75, 0.60, 0.90],
            border_radius: 0.0,
        });

        out
    }
}

fn format_playback_time(secs: f64) -> String {
    let minutes = (secs / 60.0) as u32;
    let s = secs % 60.0;
    format!("{}:{:04.1}", minutes, s)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const DEFAULT_BPM: f32 = 120.0;

fn pixels_per_beat() -> f32 {
    audio::PIXELS_PER_SECOND * 60.0 / DEFAULT_BPM
}

/// Musical subdivision levels in beats: 32, 16, 8, 4, 2, 1, 1/2, 1/4, 1/8, 1/16, 1/32
const BEAT_SUBDIVISIONS: &[f32] = &[32.0, 16.0, 8.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.125, 0.0625, 0.03125];

/// Returns (minor_spacing_world, beats_per_bar) for adaptive grid.
/// Picks the subdivision where screen-px spacing is closest to the target.
fn musical_grid_spacing(zoom: f32, target_px: f32, triplet: bool) -> f32 {
    let ppb = pixels_per_beat();
    let triplet_mul = if triplet { 2.0 / 3.0 } else { 1.0 };
    let mut best = BEAT_SUBDIVISIONS[0] * ppb * triplet_mul;
    let mut best_diff = f32::MAX;
    for &subdiv in BEAT_SUBDIVISIONS {
        let world_spacing = subdiv * ppb * triplet_mul;
        let screen_spacing = world_spacing * zoom;
        let diff = (screen_spacing - target_px).abs();
        if diff < best_diff {
            best_diff = diff;
            best = world_spacing;
        }
    }
    best
}

fn grid_spacing_for_settings(settings: &Settings, zoom: f32) -> f32 {
    match settings.grid_mode {
        GridMode::Adaptive(size) => {
            musical_grid_spacing(zoom, size.target_px(), settings.triplet_grid)
        }
        GridMode::Fixed(fg) => {
            let ppb = pixels_per_beat();
            let triplet_mul = if settings.triplet_grid { 2.0 / 3.0 } else { 1.0 };
            fg.beats() * ppb * triplet_mul
        }
    }
}

/// Snap a world-X coordinate to the nearest grid line.
pub(crate) fn snap_to_grid(world_x: f32, settings: &Settings, zoom: f32) -> f32 {
    if !settings.grid_enabled || !settings.snap_to_grid {
        return world_x;
    }
    let spacing = grid_spacing_for_settings(settings, zoom);
    (world_x / spacing).round() * spacing
}

pub(crate) fn point_in_rect(pos: [f32; 2], rect_pos: [f32; 2], rect_size: [f32; 2]) -> bool {
    pos[0] >= rect_pos[0]
        && pos[0] <= rect_pos[0] + rect_size[0]
        && pos[1] >= rect_pos[1]
        && pos[1] <= rect_pos[1] + rect_size[1]
}

fn rects_overlap(a_pos: [f32; 2], a_size: [f32; 2], b_pos: [f32; 2], b_size: [f32; 2]) -> bool {
    a_pos[0] < b_pos[0] + b_size[0]
        && a_pos[0] + a_size[0] > b_pos[0]
        && a_pos[1] < b_pos[1] + b_size[1]
        && a_pos[1] + a_size[1] > b_pos[1]
}

fn canonical_rect(a: [f32; 2], b: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let x = a[0].min(b[0]);
    let y = a[1].min(b[1]);
    let w = (a[0] - b[0]).abs();
    let h = (a[1] - b[1]).abs();
    ([x, y], [w, h])
}

/// Returns (waveform_index, is_fade_in) if the cursor is over a fade handle.
fn hit_test_fade_handle(
    waveforms: &[WaveformObject],
    world_pos: [f32; 2],
    camera: &Camera,
) -> Option<(usize, bool)> {
    let handle_sz = waveform::FADE_HANDLE_SIZE / camera.zoom;
    let hit_margin = handle_sz * 0.8;
    for (i, wf) in waveforms.iter().enumerate().rev() {
        let fi_cx = wf.position[0] + wf.fade_in_px;
        let fi_cy = wf.position[1];
        if (world_pos[0] - fi_cx).abs() < hit_margin && (world_pos[1] - fi_cy).abs() < hit_margin {
            return Some((i, true));
        }

        let fo_cx = wf.position[0] + wf.size[0] - wf.fade_out_px;
        let fo_cy = wf.position[1];
        if (world_pos[0] - fo_cx).abs() < hit_margin && (world_pos[1] - fo_cy).abs() < hit_margin {
            return Some((i, false));
        }
    }
    None
}

fn hit_test(
    objects: &[CanvasObject],
    waveforms: &[WaveformObject],
    effect_regions: &[effects::EffectRegion],
    components: &[component::ComponentDef],
    component_instances: &[component::ComponentInstance],
    editing_component: Option<usize>,
    world_pos: [f32; 2],
    camera: &Camera,
) -> Option<HitTarget> {
    // When editing a component, only its waveforms are hittable
    if let Some(ec_idx) = editing_component {
        if let Some(def) = components.get(ec_idx) {
            for &wf_idx in def.waveform_indices.iter().rev() {
                if wf_idx < waveforms.len() {
                    if point_in_rect(world_pos, waveforms[wf_idx].position, waveforms[wf_idx].size) {
                        return Some(HitTarget::Waveform(wf_idx));
                    }
                }
            }
        }
        return None;
    }

    // Instances first (on top)
    for (i, inst) in component_instances.iter().enumerate().rev() {
        if let Some(def) = components.iter().find(|c| c.id == inst.component_id) {
            if point_in_rect(world_pos, inst.position, def.size) {
                return Some(HitTarget::ComponentInstance(i));
            }
        }
    }
    for (i, wf) in waveforms.iter().enumerate().rev() {
        let in_component = components.iter().any(|c| c.waveform_indices.contains(&i));
        if in_component {
            continue;
        }
        if point_in_rect(world_pos, wf.position, wf.size) {
            return Some(HitTarget::Waveform(i));
        }
    }
    for (i, obj) in objects.iter().enumerate().rev() {
        if point_in_rect(world_pos, obj.position, obj.size) {
            return Some(HitTarget::Object(i));
        }
    }
    for (i, def) in components.iter().enumerate().rev() {
        if point_in_rect(world_pos, def.position, def.size) {
            return Some(HitTarget::ComponentDef(i));
        }
    }
    for (i, er) in effect_regions.iter().enumerate().rev() {
        if er.hit_test_border(world_pos, camera) {
            return Some(HitTarget::EffectRegion(i));
        }
    }
    None
}

fn targets_in_rect(
    objects: &[CanvasObject],
    waveforms: &[WaveformObject],
    effect_regions: &[effects::EffectRegion],
    components: &[component::ComponentDef],
    component_instances: &[component::ComponentInstance],
    editing_component: Option<usize>,
    rect_pos: [f32; 2],
    rect_size: [f32; 2],
) -> Vec<HitTarget> {
    let mut result = Vec::new();

    // When editing a component, only its waveforms are selectable via rect
    if let Some(ec_idx) = editing_component {
        if let Some(def) = components.get(ec_idx) {
            for &wf_idx in &def.waveform_indices {
                if wf_idx < waveforms.len() {
                    if rects_overlap(rect_pos, rect_size, waveforms[wf_idx].position, waveforms[wf_idx].size) {
                        result.push(HitTarget::Waveform(wf_idx));
                    }
                }
            }
        }
        return result;
    }

    for (i, obj) in objects.iter().enumerate() {
        if rects_overlap(rect_pos, rect_size, obj.position, obj.size) {
            result.push(HitTarget::Object(i));
        }
    }
    for (i, wf) in waveforms.iter().enumerate() {
        let in_component = components.iter().any(|c| c.waveform_indices.contains(&i));
        if in_component {
            continue;
        }
        if rects_overlap(rect_pos, rect_size, wf.position, wf.size) {
            result.push(HitTarget::Waveform(i));
        }
    }
    for (i, er) in effect_regions.iter().enumerate() {
        if rects_overlap(rect_pos, rect_size, er.position, er.size) {
            result.push(HitTarget::EffectRegion(i));
        }
    }
    for (i, def) in components.iter().enumerate() {
        if rects_overlap(rect_pos, rect_size, def.position, def.size) {
            result.push(HitTarget::ComponentDef(i));
        }
    }
    for (i, inst) in component_instances.iter().enumerate() {
        if let Some(def) = components.iter().find(|c| c.id == inst.component_id) {
            if rects_overlap(rect_pos, rect_size, inst.position, def.size) {
                result.push(HitTarget::ComponentInstance(i));
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Instance building
// ---------------------------------------------------------------------------

struct RenderContext<'a> {
    camera: &'a Camera,
    screen_w: f32,
    screen_h: f32,
    objects: &'a [CanvasObject],
    waveforms: &'a [WaveformObject],
    effect_regions: &'a [effects::EffectRegion],
    hovered: Option<HitTarget>,
    selected: &'a [HitTarget],
    selection_rect: Option<([f32; 2], [f32; 2])>,
    file_hovering: bool,
    playhead_world_x: Option<f32>,
    export_region: Option<&'a ExportRegion>,
    components: &'a [component::ComponentDef],
    component_instances: &'a [component::ComponentInstance],
    editing_component: Option<usize>,
    settings: &'a Settings,
}

fn build_instances(ctx: &RenderContext) -> Vec<InstanceRaw> {
    let mut out = Vec::with_capacity(1024);

    let camera = ctx.camera;
    let world_left = camera.position[0];
    let world_top = camera.position[1];
    let world_right = world_left + ctx.screen_w / camera.zoom;
    let world_bottom = world_top + ctx.screen_h / camera.zoom;

    // --- musical grid ---
    if ctx.settings.grid_enabled {
        let spacing = grid_spacing_for_settings(ctx.settings, camera.zoom);
        let ppb = pixels_per_beat();
        let bar_spacing = ppb * 4.0;
        let line_w = 1.0 / camera.zoom;
        let major_line_w = 2.0 / camera.zoom;
        let bar_line_w = 2.5 / camera.zoom;
        let grid_i = ctx.settings.grid_line_intensity;
        let minor_color = [1.0, 1.0, 1.0, grid_i * 0.20];
        let beat_color = [1.0, 1.0, 1.0, grid_i * 0.38];
        let bar_color = [1.0, 1.0, 1.0, grid_i * 0.60];

        let first_xi = (world_left / spacing).floor() as i64;
        let last_xi = (world_right / spacing).ceil() as i64;
        for i in first_xi..=last_xi {
            let x = i as f32 * spacing;
            let on_bar = (x / bar_spacing).round() * bar_spacing;
            let on_beat = (x / ppb).round() * ppb;
            let is_bar = (x - on_bar).abs() < 0.01;
            let is_beat = !is_bar && (x - on_beat).abs() < 0.01;
            let (w, c) = if is_bar {
                (bar_line_w, bar_color)
            } else if is_beat {
                (major_line_w, beat_color)
            } else {
                (line_w, minor_color)
            };
            out.push(InstanceRaw {
                position: [x - w * 0.5, world_top],
                size: [w, world_bottom - world_top],
                color: c,
                border_radius: 0.0,
            });
        }

        let first_yi = (world_top / spacing).floor() as i64;
        let last_yi = (world_bottom / spacing).ceil() as i64;
        for i in first_yi..=last_yi {
            let y = i as f32 * spacing;
            let is_major = i % 4 == 0;
            let w = if is_major { major_line_w } else { line_w };
            let c = if is_major { beat_color } else { minor_color };
            out.push(InstanceRaw {
                position: [world_left, y - w * 0.5],
                size: [world_right - world_left, w],
                color: c,
                border_radius: 0.0,
            });
        }
    }

    // --- origin axes ---
    let axis_w = 2.0 / camera.zoom;
    if world_top <= 0.0 && world_bottom >= 0.0 {
        out.push(InstanceRaw {
            position: [world_left, -axis_w * 0.5],
            size: [world_right - world_left, axis_w],
            color: [0.85, 0.25, 0.25, 0.5],
            border_radius: 0.0,
        });
    }
    if world_left <= 0.0 && world_right >= 0.0 {
        out.push(InstanceRaw {
            position: [-axis_w * 0.5, world_top],
            size: [axis_w, world_bottom - world_top],
            color: [0.25, 0.85, 0.25, 0.5],
            border_radius: 0.0,
        });
    }

    // --- effect regions (rendered behind everything) ---
    for (i, er) in ctx.effect_regions.iter().enumerate() {
        let is_sel = ctx.selected.contains(&HitTarget::EffectRegion(i));
        let is_hov = ctx.hovered == Some(HitTarget::EffectRegion(i));
        let is_active = ctx.playhead_world_x.map_or(false, |px| {
            px >= er.position[0] && px <= er.position[0] + er.size[0]
        });
        out.extend(effects::build_effect_region_instances(
            er, camera, is_hov, is_sel, is_active,
        ));
    }

    // --- export region ---
    if let Some(er) = ctx.export_region {
        out.push(InstanceRaw {
            position: er.position,
            size: er.size,
            color: EXPORT_FILL_COLOR,
            border_radius: 6.0 / camera.zoom,
        });

        let bw = 1.5 / camera.zoom;
        push_border(&mut out, er.position, er.size, bw, EXPORT_BORDER_COLOR);

        // Dashed top indicator
        let dash_h = 3.0 / camera.zoom;
        let dash_w = 20.0 / camera.zoom;
        let gap = 10.0 / camera.zoom;
        let y = er.position[1] - dash_h - 2.0 / camera.zoom;
        let mut x = er.position[0];
        while x < er.position[0] + er.size[0] {
            let w = dash_w.min(er.position[0] + er.size[0] - x);
            out.push(InstanceRaw {
                position: [x, y],
                size: [w, dash_h],
                color: EXPORT_BORDER_COLOR,
                border_radius: 1.0 / camera.zoom,
            });
            x += dash_w + gap;
        }

        // "Render" pill background in top-left corner
        let pill_w = EXPORT_RENDER_PILL_W / camera.zoom;
        let pill_h = EXPORT_RENDER_PILL_H / camera.zoom;
        let pill_x = er.position[0] + 4.0 / camera.zoom;
        let pill_y = er.position[1] + 4.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [pill_x, pill_y],
            size: [pill_w, pill_h],
            color: EXPORT_RENDER_PILL_COLOR,
            border_radius: pill_h * 0.5,
        });

        // Resize handles at corners
        let handle_sz = 8.0 / camera.zoom;
        for &hx in &[er.position[0] - handle_sz * 0.5, er.position[0] + er.size[0] - handle_sz * 0.5] {
            for &hy in &[er.position[1] - handle_sz * 0.5, er.position[1] + er.size[1] - handle_sz * 0.5] {
                out.push(InstanceRaw {
                    position: [hx, hy],
                    size: [handle_sz, handle_sz],
                    color: [0.20, 0.80, 0.60, 0.9],
                    border_radius: 2.0 / camera.zoom,
                });
            }
        }
    }

    // --- canvas objects ---
    let ci = ctx.settings.color_intensity;
    for (i, obj) in ctx.objects.iter().enumerate() {
        let is_sel = ctx.selected.contains(&HitTarget::Object(i));
        let is_hov = ctx.hovered == Some(HitTarget::Object(i));
        let mut color = obj.color;
        if ci > 0.001 {
            let lum = 0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2];
            let boost = 1.0 + ci * 2.0;
            color[0] = (lum + (color[0] - lum) * boost).clamp(0.0, 1.0);
            color[1] = (lum + (color[1] - lum) * boost).clamp(0.0, 1.0);
            color[2] = (lum + (color[2] - lum) * boost).clamp(0.0, 1.0);
        }
        if is_sel || is_hov {
            color[0] = (color[0] + 0.10).min(1.0);
            color[1] = (color[1] + 0.10).min(1.0);
            color[2] = (color[2] + 0.10).min(1.0);
        }
        out.push(InstanceRaw {
            position: obj.position,
            size: obj.size,
            color,
            border_radius: obj.border_radius,
        });
    }

    // --- waveforms ---
    for (i, wf) in ctx.waveforms.iter().enumerate() {
        let is_sel = ctx.selected.contains(&HitTarget::Waveform(i));
        let is_hov = ctx.hovered == Some(HitTarget::Waveform(i));
        out.extend(waveform::build_waveform_instances(
            wf, camera, world_left, world_right, is_hov, is_sel,
        ));
    }

    // --- component definitions ---
    for (i, def) in ctx.components.iter().enumerate() {
        let is_sel = ctx.selected.contains(&HitTarget::ComponentDef(i));
        let is_hov = ctx.hovered == Some(HitTarget::ComponentDef(i));
        let is_editing = ctx.editing_component == Some(i);
        out.extend(component::build_component_def_instances(
            def, camera, is_hov, is_sel || is_editing,
        ));
    }

    // --- component instances ---
    for (i, inst) in ctx.component_instances.iter().enumerate() {
        if let Some(def) = ctx.components.iter().find(|c| c.id == inst.component_id) {
            let is_sel = ctx.selected.contains(&HitTarget::ComponentInstance(i));
            let is_hov = ctx.hovered == Some(HitTarget::ComponentInstance(i));
            out.extend(component::build_component_instance_instances(
                inst, def, ctx.waveforms, camera, world_left, world_right, is_hov, is_sel,
            ));
        }
    }

    // --- edit mode dimming overlay ---
    if let Some(ec_idx) = ctx.editing_component {
        if let Some(def) = ctx.components.get(ec_idx) {
            // Dim everything outside the component with 4 dark rectangles
            let dim_color = [0.0, 0.0, 0.0, 0.50];
            // Top strip
            out.push(InstanceRaw {
                position: [world_left, world_top],
                size: [world_right - world_left, def.position[1] - world_top],
                color: dim_color,
                border_radius: 0.0,
            });
            // Bottom strip
            let bot_y = def.position[1] + def.size[1];
            out.push(InstanceRaw {
                position: [world_left, bot_y],
                size: [world_right - world_left, world_bottom - bot_y],
                color: dim_color,
                border_radius: 0.0,
            });
            // Left strip
            out.push(InstanceRaw {
                position: [world_left, def.position[1]],
                size: [def.position[0] - world_left, def.size[1]],
                color: dim_color,
                border_radius: 0.0,
            });
            // Right strip
            let right_x = def.position[0] + def.size[0];
            out.push(InstanceRaw {
                position: [right_x, def.position[1]],
                size: [world_right - right_x, def.size[1]],
                color: dim_color,
                border_radius: 0.0,
            });
        }
    }

    // --- selection highlights (rendered on top of everything) ---
    let sel_bw = 2.0 / camera.zoom;
    let handle_sz = 8.0 / camera.zoom;
    for target in ctx.selected {
        let (pos, size) = target_rect(ctx.objects, ctx.waveforms, ctx.effect_regions, ctx.components, ctx.component_instances, target);
        push_border(&mut out, pos, size, sel_bw, SEL_COLOR);

        for &hx in &[pos[0] - handle_sz * 0.5, pos[0] + size[0] - handle_sz * 0.5] {
            for &hy in &[pos[1] - handle_sz * 0.5, pos[1] + size[1] - handle_sz * 0.5] {
                out.push(InstanceRaw {
                    position: [hx, hy],
                    size: [handle_sz, handle_sz],
                    color: [1.0, 1.0, 1.0, 1.0],
                    border_radius: 2.0 / camera.zoom,
                });
            }
        }
    }

    // --- selection rectangle ---
    if let Some((start, current)) = ctx.selection_rect {
        let (rp, rs) = canonical_rect(start, current);
        out.push(InstanceRaw {
            position: rp,
            size: rs,
            color: [0.30, 0.55, 1.0, 0.10],
            border_radius: 0.0,
        });
        let bw = 1.0 / camera.zoom;
        push_border(&mut out, rp, rs, bw, [0.35, 0.65, 1.0, 0.5]);
    }

    // --- playback cursor ---
    if let Some(px) = ctx.playhead_world_x {
        let line_w = 2.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [px - line_w * 0.5, world_top],
            size: [line_w, world_bottom - world_top],
            color: [1.0, 1.0, 1.0, 0.85],
            border_radius: 0.0,
        });
        let head_sz = 10.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [px - head_sz * 0.5, world_top],
            size: [head_sz, head_sz],
            color: [1.0, 1.0, 1.0, 0.95],
            border_radius: 2.0 / camera.zoom,
        });
    }

    // --- file drop zone overlay ---
    if ctx.file_hovering {
        out.push(InstanceRaw {
            position: [world_left, world_top],
            size: [world_right - world_left, world_bottom - world_top],
            color: [0.25, 0.50, 1.0, 0.10],
            border_radius: 0.0,
        });
        let bw = 3.0 / camera.zoom;
        push_border(
            &mut out,
            [world_left, world_top],
            [world_right - world_left, world_bottom - world_top],
            bw,
            [0.35, 0.65, 1.0, 0.7],
        );
    }

    out
}

fn build_waveform_vertices(ctx: &RenderContext) -> Vec<WaveformVertex> {
    let camera = ctx.camera;
    let world_left = camera.position[0];
    let world_right = world_left + ctx.screen_w / camera.zoom;

    let mut verts = Vec::new();
    for (i, wf) in ctx.waveforms.iter().enumerate() {
        let is_sel = ctx.selected.contains(&HitTarget::Waveform(i));
        let is_hov = ctx.hovered == Some(HitTarget::Waveform(i));
        verts.extend(waveform::build_waveform_triangles(
            wf, camera, world_left, world_right, is_hov, is_sel,
        ));
    }
    verts
}

pub(crate) fn push_border(
    out: &mut Vec<InstanceRaw>,
    pos: [f32; 2],
    size: [f32; 2],
    bw: f32,
    color: [f32; 4],
) {
    out.push(InstanceRaw {
        position: pos,
        size: [size[0], bw],
        color,
        border_radius: 0.0,
    });
    out.push(InstanceRaw {
        position: [pos[0], pos[1] + size[1] - bw],
        size: [size[0], bw],
        color,
        border_radius: 0.0,
    });
    out.push(InstanceRaw {
        position: pos,
        size: [bw, size[1]],
        color,
        border_radius: 0.0,
    });
    out.push(InstanceRaw {
        position: [pos[0] + size[0] - bw, pos[1]],
        size: [bw, size[1]],
        color,
        border_radius: 0.0,
    });
}

fn target_rect(
    objects: &[CanvasObject],
    waveforms: &[WaveformObject],
    effect_regions: &[effects::EffectRegion],
    components: &[component::ComponentDef],
    component_instances: &[component::ComponentInstance],
    target: &HitTarget,
) -> ([f32; 2], [f32; 2]) {
    match target {
        HitTarget::Object(i) => (objects[*i].position, objects[*i].size),
        HitTarget::Waveform(i) => (waveforms[*i].position, waveforms[*i].size),
        HitTarget::EffectRegion(i) => (effect_regions[*i].position, effect_regions[*i].size),
        HitTarget::ComponentDef(i) => (components[*i].position, components[*i].size),
        HitTarget::ComponentInstance(i) => {
            let inst = &component_instances[*i];
            let def = components.iter().find(|c| c.id == inst.component_id);
            match def {
                Some(d) => (inst.position, d.size),
                None => (inst.position, [100.0, 100.0]),
            }
        }
    }
}

fn default_objects() -> Vec<CanvasObject> {
    vec![]
}

// ---------------------------------------------------------------------------
// GPU state
// ---------------------------------------------------------------------------

const MAX_WAVEFORM_VERTICES: usize = 131072;

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    waveform_pipeline: wgpu::RenderPipeline,
    waveform_vertex_buffer: wgpu::Buffer,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    screen_camera_buffer: wgpu::Buffer,
    screen_camera_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,

    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    scale_factor: f32,

    browser_text_buffers: Vec<TextBuffer>,
    browser_text_generation: u64,
}

impl Gpu {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("No suitable GPU adapter found");

        log::info!("GPU adapter: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canvas shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let screen_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let screen_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen camera bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_camera_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout, instance_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- waveform pipeline ---
        let wf_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("waveform shader"),
            source: wgpu::ShaderSource::Wgsl(WAVEFORM_SHADER_SRC.into()),
        });

        let wf_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WaveformVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        };

        let waveform_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("waveform pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &wf_shader,
                entry_point: Some("wf_vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wf_vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &wf_shader,
                entry_point: Some("wf_fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let waveform_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("waveform vertex buffer"),
            size: (MAX_WAVEFORM_VERTICES * std::mem::size_of::<WaveformVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad vertices"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad indices"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance buffer"),
            size: (MAX_INSTANCES * std::mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- glyphon text rendering ---
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = glyphon::Cache::new(&device);
        let mut text_atlas = TextAtlas::new(&device, &queue, &cache, surface_format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        let viewport = Viewport::new(&device, &cache);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            waveform_pipeline,
            waveform_vertex_buffer,
            camera_buffer,
            camera_bind_group,
            screen_camera_buffer,
            screen_camera_bind_group,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            font_system,
            swash_cache,
            text_atlas,
            text_renderer,
            viewport,
            scale_factor,
            browser_text_buffers: Vec::new(),
            browser_text_generation: 0,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn render(
        &mut self,
        camera: &Camera,
        world_instances: &[InstanceRaw],
        waveform_vertices: &[WaveformVertex],
        command_palette: Option<&CommandPalette>,
        context_menu: Option<&ContextMenu>,
        sample_browser: Option<&browser::SampleBrowser>,
        plugin_browser: Option<(&browser::PluginBrowserSection, f32)>,
        browser_drag_ghost: Option<(&str, [f32; 2])>,
        is_playing: bool,
        is_recording: bool,
        playback_position: f64,
        export_region: Option<&ExportRegion>,
        effect_regions: &[effects::EffectRegion],
        editing_effect_name: Option<(usize, &str)>,
        waveforms: &[waveform::WaveformObject],
        editing_waveform_name: Option<(usize, &str)>,
        plugin_editor: Option<&plugin_editor::PluginEditorWindow>,
        settings_window: Option<&SettingsWindow>,
        settings: &Settings,
    ) {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        if w < 1.0 || h < 1.0 {
            return;
        }

        let cam_uniform = CameraUniform {
            view_proj: camera.view_proj(w, h),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[cam_uniform]));

        let screen_cam = CameraUniform {
            view_proj: screen_ortho(w, h),
        };
        self.queue.write_buffer(
            &self.screen_camera_buffer,
            0,
            bytemuck::cast_slice(&[screen_cam]),
        );

        let world_count = world_instances.len().min(MAX_INSTANCES);
        self.queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&world_instances[..world_count]),
        );

        let wf_vert_count = waveform_vertices.len().min(MAX_WAVEFORM_VERTICES);
        if wf_vert_count > 0 {
            self.queue.write_buffer(
                &self.waveform_vertex_buffer,
                0,
                bytemuck::cast_slice(&waveform_vertices[..wf_vert_count]),
            );
        }

        // Build overlay instances: browser panel + drag ghost + command palette
        let mut overlay_instances: Vec<InstanceRaw> = Vec::new();

        if let Some(br) = sample_browser {
            overlay_instances.extend(br.build_instances(w, h, self.scale_factor));
        }

        if let Some((pb, y_offset)) = plugin_browser {
            let panel_w = sample_browser.map_or(260.0 * self.scale_factor, |b| b.panel_width(self.scale_factor));
            let clip_top = browser::HEADER_HEIGHT * self.scale_factor;
            overlay_instances.extend(pb.build_instances(panel_w, y_offset, h, self.scale_factor, clip_top));
        }

        if let Some((_, pos)) = browser_drag_ghost {
            overlay_instances.push(InstanceRaw {
                position: [pos[0] - 4.0, pos[1] - 4.0],
                size: [160.0 * self.scale_factor, 24.0 * self.scale_factor],
                color: [0.20, 0.20, 0.28, 0.90],
                border_radius: 4.0 * self.scale_factor,
            });
        }

        if let Some(p) = command_palette {
            overlay_instances.extend(p.build_instances(w, h, self.scale_factor));
        }

        if let Some(cm) = context_menu {
            overlay_instances.extend(cm.build_instances(w, h, self.scale_factor));
        }

        if let Some(sw) = settings_window {
            overlay_instances.extend(sw.build_instances(settings, w, h, self.scale_factor));
        }

        if let Some(pe) = plugin_editor {
            overlay_instances.extend(pe.build_instances(w, h, self.scale_factor));
        }

        overlay_instances.extend(TransportPanel::build_instances(
            w,
            h,
            self.scale_factor,
            is_playing,
            is_recording,
        ));

        overlay_instances.extend(TransportPanel::build_fx_button_instances(
            w,
            h,
            self.scale_factor,
        ));

        overlay_instances.extend(TransportPanel::build_export_button_instances(
            w,
            h,
            self.scale_factor,
        ));

        let overlay_count = overlay_instances.len().min(MAX_INSTANCES - world_count);
        if overlay_count > 0 {
            let offset = (world_count * std::mem::size_of::<InstanceRaw>()) as u64;
            self.queue.write_buffer(
                &self.instance_buffer,
                offset,
                bytemuck::cast_slice(&overlay_instances[..overlay_count]),
            );
        }

        // --- prepare text ---
        let scale = self.scale_factor;
        let mut text_buffers: Vec<TextBuffer> = Vec::new();
        let mut text_meta: Vec<(f32, f32, TextColor, TextBounds)> = Vec::new();

        let full_bounds = TextBounds {
            left: 0,
            top: 0,
            right: w as i32,
            bottom: h as i32,
        };

        // Browser text: shape ALL entries once, positions computed each frame
        if let Some(br) = sample_browser {
            if br.text_generation != self.browser_text_generation {
                self.browser_text_buffers.clear();
                for te in &br.cached_text {
                    let mut buf = TextBuffer::new(
                        &mut self.font_system,
                        Metrics::new(te.font_size, te.line_height),
                    );
                    buf.set_size(
                        &mut self.font_system,
                        Some(te.max_width),
                        Some(te.line_height),
                    );
                    let attrs = Attrs::new()
                        .family(Family::Name(".AppleSystemUIFont"))
                        .weight(glyphon::Weight(te.weight));
                    buf.set_text(&mut self.font_system, &te.text, attrs, Shaping::Advanced);
                    buf.shape_until_scroll(&mut self.font_system, false);
                    self.browser_text_buffers.push(buf);
                }
                self.browser_text_generation = br.text_generation;
            }
        } else if !self.browser_text_buffers.is_empty() {
            self.browser_text_buffers.clear();
        }

        // Plugin browser section text
        if let Some((pb, _)) = plugin_browser {
            let panel_w = sample_browser.map_or(260.0 * scale, |b| b.panel_width(scale));
            let clip_top = browser::HEADER_HEIGHT * scale;
            for te in &pb.cached_text {
                let actual_y = te.base_y;
                if actual_y + te.line_height < clip_top || actual_y > h {
                    continue;
                }
                let mut buf = TextBuffer::new(
                    &mut self.font_system,
                    Metrics::new(te.font_size, te.line_height),
                );
                buf.set_size(&mut self.font_system, Some(te.max_width), Some(te.line_height));
                let attrs = Attrs::new()
                    .family(Family::Name(".AppleSystemUIFont"))
                    .weight(glyphon::Weight(te.weight));
                buf.set_text(&mut self.font_system, &te.text, attrs, Shaping::Advanced);
                buf.shape_until_scroll(&mut self.font_system, false);
                text_buffers.push(buf);
                text_meta.push((
                    te.x,
                    actual_y,
                    TextColor::rgba(te.color[0], te.color[1], te.color[2], te.color[3]),
                    TextBounds {
                        left: 0,
                        top: (actual_y.max(clip_top)) as i32,
                        right: (panel_w - 8.0 * scale) as i32,
                        bottom: (actual_y + te.line_height) as i32,
                    },
                ));
            }
        }

        // Drag ghost text
        if let Some((label, pos)) = browser_drag_ghost {
            let font_sz = 12.0 * scale;
            let line_h = 16.0 * scale;
            let mut buf = TextBuffer::new(&mut self.font_system, Metrics::new(font_sz, line_h));
            buf.set_size(&mut self.font_system, Some(150.0 * scale), Some(line_h));
            buf.set_text(
                &mut self.font_system,
                label,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                pos[0] + 4.0 * scale,
                pos[1] - 4.0 + (24.0 * scale - line_h) * 0.5,
                TextColor::rgb(220, 220, 230),
                full_bounds,
            ));
        }

        if let Some(palette) = command_palette {
            let (ppos, _psize) = palette.palette_rect(w, h, scale);
            let margin = PALETTE_PADDING * scale;
            let list_top = ppos[1] + PALETTE_INPUT_HEIGHT * scale + 1.0 * scale;

            // Search input text (or placeholder)
            let (display_text, search_color) = match palette.mode {
                PaletteMode::VolumeFader => {
                    ("Master Volume", TextColor::rgb(235, 235, 240))
                }
                _ if palette.search_text.is_empty() => {
                    ("Search", TextColor::rgba(140, 140, 150, 160))
                }
                _ => {
                    (palette.search_text.as_str(), TextColor::rgb(235, 235, 240))
                }
            };
            let sfont = 15.0 * scale;
            let sline = 22.0 * scale;
            let mut buf = TextBuffer::new(&mut self.font_system, Metrics::new(sfont, sline));
            buf.set_size(
                &mut self.font_system,
                Some(PALETTE_WIDTH * scale - 60.0 * scale),
                Some(PALETTE_INPUT_HEIGHT * scale),
            );
            buf.set_text(
                &mut self.font_system,
                display_text,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                ppos[0] + 36.0 * scale,
                ppos[1] + (PALETTE_INPUT_HEIGHT * scale - sline) * 0.5,
                search_color,
                full_bounds,
            ));

            match palette.mode {
                PaletteMode::VolumeFader => {
                    let pad = 16.0 * scale;
                    let track_y = list_top + 36.0 * scale;
                    let track_h = 6.0 * scale;
                    let rms_y = track_y + track_h + 22.0 * scale;

                    let pct = (palette.fader_value * 100.0) as u32;
                    let vol_text = format!("{}%", pct);
                    let label_font = 13.0 * scale;
                    let label_line = 18.0 * scale;
                    let mut buf = TextBuffer::new(
                        &mut self.font_system,
                        Metrics::new(label_font, label_line),
                    );
                    buf.set_size(
                        &mut self.font_system,
                        Some(PALETTE_WIDTH * scale - margin * 2.0),
                        Some(20.0 * scale),
                    );
                    buf.set_text(
                        &mut self.font_system,
                        &vol_text,
                        Attrs::new().family(Family::SansSerif),
                        Shaping::Advanced,
                    );
                    buf.shape_until_scroll(&mut self.font_system, false);
                    text_buffers.push(buf);
                    text_meta.push((
                        ppos[0] + margin + pad,
                        list_top + 14.0 * scale,
                        TextColor::rgba(200, 200, 210, 220),
                        full_bounds,
                    ));

                    let db_val = if palette.fader_rms > 0.0001 {
                        20.0 * palette.fader_rms.log10()
                    } else {
                        -60.0
                    };
                    let rms_text = format!("RMS: {:.1} dB", db_val);
                    let small_font = 11.0 * scale;
                    let small_line = 15.0 * scale;
                    let mut buf = TextBuffer::new(
                        &mut self.font_system,
                        Metrics::new(small_font, small_line),
                    );
                    buf.set_size(
                        &mut self.font_system,
                        Some(PALETTE_WIDTH * scale - margin * 2.0),
                        Some(16.0 * scale),
                    );
                    buf.set_text(
                        &mut self.font_system,
                        &rms_text,
                        Attrs::new().family(Family::SansSerif),
                        Shaping::Advanced,
                    );
                    buf.shape_until_scroll(&mut self.font_system, false);
                    text_buffers.push(buf);
                    text_meta.push((
                        ppos[0] + margin + pad,
                        rms_y + 8.0 * scale,
                        TextColor::rgba(140, 140, 150, 180),
                        full_bounds,
                    ));
                }
                PaletteMode::Commands => {
                    let sect_font = 11.0 * scale;
                    let sect_line = 16.0 * scale;
                    let ifont = 13.5 * scale;
                    let iline = 20.0 * scale;
                    let shortcut_font = 12.0 * scale;
                    let shortcut_line = 17.0 * scale;

                    let mut y = list_top;
                    for row in palette.visible_rows() {
                        match row {
                            PaletteRow::Section(label) => {
                                let mut buf = TextBuffer::new(
                                    &mut self.font_system,
                                    Metrics::new(sect_font, sect_line),
                                );
                                buf.set_size(
                                    &mut self.font_system,
                                    Some(PALETTE_WIDTH * scale - margin * 4.0),
                                    Some(PALETTE_SECTION_HEIGHT * scale),
                                );
                                buf.set_text(
                                    &mut self.font_system,
                                    label,
                                    Attrs::new().family(Family::SansSerif),
                                    Shaping::Advanced,
                                );
                                buf.shape_until_scroll(&mut self.font_system, false);
                                text_buffers.push(buf);
                                text_meta.push((
                                    ppos[0] + margin + 12.0 * scale,
                                    y + (PALETTE_SECTION_HEIGHT * scale - sect_line) * 0.5
                                        + 2.0 * scale,
                                    TextColor::rgba(120, 140, 170, 200),
                                    full_bounds,
                                ));
                                y += PALETTE_SECTION_HEIGHT * scale;
                            }
                            PaletteRow::Command(ci) => {
                                let cmd = &COMMANDS[*ci];

                                let mut buf = TextBuffer::new(
                                    &mut self.font_system,
                                    Metrics::new(ifont, iline),
                                );
                                buf.set_size(
                                    &mut self.font_system,
                                    Some(PALETTE_WIDTH * scale * 0.65),
                                    Some(PALETTE_ITEM_HEIGHT * scale),
                                );
                                buf.set_text(
                                    &mut self.font_system,
                                    cmd.name,
                                    Attrs::new().family(Family::SansSerif),
                                    Shaping::Advanced,
                                );
                                buf.shape_until_scroll(&mut self.font_system, false);
                                text_buffers.push(buf);
                                text_meta.push((
                                    ppos[0] + margin + 12.0 * scale,
                                    y + (PALETTE_ITEM_HEIGHT * scale - iline) * 0.5,
                                    TextColor::rgb(215, 215, 222),
                                    full_bounds,
                                ));

                                if !cmd.shortcut.is_empty() {
                                    let mut buf = TextBuffer::new(
                                        &mut self.font_system,
                                        Metrics::new(shortcut_font, shortcut_line),
                                    );
                                    buf.set_size(
                                        &mut self.font_system,
                                        Some(80.0 * scale),
                                        Some(PALETTE_ITEM_HEIGHT * scale),
                                    );
                                    buf.set_text(
                                        &mut self.font_system,
                                        cmd.shortcut,
                                        Attrs::new().family(Family::SansSerif),
                                        Shaping::Advanced,
                                    );
                                    buf.shape_until_scroll(&mut self.font_system, false);
                                    text_buffers.push(buf);
                                    text_meta.push((
                                        ppos[0] + PALETTE_WIDTH * scale - margin - 70.0 * scale,
                                        y + (PALETTE_ITEM_HEIGHT * scale - shortcut_line) * 0.5,
                                        TextColor::rgba(120, 120, 135, 180),
                                        full_bounds,
                                    ));
                                }

                                y += PALETTE_ITEM_HEIGHT * scale;
                            }
                        }
                    }
                }
            }
        }

        if let Some(cm) = context_menu {
            let (mpos, _msize) = cm.menu_rect(w, h, scale);
            let pad = CTX_MENU_PADDING * scale;
            let label_font = 13.0 * scale;
            let label_line = 18.0 * scale;
            let shortcut_font = 12.0 * scale;
            let shortcut_line = 17.0 * scale;
            let section_font = 11.0 * scale;
            let section_line = 15.0 * scale;
            let has_any_checked = cm.entries.iter().any(|e| matches!(e, ContextMenuEntry::Item(it) if it.checked));
            let check_indent = if has_any_checked { 16.0 * scale } else { 0.0 };

            let mut y = mpos[1] + pad;
            for entry in &cm.entries {
                match entry {
                    ContextMenuEntry::Item(item) => {
                        let mut buf = TextBuffer::new(
                            &mut self.font_system,
                            Metrics::new(label_font, label_line),
                        );
                        buf.set_size(
                            &mut self.font_system,
                            Some(CTX_MENU_WIDTH * scale * 0.55),
                            Some(CTX_MENU_ITEM_HEIGHT * scale),
                        );
                        buf.set_text(
                            &mut self.font_system,
                            item.label,
                            Attrs::new().family(Family::SansSerif),
                            Shaping::Advanced,
                        );
                        buf.shape_until_scroll(&mut self.font_system, false);
                        text_buffers.push(buf);
                        text_meta.push((
                            mpos[0] + pad + 10.0 * scale + check_indent,
                            y + (CTX_MENU_ITEM_HEIGHT * scale - label_line) * 0.5,
                            TextColor::rgb(220, 220, 228),
                            full_bounds,
                        ));

                        if !item.shortcut.is_empty() {
                            let mut buf = TextBuffer::new(
                                &mut self.font_system,
                                Metrics::new(shortcut_font, shortcut_line),
                            );
                            buf.set_size(
                                &mut self.font_system,
                                Some(60.0 * scale),
                                Some(CTX_MENU_ITEM_HEIGHT * scale),
                            );
                            buf.set_text(
                                &mut self.font_system,
                                item.shortcut,
                                Attrs::new().family(Family::SansSerif),
                                Shaping::Advanced,
                            );
                            buf.shape_until_scroll(&mut self.font_system, false);
                            text_buffers.push(buf);
                            text_meta.push((
                                mpos[0] + CTX_MENU_WIDTH * scale - pad - 50.0 * scale,
                                y + (CTX_MENU_ITEM_HEIGHT * scale - shortcut_line) * 0.5,
                                TextColor::rgba(120, 120, 135, 180),
                                full_bounds,
                            ));
                        }

                        y += CTX_MENU_ITEM_HEIGHT * scale;
                    }
                    ContextMenuEntry::Separator => {
                        y += CTX_MENU_SEPARATOR_HEIGHT * scale;
                    }
                    ContextMenuEntry::SectionHeader(label) => {
                        let mut buf = TextBuffer::new(
                            &mut self.font_system,
                            Metrics::new(section_font, section_line),
                        );
                        buf.set_size(
                            &mut self.font_system,
                            Some(CTX_MENU_WIDTH * scale * 0.8),
                            Some(CTX_MENU_SECTION_HEIGHT * scale),
                        );
                        buf.set_text(
                            &mut self.font_system,
                            label,
                            Attrs::new().family(Family::SansSerif),
                            Shaping::Advanced,
                        );
                        buf.shape_until_scroll(&mut self.font_system, false);
                        text_buffers.push(buf);
                        text_meta.push((
                            mpos[0] + pad + 10.0 * scale,
                            y + (CTX_MENU_SECTION_HEIGHT * scale - section_line) * 0.5,
                            TextColor::rgba(150, 150, 160, 200),
                            full_bounds,
                        ));
                        y += CTX_MENU_SECTION_HEIGHT * scale;
                    }
                }
            }
        }

        // Plugin editor text
        if let Some(pe) = plugin_editor {
            for te in pe.get_text_entries(w, h, scale) {
                let mut buf = TextBuffer::new(
                    &mut self.font_system,
                    Metrics::new(te.font_size, te.line_height),
                );
                buf.set_size(&mut self.font_system, Some(te.max_width), Some(te.line_height * 2.0));
                let attrs = Attrs::new()
                    .family(Family::Name(".AppleSystemUIFont"))
                    .weight(glyphon::Weight(te.weight));
                buf.set_text(&mut self.font_system, &te.text, attrs, Shaping::Advanced);
                buf.shape_until_scroll(&mut self.font_system, false);
                text_buffers.push(buf);
                text_meta.push((
                    te.x,
                    te.y,
                    TextColor::rgba(te.color[0], te.color[1], te.color[2], te.color[3]),
                    full_bounds,
                ));
            }
        }

        // Settings window text
        if let Some(sw) = settings_window {
            for te in sw.get_text_entries(settings, w, h, scale) {
                let mut buf = TextBuffer::new(
                    &mut self.font_system,
                    Metrics::new(te.font_size, te.line_height),
                );
                buf.set_size(&mut self.font_system, Some(300.0 * scale), Some(te.line_height * 2.0));
                let attrs = Attrs::new()
                    .family(Family::Name(".AppleSystemUIFont"))
                    .weight(glyphon::Weight(te.weight));
                buf.set_text(&mut self.font_system, &te.text, attrs, Shaping::Advanced);
                buf.shape_until_scroll(&mut self.font_system, false);
                text_buffers.push(buf);
                text_meta.push((
                    te.x,
                    te.y,
                    TextColor::rgba(te.color[0], te.color[1], te.color[2], te.color[3]),
                    full_bounds,
                ));
            }
        }

        // Export region "Render" label with duration (world-space → screen-space)
        if let Some(er) = export_region {
            let pill_world_x = er.position[0] + 4.0 / camera.zoom;
            let pill_world_y = er.position[1] + 4.0 / camera.zoom;
            let pill_screen_x = (pill_world_x - camera.position[0]) * camera.zoom;
            let pill_screen_y = (pill_world_y - camera.position[1]) * camera.zoom;
            let pill_w_screen = EXPORT_RENDER_PILL_W;
            let pill_h_screen = EXPORT_RENDER_PILL_H;

            let duration_secs = er.size[0] as f64 / PIXELS_PER_SECOND as f64;
            let label_text = if duration_secs < 60.0 {
                format!("Render  {:.1}s", duration_secs)
            } else {
                let mins = (duration_secs / 60.0) as u32;
                let secs = duration_secs % 60.0;
                format!("Render  {}:{:04.1}", mins, secs)
            };

            let label_font = 11.0 * scale;
            let label_line = 16.0 * scale;
            let mut buf = TextBuffer::new(&mut self.font_system, Metrics::new(label_font, label_line));
            buf.set_size(
                &mut self.font_system,
                Some(pill_w_screen),
                Some(pill_h_screen),
            );
            buf.set_text(
                &mut self.font_system,
                &label_text,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                pill_screen_x + 8.0,
                pill_screen_y + (pill_h_screen - label_line) * 0.5,
                TextColor::rgb(255, 255, 255),
                full_bounds,
            ));
        }

        // Effect region name labels (world-space -> screen-space)
        for (er_idx, er) in effect_regions.iter().enumerate() {
            let region_screen_w = er.size[0] * camera.zoom;
            if region_screen_w < 30.0 {
                continue;
            }

            let pad = 6.0 / camera.zoom;
            let name_x_world = er.position[0] + pad;
            let name_y_world = er.position[1] - 18.0 / camera.zoom;
            let name_screen_x = (name_x_world - camera.position[0]) * camera.zoom;
            let name_screen_y = (name_y_world - camera.position[1]) * camera.zoom;

            let display_name = if let Some((idx, ref text)) = editing_effect_name {
                if idx == er_idx { format!("{}|", text) } else { er.name.clone() }
            } else {
                er.name.clone()
            };

            let name_font = 10.0 * scale;
            let name_line = 14.0 * scale;
            let mut buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(name_font, name_line),
            );
            let max_text_w = (region_screen_w - 12.0 * scale).max(20.0);
            buf.set_size(
                &mut self.font_system,
                Some(max_text_w),
                Some(name_line),
            );
            let is_editing = editing_effect_name.map_or(false, |(idx, _)| idx == er_idx);
            let alpha = if is_editing { 255 } else { 180 };
            let attrs = Attrs::new()
                .family(Family::Name(".AppleSystemUIFont"))
                .weight(glyphon::Weight(500));
            buf.set_text(&mut self.font_system, &display_name, attrs, Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                name_screen_x,
                name_screen_y,
                TextColor::rgba(255, 255, 255, alpha),
                full_bounds,
            ));
        }

        // Waveform sample name labels (world-space -> screen-space)
        for (wf_idx, wf) in waveforms.iter().enumerate() {
            let clip_screen_w = wf.size[0] * camera.zoom;
            if clip_screen_w < 30.0 {
                continue;
            }

            let pad = 6.0 / camera.zoom;
            let name_x_world = wf.position[0] + pad;
            let name_y_world = wf.position[1] + pad;
            let name_screen_x = (name_x_world - camera.position[0]) * camera.zoom;
            let name_screen_y = (name_y_world - camera.position[1]) * camera.zoom;

            let display_name = if let Some((idx, ref text)) = editing_waveform_name {
                if idx == wf_idx { format!("{}|", text) } else { wf.filename.clone() }
            } else {
                wf.filename.clone()
            };

            let name_font = 10.0 * scale;
            let name_line = 14.0 * scale;
            let mut buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(name_font, name_line),
            );
            let max_text_w = (clip_screen_w - 12.0 * scale).max(20.0);
            buf.set_size(
                &mut self.font_system,
                Some(max_text_w),
                Some(name_line),
            );
            let is_editing = editing_waveform_name.map_or(false, |(idx, _)| idx == wf_idx);
            let alpha = if is_editing { 255 } else { 180 };
            let attrs = Attrs::new()
                .family(Family::Name(".AppleSystemUIFont"))
                .weight(glyphon::Weight(500));
            buf.set_text(&mut self.font_system, &display_name, attrs, Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                name_screen_x,
                name_screen_y,
                TextColor::rgba(255, 255, 255, alpha),
                full_bounds,
            ));
        }

        // Effect region plugin name labels (world-space -> screen-space)
        for er in effect_regions {
            let labels = effects::plugin_label_rects(er, camera);
            for (i, rect) in labels.iter().enumerate() {
                let screen_x = (rect.position[0] - camera.position[0]) * camera.zoom;
                let screen_y = (rect.position[1] - camera.position[1]) * camera.zoom;
                let pill_w_screen = rect.size[0] * camera.zoom;
                let pill_h_screen = rect.size[1] * camera.zoom;

                let name = &er.chain[i].plugin_name;
                let label_font = 10.0 * scale;
                let label_line = 14.0 * scale;
                let mut buf = TextBuffer::new(
                    &mut self.font_system,
                    Metrics::new(label_font, label_line),
                );
                buf.set_size(
                    &mut self.font_system,
                    Some(pill_w_screen - 8.0),
                    Some(pill_h_screen),
                );
                let attrs = Attrs::new()
                    .family(Family::Name(".AppleSystemUIFont"))
                    .weight(glyphon::Weight(500));
                buf.set_text(&mut self.font_system, name, attrs, Shaping::Advanced);
                buf.shape_until_scroll(&mut self.font_system, false);
                text_buffers.push(buf);
                text_meta.push((
                    screen_x + 4.0 * scale,
                    screen_y + (pill_h_screen - label_line) * 0.5,
                    TextColor::rgba(255, 255, 255, 220),
                    full_bounds,
                ));
            }
        }

        // Transport panel time text
        {
            let (tp_pos, tp_size) = TransportPanel::panel_rect(w, h, scale);
            let time_str = format_playback_time(playback_position);
            let tfont = 13.0 * scale;
            let tline = 18.0 * scale;
            let mut buf = TextBuffer::new(&mut self.font_system, Metrics::new(tfont, tline));
            buf.set_size(
                &mut self.font_system,
                Some(TRANSPORT_WIDTH * scale * 0.6),
                Some(tline),
            );
            buf.set_text(
                &mut self.font_system,
                &time_str,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                tp_pos[0] + 38.0 * scale,
                tp_pos[1] + (tp_size[1] - tline) * 0.5,
                TextColor::rgba(220, 220, 230, 220),
                full_bounds,
            ));
        }

        // Transport panel BPM text
        {
            let (tp_pos, tp_size) = TransportPanel::panel_rect(w, h, scale);
            let bpm_str = format!("{} bpm", DEFAULT_BPM as u32);
            let tfont = 13.0 * scale;
            let tline = 18.0 * scale;
            let mut buf = TextBuffer::new(&mut self.font_system, Metrics::new(tfont, tline));
            buf.set_size(&mut self.font_system, Some(80.0 * scale), Some(tline));
            buf.set_text(
                &mut self.font_system,
                &bpm_str,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            text_buffers.push(buf);
            text_meta.push((
                tp_pos[0] + tp_size[0] - 80.0 * scale,
                tp_pos[1] + (tp_size[1] - tline) * 0.5,
                TextColor::rgba(220, 220, 230, 220),
                full_bounds,
            ));
        }

        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );

        let mut browser_text_areas: Vec<TextArea> = Vec::new();
        if let Some(br) = sample_browser {
            let panel_w = br.panel_width(scale);
            let header_h = browser::HEADER_HEIGHT * scale;
            for (idx, te) in br.cached_text.iter().enumerate() {
                if idx >= self.browser_text_buffers.len() {
                    break;
                }
                let actual_y = if te.is_header {
                    te.base_y
                } else {
                    te.base_y - br.scroll_offset
                };
                if !te.is_header && (actual_y + te.line_height < header_h || actual_y > h) {
                    continue;
                }
                let clip_top = if actual_y < header_h {
                    header_h
                } else {
                    actual_y
                };
                browser_text_areas.push(TextArea {
                    buffer: &self.browser_text_buffers[idx],
                    left: te.x,
                    top: actual_y,
                    scale: 1.0,
                    default_color: TextColor::rgba(
                        te.color[0],
                        te.color[1],
                        te.color[2],
                        te.color[3],
                    ),
                    bounds: TextBounds {
                        left: 0,
                        top: clip_top as i32,
                        right: (panel_w - 8.0 * scale) as i32,
                        bottom: (actual_y + te.line_height) as i32,
                    },
                    custom_glyphs: &[],
                });
            }
        }

        let other_areas = text_buffers.iter().zip(text_meta.iter()).map(
            |(buffer, &(left, top, color, bounds))| TextArea {
                buffer,
                left,
                top,
                scale: 1.0,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            },
        );

        let text_areas: Vec<TextArea> = browser_text_areas.into_iter().chain(other_areas).collect();

        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.text_atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .unwrap();

        // --- render pass ---
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                log::error!("Surface error: {e:?}");
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.09 * settings.brightness as f64,
                            g: 0.09 * settings.brightness as f64,
                            b: 0.12 * settings.brightness as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..QUAD_INDICES.len() as u32, 0, 0..world_count as u32);

            if wf_vert_count > 0 {
                pass.set_pipeline(&self.waveform_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.waveform_vertex_buffer.slice(..));
                pass.draw(0..wf_vert_count as u32, 0..1);
            }

            if overlay_count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.screen_camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(
                    0..QUAD_INDICES.len() as u32,
                    0,
                    world_count as u32..(world_count + overlay_count) as u32,
                );
            }

            self.text_renderer
                .render(&self.text_atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

struct MenuState {
    menu: muda::Menu,
    new_project: MenuId,
    save_project: MenuId,
    settings: MenuId,
    open_project_items: Vec<(MenuId, String)>,
    open_submenu: MudaSubmenu,
    initialized: bool,
}

struct App {
    gpu: Option<Gpu>,
    camera: Camera,
    objects: Vec<CanvasObject>,
    waveforms: Vec<WaveformObject>,
    audio_clips: Vec<AudioClipData>,
    audio_engine: Option<AudioEngine>,
    recorder: Option<AudioRecorder>,
    recording_waveform_idx: Option<usize>,
    last_canvas_click_world: [f32; 2],
    selected: Vec<HitTarget>,
    drag: DragState,
    mouse_pos: [f32; 2],
    hovered: Option<HitTarget>,
    fade_handle_hovered: Option<(usize, bool)>,
    file_hovering: bool,
    modifiers: ModifiersState,
    command_palette: Option<CommandPalette>,
    context_menu: Option<ContextMenu>,
    sample_browser: browser::SampleBrowser,
    storage: Option<Storage>,
    has_saved_state: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    current_project_id: String,
    current_project_name: String,
    effect_regions: Vec<effects::EffectRegion>,
    components: Vec<component::ComponentDef>,
    component_instances: Vec<component::ComponentInstance>,
    next_component_id: component::ComponentId,
    plugin_registry: effects::PluginRegistry,
    plugin_browser: browser::PluginBrowserSection,
    export_region: Option<ExportRegion>,
    export_hover: ExportHover,
    component_def_hover: ComponentDefHover,
    effect_region_hover: EffectRegionHover,
    editing_component: Option<usize>,
    editing_effect_name: Option<(usize, String)>,
    editing_waveform_name: Option<(usize, String)>,
    last_click_time: std::time::Instant,
    last_click_world: [f32; 2],
    clipboard: Clipboard,
    settings: Settings,
    settings_window: Option<SettingsWindow>,
    plugin_editor: Option<plugin_editor::PluginEditorWindow>,
    menu_state: Option<MenuState>,
}

impl App {
    fn new() -> Self {
        let project_id = "default".to_string();
        let db_path = default_db_path();
        println!("  Database: {}", db_path.display());

        let storage = Storage::open(&db_path);

        if let Some(s) = &storage {
            let projects = s.list_projects();
            if !projects.is_empty() {
                println!("  Projects:");
                for p in &projects {
                    let marker = if p.project_id == project_id { " *" } else { "" };
                    println!("    - {} ({}){}", p.name, p.project_id, marker);
                }
            }
        }

        let loaded = storage.as_ref().and_then(|s| s.load(&project_id));
        let has_saved_state = loaded.is_some();
        let (
            camera,
            objects,
            waveforms,
            project_name,
            browser_folders,
            browser_width,
            browser_visible,
            browser_expanded,
            stored_effect_regions,
            stored_components,
            stored_component_instances,
        ) = match loaded {
            Some(state) => {
                println!(
                    "  Loaded project '{}' ({} objects, {} waveforms, {} effect regions)",
                    state.name,
                    state.objects.len(),
                    state.waveforms.len(),
                    state.effect_regions.len(),
                );
                let cam = Camera {
                    position: state.camera_position,
                    zoom: state.camera_zoom,
                };
                let name = state.name.clone();
                let folders: Vec<PathBuf> =
                    state.browser_folders.iter().map(PathBuf::from).collect();
                let bw = if state.browser_width > 0.0 {
                    state.browser_width
                } else {
                    260.0
                };
                let expanded: HashSet<PathBuf> =
                    state.browser_expanded.iter().map(PathBuf::from).collect();
                let waveforms: Vec<WaveformObject> = state.waveforms.into_iter().map(|sw| {
                    WaveformObject {
                        position: sw.position,
                        size: sw.size,
                        color: sw.color,
                        border_radius: sw.border_radius,
                        left_samples: Arc::new(Vec::new()),
                        right_samples: Arc::new(Vec::new()),
                        left_peaks: Arc::new(WaveformPeaks::empty()),
                        right_peaks: Arc::new(WaveformPeaks::empty()),
                        sample_rate: sw.sample_rate,
                        filename: sw.filename,
                        fade_in_px: sw.fade_in_px,
                        fade_out_px: sw.fade_out_px,
                    }
                }).collect();
                (
                    cam,
                    state.objects,
                    waveforms,
                    name,
                    folders,
                    bw,
                    state.browser_visible,
                    Some(expanded),
                    state.effect_regions,
                    state.components,
                    state.component_instances,
                )
            }
            None => {
                println!("  No saved project found, starting fresh");
                (
                    Camera::new(),
                    default_objects(),
                    Vec::new(),
                    "Untitled".to_string(),
                    Vec::new(),
                    260.0,
                    false,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        };

        let mut sample_browser = if let Some(expanded) = browser_expanded {
            browser::SampleBrowser::from_state(browser_folders, expanded, browser_visible)
        } else {
            browser::SampleBrowser::from_folders(browser_folders)
        };
        sample_browser.width = browser_width;

        let mut settings = Settings::load();

        let device_name = if settings.audio_output_device == "No Device" {
            None
        } else {
            Some(settings.audio_output_device.as_str())
        };
        let audio_engine = AudioEngine::new_with_device(device_name);
        if let Some(ref engine) = audio_engine {
            let actual = engine.device_name();
            if settings.audio_output_device != actual {
                println!("  Correcting stale output device setting: '{}' -> '{}'", settings.audio_output_device, actual);
                settings.audio_output_device = actual.to_string();
                settings.save();
            }
        } else {
            println!("  Warning: no audio output device found");
        }

        let recorder = AudioRecorder::new();
        if recorder.is_none() {
            println!("  Warning: no audio input device found");
        }

        let plugin_registry = effects::PluginRegistry::new();

        // Restore effect region geometry; plugins will be loaded lazily on first scan
        let restored_effect_regions: Vec<effects::EffectRegion> = stored_effect_regions.into_iter().map(|ser| {
            let mut region = effects::EffectRegion::new(ser.position, ser.size);
            region.name = ser.name;
            for (pid, pname) in ser.plugin_ids.iter().zip(ser.plugin_names.iter()) {
                region.chain.push(effects::PluginSlot {
                    plugin_id: pid.clone(),
                    plugin_name: pname.clone(),
                    plugin_path: std::path::PathBuf::new(),
                    bypass: false,
                    instance: Arc::new(std::sync::Mutex::new(None)),
                });
            }
            region
        }).collect();

        let plugin_browser = browser::PluginBrowserSection::new();

        let restored_components: Vec<component::ComponentDef> = stored_components.into_iter().map(|sc| {
            component::ComponentDef {
                id: sc.id,
                name: sc.name,
                position: sc.position,
                size: sc.size,
                waveform_indices: sc.waveform_indices.iter().map(|&i| i as usize).collect(),
            }
        }).collect();
        let restored_instances: Vec<component::ComponentInstance> = stored_component_instances.into_iter().map(|si| {
            component::ComponentInstance {
                component_id: si.component_id,
                position: si.position,
            }
        }).collect();
        let next_component_id = restored_components.iter().map(|c| c.id).max().unwrap_or(0) + 1;

        Self {
            gpu: None,
            camera,
            objects,
            waveforms,
            audio_clips: Vec::new(),
            audio_engine,
            recorder,
            recording_waveform_idx: None,
            last_canvas_click_world: [0.0; 2],
            selected: Vec::new(),
            drag: DragState::None,
            mouse_pos: [0.0; 2],
            hovered: None,
            fade_handle_hovered: None,
            file_hovering: false,
            modifiers: ModifiersState::empty(),
            command_palette: None,
            context_menu: None,
            sample_browser,
            storage,
            has_saved_state,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_project_id: project_id,
            current_project_name: project_name,
            effect_regions: restored_effect_regions,
            components: restored_components,
            component_instances: restored_instances,
            next_component_id,
            plugin_registry,
            plugin_browser,
            export_region: None,
            export_hover: ExportHover::None,
            component_def_hover: ComponentDefHover::None,
            effect_region_hover: EffectRegionHover::None,
            editing_component: None,
            editing_effect_name: None,
            editing_waveform_name: None,
            last_click_time: std::time::Instant::now(),
            last_click_world: [0.0; 2],
            clipboard: Clipboard::new(),
            settings,
            settings_window: None,
            plugin_editor: None,
            menu_state: None,
        }
    }

    fn save_project(&self) {
        if let Some(storage) = &self.storage {
            let stored_regions: Vec<storage::StoredEffectRegion> = self.effect_regions.iter().map(|er| {
                storage::StoredEffectRegion {
                    position: er.position,
                    size: er.size,
                    plugin_ids: er.chain.iter().map(|s| s.plugin_id.clone()).collect(),
                    plugin_names: er.chain.iter().map(|s| s.plugin_name.clone()).collect(),
                    name: er.name.clone(),
                }
            }).collect();

            let stored_components: Vec<storage::StoredComponent> = self.components.iter().map(|c| {
                storage::StoredComponent {
                    id: c.id,
                    name: c.name.clone(),
                    position: c.position,
                    size: c.size,
                    waveform_indices: c.waveform_indices.iter().map(|&i| i as u64).collect(),
                }
            }).collect();
            let stored_instances: Vec<storage::StoredComponentInstance> = self.component_instances.iter().map(|inst| {
                storage::StoredComponentInstance {
                    component_id: inst.component_id,
                    position: inst.position,
                }
            }).collect();

            let stored_waveforms: Vec<storage::StoredWaveform> = self.waveforms.iter().map(|wf| {
                storage::StoredWaveform {
                    position: wf.position,
                    size: wf.size,
                    color: wf.color,
                    border_radius: wf.border_radius,
                    filename: wf.filename.clone(),
                    fade_in_px: wf.fade_in_px,
                    fade_out_px: wf.fade_out_px,
                    sample_rate: wf.sample_rate,
                }
            }).collect();

            let state = ProjectState {
                name: self.current_project_name.clone(),
                camera_position: self.camera.position,
                camera_zoom: self.camera.zoom,
                objects: self.objects.clone(),
                waveforms: stored_waveforms,
                browser_folders: self
                    .sample_browser
                    .root_folders
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                browser_width: self.sample_browser.width,
                browser_visible: self.sample_browser.visible,
                browser_expanded: self
                    .sample_browser
                    .expanded
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                effect_regions: stored_regions,
                components: stored_components,
                component_instances: stored_instances,
            };
            storage.save(&self.current_project_id, state);
            println!("Project '{}' saved", self.current_project_name);
        }
    }

    fn handle_menu_event(&mut self, id: MenuId) {
        let menu = match &self.menu_state {
            Some(m) => m,
            None => return,
        };

        if id == menu.new_project {
            self.new_project();
            self.refresh_open_project_menu();
            self.request_redraw();
        } else if id == menu.save_project {
            self.save_project();
            self.refresh_open_project_menu();
        } else if id == menu.settings {
            self.command_palette = None;
            self.context_menu = None;
            self.settings_window = if self.settings_window.is_some() {
                None
            } else {
                Some(SettingsWindow::new())
            };
            self.request_redraw();
        } else if let Some(project_id) = menu
            .open_project_items
            .iter()
            .find(|(mid, _)| *mid == id)
            .map(|(_, pid)| pid.clone())
        {
            self.load_project(&project_id);
            self.refresh_open_project_menu();
            self.request_redraw();
        }
    }

    fn new_project(&mut self) {
        self.save_project();

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.current_project_id = format!("project_{ts}");
        self.current_project_name = "Untitled".to_string();

        self.objects = default_objects();
        self.waveforms.clear();
        self.audio_clips.clear();
        self.effect_regions.clear();
        self.components.clear();
        self.component_instances.clear();
        self.next_component_id = 1;
        self.selected.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.camera = Camera::new();
        self.export_region = None;
        self.editing_component = None;
        self.editing_effect_name = None;
        self.editing_waveform_name = None;
        self.command_palette = None;
        self.context_menu = None;

        if let Some(gpu) = &self.gpu {
            self.camera.zoom = gpu.window.scale_factor() as f32;
        }

        self.sync_audio_clips();
        self.save_project();
        println!("New project '{}'", self.current_project_id);
    }

    fn load_project(&mut self, project_id: &str) {
        if project_id == self.current_project_id {
            return;
        }
        self.save_project();

        let state = match self.storage.as_ref().and_then(|s| s.load(project_id)) {
            Some(s) => s,
            None => {
                println!("Failed to load project '{project_id}'");
                return;
            }
        };

        println!(
            "Loading project '{}' ({} objects, {} waveforms)",
            state.name,
            state.objects.len(),
            state.waveforms.len(),
        );

        self.current_project_id = project_id.to_string();
        self.current_project_name = state.name;
        self.camera = Camera {
            position: state.camera_position,
            zoom: state.camera_zoom,
        };
        self.objects = state.objects;
        self.waveforms = state
            .waveforms
            .into_iter()
            .map(|sw| WaveformObject {
                position: sw.position,
                size: sw.size,
                color: sw.color,
                border_radius: sw.border_radius,
                left_samples: Arc::new(Vec::new()),
                right_samples: Arc::new(Vec::new()),
                left_peaks: Arc::new(WaveformPeaks::empty()),
                right_peaks: Arc::new(WaveformPeaks::empty()),
                sample_rate: sw.sample_rate,
                filename: sw.filename,
                fade_in_px: sw.fade_in_px,
                fade_out_px: sw.fade_out_px,
            })
            .collect();
        self.audio_clips.clear();

        let restored_regions: Vec<effects::EffectRegion> = state
            .effect_regions
            .into_iter()
            .map(|ser| {
                let mut region = effects::EffectRegion::new(ser.position, ser.size);
                region.name = ser.name;
                for (pid, pname) in ser.plugin_ids.iter().zip(ser.plugin_names.iter()) {
                    region.chain.push(effects::PluginSlot {
                        plugin_id: pid.clone(),
                        plugin_name: pname.clone(),
                        plugin_path: std::path::PathBuf::new(),
                        bypass: false,
                        instance: Arc::new(std::sync::Mutex::new(None)),
                    });
                }
                region
            })
            .collect();
        self.effect_regions = restored_regions;

        self.components = state
            .components
            .into_iter()
            .map(|sc| component::ComponentDef {
                id: sc.id,
                name: sc.name,
                position: sc.position,
                size: sc.size,
                waveform_indices: sc.waveform_indices.iter().map(|&i| i as usize).collect(),
            })
            .collect();
        self.component_instances = state
            .component_instances
            .into_iter()
            .map(|si| component::ComponentInstance {
                component_id: si.component_id,
                position: si.position,
            })
            .collect();
        self.next_component_id =
            self.components.iter().map(|c| c.id).max().unwrap_or(0) + 1;

        self.sample_browser = if !state.browser_expanded.is_empty() {
            let folders: Vec<PathBuf> = state.browser_folders.iter().map(PathBuf::from).collect();
            let expanded: HashSet<PathBuf> =
                state.browser_expanded.iter().map(PathBuf::from).collect();
            let mut b = browser::SampleBrowser::from_state(folders, expanded, state.browser_visible);
            b.width = if state.browser_width > 0.0 {
                state.browser_width
            } else {
                260.0
            };
            b
        } else {
            let folders: Vec<PathBuf> = state.browser_folders.iter().map(PathBuf::from).collect();
            let mut b = browser::SampleBrowser::from_folders(folders);
            b.width = 260.0;
            b
        };

        self.selected.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.export_region = None;
        self.editing_component = None;
        self.editing_effect_name = None;
        self.editing_waveform_name = None;
        self.command_palette = None;
        self.context_menu = None;

        self.sync_audio_clips();
        println!("Project '{}' loaded", self.current_project_name);
    }

    fn refresh_open_project_menu(&mut self) {
        let menu = match &mut self.menu_state {
            Some(m) => m,
            None => return,
        };

        while menu.open_submenu.remove_at(0).is_some() {}

        let mut new_items: Vec<(MenuId, String)> = Vec::new();
        if let Some(s) = &self.storage {
            for entry in s.list_projects() {
                let item = muda::MenuItem::new(&entry.name, true, None);
                new_items.push((item.id().clone(), entry.project_id));
                let _ = menu.open_submenu.append(&item);
            }
        }
        if new_items.is_empty() {
            let _ = menu
                .open_submenu
                .append(&muda::MenuItem::new("No Projects", false, None));
        }
        menu.open_project_items = new_items;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.objects.clone(),
            waveforms: self.waveforms.clone(),
            audio_clips: self.audio_clips.clone(),
            effect_regions: self.effect_regions.iter().map(|er| EffectRegionSnapshot {
                position: er.position,
                size: er.size,
                plugin_ids: er.chain.iter().map(|s| s.plugin_id.clone()).collect(),
                plugin_names: er.chain.iter().map(|s| s.plugin_name.clone()).collect(),
                name: er.name.clone(),
            }).collect(),
            components: self.components.clone(),
            component_instances: self.component_instances.clone(),
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.objects = prev.objects;
            self.waveforms = prev.waveforms;
            self.audio_clips = prev.audio_clips;
            self.restore_effect_regions(prev.effect_regions);
            self.components = prev.components;
            self.component_instances = prev.component_instances;
            self.selected.clear();
            self.sync_audio_clips();
            self.request_redraw();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.objects = next.objects;
            self.waveforms = next.waveforms;
            self.audio_clips = next.audio_clips;
            self.restore_effect_regions(next.effect_regions);
            self.components = next.components;
            self.component_instances = next.component_instances;
            self.selected.clear();
            self.sync_audio_clips();
            self.request_redraw();
        }
    }

    fn restore_effect_regions(&mut self, snapshots: Vec<EffectRegionSnapshot>) {
        self.effect_regions = snapshots.into_iter().map(|snap| {
            let mut region = effects::EffectRegion::new(snap.position, snap.size);
            region.name = snap.name;
            for (pid, pname) in snap.plugin_ids.iter().zip(snap.plugin_names.iter()) {
                let instance = if self.plugin_registry.is_scanned() {
                    self.plugin_registry.load_plugin(pid, 48000.0, 512)
                } else {
                    None
                };
                region.chain.push(effects::PluginSlot {
                    plugin_id: pid.clone(),
                    plugin_name: pname.clone(),
                    plugin_path: std::path::PathBuf::new(),
                    bypass: false,
                    instance: Arc::new(std::sync::Mutex::new(instance)),
                });
            }
            region
        }).collect();
    }

    fn update_component_bounds(&mut self, comp_idx: usize) {
        if comp_idx >= self.components.len() {
            return;
        }
        let indices = self.components[comp_idx].waveform_indices.clone();
        if indices.is_empty() {
            return;
        }
        let (pos, size) = component::bounding_box_of_waveforms(&self.waveforms, &indices);
        self.components[comp_idx].position = pos;
        self.components[comp_idx].size = size;
    }

    fn sync_audio_clips(&self) {
        if let Some(engine) = &self.audio_engine {
            let mut positions: Vec<[f32; 2]> = self.waveforms.iter().map(|wf| wf.position).collect();
            let mut sizes: Vec<[f32; 2]> = self.waveforms.iter().map(|wf| wf.size).collect();
            let mut clips: Vec<&AudioClipData> = self.audio_clips.iter().collect();
            let mut fade_ins: Vec<f32> = self.waveforms.iter().map(|wf| wf.fade_in_px).collect();
            let mut fade_outs: Vec<f32> = self.waveforms.iter().map(|wf| wf.fade_out_px).collect();

            // Add virtual clips for each component instance
            for inst in &self.component_instances {
                if let Some(def) = self.components.iter().find(|c| c.id == inst.component_id) {
                    let offset = [
                        inst.position[0] - def.position[0],
                        inst.position[1] - def.position[1],
                    ];
                    for &wf_idx in &def.waveform_indices {
                        if wf_idx < self.waveforms.len() && wf_idx < self.audio_clips.len() {
                            let wf = &self.waveforms[wf_idx];
                            positions.push([wf.position[0] + offset[0], wf.position[1] + offset[1]]);
                            sizes.push(wf.size);
                            clips.push(&self.audio_clips[wf_idx]);
                            fade_ins.push(wf.fade_in_px);
                            fade_outs.push(wf.fade_out_px);
                        }
                    }
                }
            }

            let owned_clips: Vec<AudioClipData> = clips.iter().map(|c| (*c).clone()).collect();
            engine.update_clips(&positions, &sizes, &owned_clips, &fade_ins, &fade_outs);

            let regions: Vec<audio::AudioEffectRegion> = self.effect_regions.iter().map(|er| {
                audio::AudioEffectRegion {
                    x_start_px: er.position[0],
                    x_end_px: er.position[0] + er.size[0],
                    y_start: er.position[1],
                    y_end: er.position[1] + er.size[1],
                    plugins: er.chain.iter().map(|slot| slot.instance.clone()).collect(),
                }
            }).collect();
            engine.update_effect_regions(regions);
        }
    }

    fn toggle_recording(&mut self) {
        if self.recorder.is_none() {
            return;
        }

        let is_rec = self.recorder.as_ref().unwrap().is_recording();

        if is_rec {
            let loaded = self.recorder.as_mut().unwrap().stop();
            if let Some(loaded) = loaded {
                if let Some(idx) = self.recording_waveform_idx.take() {
                    if idx < self.waveforms.len() {
                        self.waveforms[idx].size[0] = loaded.width;
                        self.waveforms[idx].left_peaks = Arc::new(WaveformPeaks::build(&loaded.left_samples));
                        self.waveforms[idx].right_peaks = Arc::new(WaveformPeaks::build(&loaded.right_samples));
                        self.waveforms[idx].left_samples = loaded.left_samples.clone();
                        self.waveforms[idx].right_samples = loaded.right_samples.clone();
                        self.waveforms[idx].sample_rate = loaded.sample_rate;
                    }
                    if idx < self.audio_clips.len() {
                        self.audio_clips[idx] = AudioClipData {
                            samples: loaded.samples,
                            sample_rate: loaded.sample_rate,
                            duration_secs: loaded.duration_secs,
                        };
                    }
                    self.sync_audio_clips();
                }
            } else {
                if let Some(idx) = self.recording_waveform_idx.take() {
                    if idx < self.waveforms.len() {
                        self.waveforms.remove(idx);
                    }
                    if idx < self.audio_clips.len() {
                        self.audio_clips.remove(idx);
                    }
                }
            }
        } else {
            let world = self.last_canvas_click_world;
            let height = 150.0;
            let color_idx = self.waveforms.len() % WAVEFORM_COLORS.len();
            let sample_rate = self.recorder.as_ref().unwrap().sample_rate();

            self.push_undo();
            let idx = self.waveforms.len();
            self.waveforms.push(WaveformObject {
                position: [world[0], world[1] - height * 0.5],
                size: [0.0, height],
                color: WAVEFORM_COLORS[color_idx],
                border_radius: 8.0,
                left_samples: Arc::new(Vec::new()),
                right_samples: Arc::new(Vec::new()),
                left_peaks: Arc::new(WaveformPeaks::empty()),
                right_peaks: Arc::new(WaveformPeaks::empty()),
                sample_rate,
                filename: "Recording".to_string(),
                fade_in_px: 0.0,
                fade_out_px: 0.0,
            });
            self.audio_clips.push(AudioClipData {
                samples: Arc::new(Vec::new()),
                sample_rate,
                duration_secs: 0.0,
            });
            self.recording_waveform_idx = Some(idx);
            self.recorder.as_mut().unwrap().start();
        }
    }

    fn update_recording_waveform(&mut self) {
        let idx = match self.recording_waveform_idx {
            Some(i) => i,
            None => return,
        };
        let snapshot = self.recorder.as_ref().and_then(|r| r.current_snapshot());
        if let Some(loaded) = snapshot {
            if idx < self.waveforms.len() {
                self.waveforms[idx].size[0] = loaded.width;
                self.waveforms[idx].left_peaks = Arc::new(WaveformPeaks::build(&loaded.left_samples));
                self.waveforms[idx].right_peaks = Arc::new(WaveformPeaks::build(&loaded.right_samples));
                self.waveforms[idx].left_samples = loaded.left_samples;
                self.waveforms[idx].right_samples = loaded.right_samples;
                self.waveforms[idx].sample_rate = loaded.sample_rate;
            }
        }
    }

    fn is_recording(&self) -> bool {
        self.recorder
            .as_ref()
            .map(|r| r.is_recording())
            .unwrap_or(false)
    }

    fn trigger_export_render(&mut self) {
        let er = match &self.export_region {
            Some(er) => er,
            None => return,
        };

        let start_secs = er.position[0] as f64 / audio::PIXELS_PER_SECOND as f64;
        let end_secs = (er.position[0] + er.size[0]) as f64 / audio::PIXELS_PER_SECOND as f64;
        let y_start = er.position[1];
        let y_end = er.position[1] + er.size[1];

        if end_secs <= start_secs {
            println!("  Export region has zero or negative duration");
            return;
        }

        let path = rfd::FileDialog::new()
            .set_file_name("export.wav")
            .add_filter("WAV", &["wav"])
            .save_file();

        let path = match path {
            Some(p) => p,
            None => return,
        };

        let clips: Vec<audio::ExportClip> = self
            .waveforms
            .iter()
            .zip(self.audio_clips.iter())
            .map(|(wf, clip)| audio::ExportClip {
                buffer: clip.samples.clone(),
                source_sample_rate: clip.sample_rate,
                start_time_secs: wf.position[0] as f64 / audio::PIXELS_PER_SECOND as f64,
                duration_secs: clip.duration_secs as f64,
                position_y: wf.position[1],
                height: wf.size[1],
                fade_in_secs: (wf.fade_in_px / audio::PIXELS_PER_SECOND) as f64,
                fade_out_secs: (wf.fade_out_px / audio::PIXELS_PER_SECOND) as f64,
            })
            .collect();

        let effect_regions: Vec<audio::AudioEffectRegion> = self
            .effect_regions
            .iter()
            .map(|er| audio::AudioEffectRegion {
                x_start_px: er.position[0],
                x_end_px: er.position[0] + er.size[0],
                y_start: er.position[1],
                y_end: er.position[1] + er.size[1],
                plugins: er
                    .chain
                    .iter()
                    .map(|slot| slot.instance.clone())
                    .collect(),
            })
            .collect();

        match audio::render_to_wav(&path, start_secs, end_secs, y_start, y_end, &clips, &effect_regions) {
            Ok(()) => println!("  Exported WAV to {}", path.display()),
            Err(e) => println!("  Export failed: {}", e),
        }
    }

    fn request_redraw(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }

    fn update_cursor(&self) {
        if let Some(gpu) = &self.gpu {
            let icon = match &self.drag {
                DragState::Panning { .. } => CursorIcon::Grabbing,
                DragState::MovingSelection { .. } => CursorIcon::Grabbing,
                DragState::Selecting { .. } => CursorIcon::Crosshair,
                DragState::DraggingFromBrowser { .. } => CursorIcon::Grabbing,
                DragState::DraggingPlugin { .. } => CursorIcon::Grabbing,
                DragState::ResizingBrowser => CursorIcon::EwResize,
                DragState::MovingExportRegion { .. } => CursorIcon::Grabbing,
                DragState::ResizingExportRegion { nwse, .. } => {
                    if *nwse { CursorIcon::NwseResize } else { CursorIcon::NeswResize }
                }
                DragState::DraggingFade { .. } => CursorIcon::EwResize,
                DragState::ResizingComponentDef { nwse, .. } => {
                    if *nwse { CursorIcon::NwseResize } else { CursorIcon::NeswResize }
                }
                DragState::ResizingEffectRegion { nwse, .. } => {
                    if *nwse { CursorIcon::NwseResize } else { CursorIcon::NeswResize }
                }
                DragState::None => {
                    if self.sample_browser.visible && self.sample_browser.resize_hovered {
                        CursorIcon::EwResize
                    } else if self.fade_handle_hovered.is_some() {
                        CursorIcon::EwResize
                    } else if self.command_palette.is_some() {
                        CursorIcon::Default
                    } else {
                        match self.component_def_hover {
                            ComponentDefHover::CornerNW(_) | ComponentDefHover::CornerSE(_) => CursorIcon::NwseResize,
                            ComponentDefHover::CornerNE(_) | ComponentDefHover::CornerSW(_) => CursorIcon::NeswResize,
                            ComponentDefHover::None => match self.effect_region_hover {
                                EffectRegionHover::CornerNW(_) | EffectRegionHover::CornerSE(_) => CursorIcon::NwseResize,
                                EffectRegionHover::CornerNE(_) | EffectRegionHover::CornerSW(_) => CursorIcon::NeswResize,
                                EffectRegionHover::PluginLabel(_, _) => CursorIcon::Pointer,
                                EffectRegionHover::None => match self.export_hover {
                                    ExportHover::CornerNW | ExportHover::CornerSE => CursorIcon::NwseResize,
                                    ExportHover::CornerNE | ExportHover::CornerSW => CursorIcon::NeswResize,
                                    ExportHover::RenderPill => CursorIcon::Pointer,
                                    ExportHover::Body => CursorIcon::Grab,
                                    ExportHover::None => {
                                        if self.hovered.is_some() {
                                            CursorIcon::Grab
                                        } else {
                                            CursorIcon::Default
                                        }
                                    }
                                },
                            },
                        }
                    }
                }
            };
            gpu.window.set_cursor(icon);
        }
    }

    fn update_hover(&mut self) {
        let (sw, sh, scale) = self.screen_info();
        if let Some(palette) = &mut self.command_palette {
            if let Some(idx) = palette.item_at(self.mouse_pos, sw, sh, scale) {
                palette.selected_index = idx;
            }
        }
        let world = self.camera.screen_to_world(self.mouse_pos);
        self.fade_handle_hovered = hit_test_fade_handle(&self.waveforms, world, &self.camera);
        self.hovered = hit_test(&self.objects, &self.waveforms, &self.effect_regions, &self.components, &self.component_instances, self.editing_component, world, &self.camera);

        self.component_def_hover = ComponentDefHover::None;
        for (ci, def) in self.components.iter().enumerate() {
            let handle_sz = 12.0 / self.camera.zoom;
            let hs = handle_sz * 0.5;
            let p = def.position;
            let s = def.size;
            if point_in_rect(world, [p[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.component_def_hover = ComponentDefHover::CornerNW(ci);
                break;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.component_def_hover = ComponentDefHover::CornerNE(ci);
                break;
            } else if point_in_rect(world, [p[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.component_def_hover = ComponentDefHover::CornerSW(ci);
                break;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.component_def_hover = ComponentDefHover::CornerSE(ci);
                break;
            }
        }

        self.effect_region_hover = EffectRegionHover::None;
        'er_hover: for (i, er) in self.effect_regions.iter().enumerate() {
            // Check plugin label pills first
            let labels = effects::plugin_label_rects(er, &self.camera);
            for rect in &labels {
                if point_in_rect(world, rect.position, rect.size) {
                    self.effect_region_hover = EffectRegionHover::PluginLabel(i, rect.slot_idx);
                    break 'er_hover;
                }
            }

            let handle_sz = 12.0 / self.camera.zoom;
            let hs = handle_sz * 0.5;
            let p = er.position;
            let s = er.size;
            if point_in_rect(world, [p[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.effect_region_hover = EffectRegionHover::CornerNW(i);
                break;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.effect_region_hover = EffectRegionHover::CornerNE(i);
                break;
            } else if point_in_rect(world, [p[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.effect_region_hover = EffectRegionHover::CornerSW(i);
                break;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.effect_region_hover = EffectRegionHover::CornerSE(i);
                break;
            }
        }

        self.export_hover = ExportHover::None;
        if let Some(ref er) = self.export_region {
            let handle_sz = 12.0 / self.camera.zoom;
            let hs = handle_sz * 0.5;
            let p = er.position;
            let s = er.size;

            if point_in_rect(world, [p[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.export_hover = ExportHover::CornerNW;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] - hs], [handle_sz, handle_sz]) {
                self.export_hover = ExportHover::CornerNE;
            } else if point_in_rect(world, [p[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.export_hover = ExportHover::CornerSW;
            } else if point_in_rect(world, [p[0] + s[0] - hs, p[1] + s[1] - hs], [handle_sz, handle_sz]) {
                self.export_hover = ExportHover::CornerSE;
            } else {
                let pill_w = EXPORT_RENDER_PILL_W / self.camera.zoom;
                let pill_h = EXPORT_RENDER_PILL_H / self.camera.zoom;
                let pill_x = p[0] + 4.0 / self.camera.zoom;
                let pill_y = p[1] + 4.0 / self.camera.zoom;
                if point_in_rect(world, [pill_x, pill_y], [pill_w, pill_h]) {
                    self.export_hover = ExportHover::RenderPill;
                } else if point_in_rect(world, p, s) {
                    self.export_hover = ExportHover::Body;
                }
            }
        }

        self.update_cursor();
    }

    fn screen_info(&self) -> (f32, f32, f32) {
        match &self.gpu {
            Some(g) => (
                g.config.width as f32,
                g.config.height as f32,
                g.scale_factor,
            ),
            None => (1280.0, 800.0, 1.0),
        }
    }

    fn set_target_pos(&mut self, target: &HitTarget, pos: [f32; 2]) {
        match target {
            HitTarget::Object(i) => self.objects[*i].position = pos,
            HitTarget::Waveform(i) => self.waveforms[*i].position = pos,
            HitTarget::EffectRegion(i) => self.effect_regions[*i].position = pos,
            HitTarget::ComponentDef(i) => {
                let old_pos = self.components[*i].position;
                let dx = pos[0] - old_pos[0];
                let dy = pos[1] - old_pos[1];
                self.components[*i].position = pos;
                // Move all waveforms that belong to this component
                for &wf_idx in &self.components[*i].waveform_indices.clone() {
                    if wf_idx < self.waveforms.len() {
                        self.waveforms[wf_idx].position[0] += dx;
                        self.waveforms[wf_idx].position[1] += dy;
                    }
                }
            }
            HitTarget::ComponentInstance(i) => self.component_instances[*i].position = pos,
        }
    }

    fn get_target_pos(&self, target: &HitTarget) -> [f32; 2] {
        match target {
            HitTarget::Object(i) => self.objects[*i].position,
            HitTarget::Waveform(i) => self.waveforms[*i].position,
            HitTarget::EffectRegion(i) => self.effect_regions[*i].position,
            HitTarget::ComponentDef(i) => self.components[*i].position,
            HitTarget::ComponentInstance(i) => self.component_instances[*i].position,
        }
    }

    fn begin_move_selection(&mut self, world: [f32; 2], alt_copy: bool) {
        self.push_undo();

        if alt_copy {
            let mut new_selected: Vec<HitTarget> = Vec::new();
            for target in self.selected.clone() {
                match target {
                    HitTarget::Waveform(i) => {
                        if i < self.waveforms.len() {
                            let wf = self.waveforms[i].clone();
                            self.waveforms.push(wf);
                            let new_i = self.waveforms.len() - 1;
                            if i < self.audio_clips.len() {
                                let clip = self.audio_clips[i].clone();
                                self.audio_clips.push(clip);
                            }
                            new_selected.push(HitTarget::Waveform(new_i));
                        }
                    }
                    HitTarget::Object(i) => {
                        if i < self.objects.len() {
                            let obj = self.objects[i].clone();
                            self.objects.push(obj);
                            new_selected.push(HitTarget::Object(self.objects.len() - 1));
                        }
                    }
                    HitTarget::EffectRegion(i) => {
                        if i < self.effect_regions.len() {
                            let er = self.effect_regions[i].clone();
                            self.effect_regions.push(er);
                            new_selected.push(HitTarget::EffectRegion(self.effect_regions.len() - 1));
                        }
                    }
                    HitTarget::ComponentInstance(i) => {
                        if i < self.component_instances.len() {
                            let inst = self.component_instances[i].clone();
                            self.component_instances.push(inst);
                            new_selected.push(HitTarget::ComponentInstance(self.component_instances.len() - 1));
                        }
                    }
                    HitTarget::ComponentDef(i) => {
                        if i < self.components.len() {
                            let src = &self.components[i];
                            let new_id = self.next_component_id;
                            self.next_component_id += 1;
                            let src_indices = src.waveform_indices.clone();
                            let new_comp = component::ComponentDef {
                                id: new_id,
                                name: format!("{} copy", src.name),
                                position: src.position,
                                size: src.size,
                                waveform_indices: Vec::new(),
                            };
                            let mut new_wf_indices = Vec::new();
                            for &wi in &src_indices {
                                if wi < self.waveforms.len() {
                                    let wf = self.waveforms[wi].clone();
                                    self.waveforms.push(wf);
                                    let new_wi = self.waveforms.len() - 1;
                                    new_wf_indices.push(new_wi);
                                    if wi < self.audio_clips.len() {
                                        let clip = self.audio_clips[wi].clone();
                                        self.audio_clips.push(clip);
                                    }
                                }
                            }
                            self.components.push(component::ComponentDef {
                                waveform_indices: new_wf_indices,
                                ..new_comp
                            });
                            new_selected.push(HitTarget::ComponentDef(self.components.len() - 1));
                        }
                    }
                }
            }
            self.selected = new_selected;
        }

        let offsets: Vec<(HitTarget, [f32; 2])> = self
            .selected
            .iter()
            .map(|t| {
                let pos = self.get_target_pos(t);
                (*t, [world[0] - pos[0], world[1] - pos[1]])
            })
            .collect();
        self.drag = DragState::MovingSelection { offsets };
    }

    fn execute_command(&mut self, action: CommandAction) {
        match action {
            CommandAction::Copy => {
                self.copy_selected();
            }
            CommandAction::Paste => {
                self.paste_clipboard();
            }
            CommandAction::Duplicate => {
                self.duplicate_selected();
            }
            CommandAction::Delete => {
                self.delete_selected();
            }
            CommandAction::SelectAll => {
                self.selected.clear();
                for i in 0..self.objects.len() {
                    self.selected.push(HitTarget::Object(i));
                }
                for i in 0..self.waveforms.len() {
                    let in_component = self.components.iter().any(|c| c.waveform_indices.contains(&i));
                    if !in_component {
                        self.selected.push(HitTarget::Waveform(i));
                    }
                }
                for i in 0..self.effect_regions.len() {
                    self.selected.push(HitTarget::EffectRegion(i));
                }
                for i in 0..self.components.len() {
                    self.selected.push(HitTarget::ComponentDef(i));
                }
                for i in 0..self.component_instances.len() {
                    self.selected.push(HitTarget::ComponentInstance(i));
                }
            }
            CommandAction::Undo => self.undo(),
            CommandAction::Redo => self.redo(),
            CommandAction::SaveProject => self.save_project(),
            CommandAction::ZoomIn => {
                let (sw, sh, _) = self.screen_info();
                self.camera.zoom_at([sw * 0.5, sh * 0.5], 1.25);
            }
            CommandAction::ZoomOut => {
                let (sw, sh, _) = self.screen_info();
                self.camera.zoom_at([sw * 0.5, sh * 0.5], 0.8);
            }
            CommandAction::ResetZoom => {
                let (_, _, scale) = self.screen_info();
                self.camera.zoom = scale;
            }
            CommandAction::ToggleBrowser => {
                self.sample_browser.visible = !self.sample_browser.visible;
                if self.sample_browser.visible {
                    self.ensure_plugins_scanned();
                }
            }
            CommandAction::AddFolderToBrowser => {
                self.open_add_folder_dialog();
            }
            CommandAction::SetMasterVolume => {
                if let Some(p) = &mut self.command_palette {
                    p.mode = PaletteMode::VolumeFader;
                    p.fader_value = self
                        .audio_engine
                        .as_ref()
                        .map_or(1.0, |e| e.master_volume());
                    p.search_text.clear();
                }
                self.request_redraw();
                return;
            }
            CommandAction::CreateComponent => {
                self.create_component_from_selection();
            }
            CommandAction::CreateInstance => {
                self.create_instance_of_selected_component();
            }
            CommandAction::GoToComponent => {
                self.go_to_component_of_selected_instance();
            }
            CommandAction::OpenSettings => {
                self.settings_window = if self.settings_window.is_some() {
                    None
                } else {
                    Some(SettingsWindow::new())
                };
            }
            CommandAction::RenameEffectRegion => {
                let selected_er = self.selected.iter().find_map(|t| {
                    if let HitTarget::EffectRegion(i) = t { Some(*i) } else { None }
                });
                if let Some(idx) = selected_er {
                    if idx < self.effect_regions.len() {
                        let current = self.effect_regions[idx].name.clone();
                        self.editing_effect_name = Some((idx, current));
                    }
                }
            }
            CommandAction::RenameSample => {
                let selected_wf = self.selected.iter().find_map(|t| {
                    if let HitTarget::Waveform(i) = t { Some(*i) } else { None }
                });
                if let Some(idx) = selected_wf {
                    if idx < self.waveforms.len() {
                        let current = self.waveforms[idx].filename.clone();
                        self.editing_waveform_name = Some((idx, current));
                    }
                }
            }
            CommandAction::ToggleSnapToGrid => {
                self.settings.snap_to_grid = !self.settings.snap_to_grid;
                self.settings.save();
            }
            CommandAction::ToggleGrid => {
                self.settings.grid_enabled = !self.settings.grid_enabled;
                self.settings.save();
            }
            CommandAction::SetGridAdaptive(size) => {
                self.settings.grid_mode = GridMode::Adaptive(size);
                self.settings.save();
            }
            CommandAction::SetGridFixed(fg) => {
                self.settings.grid_mode = GridMode::Fixed(fg);
                self.settings.save();
            }
            CommandAction::NarrowGrid => {
                match self.settings.grid_mode {
                    GridMode::Adaptive(s) => {
                        self.settings.grid_mode = GridMode::Adaptive(s.narrower());
                    }
                    GridMode::Fixed(f) => {
                        self.settings.grid_mode = GridMode::Fixed(f.finer());
                    }
                }
                self.settings.save();
            }
            CommandAction::WidenGrid => {
                match self.settings.grid_mode {
                    GridMode::Adaptive(s) => {
                        self.settings.grid_mode = GridMode::Adaptive(s.wider());
                    }
                    GridMode::Fixed(f) => {
                        self.settings.grid_mode = GridMode::Fixed(f.coarser());
                    }
                }
                self.settings.save();
            }
            CommandAction::ToggleTripletGrid => {
                self.settings.triplet_grid = !self.settings.triplet_grid;
                self.settings.save();
            }
        }
        self.request_redraw();
    }

    fn create_component_from_selection(&mut self) {
        let wf_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::Waveform(i) => Some(*i),
                _ => None,
            })
            .collect();
        if wf_indices.is_empty() {
            println!("No waveforms selected to create component");
            return;
        }
        self.push_undo();
        let (pos, size) = component::bounding_box_of_waveforms(&self.waveforms, &wf_indices);
        let id = self.next_component_id;
        self.next_component_id += 1;
        let name = format!("Component {}", id);
        let def = component::ComponentDef {
            id,
            name: name.clone(),
            position: pos,
            size,
            waveform_indices: wf_indices,
        };
        self.components.push(def);
        let idx = self.components.len() - 1;
        self.selected.clear();
        self.selected.push(HitTarget::ComponentDef(idx));
        println!("Created component '{}' with {} waveforms", name, self.components[idx].waveform_indices.len());
    }

    fn create_instance_of_selected_component(&mut self) {
        let comp_idx = self.selected.iter().find_map(|t| match t {
            HitTarget::ComponentDef(i) => Some(*i),
            _ => None,
        });
        if let Some(ci) = comp_idx {
            if ci >= self.components.len() {
                return;
            }
            self.push_undo();
            let def = &self.components[ci];
            let offset_x = def.size[0] + 50.0;
            let inst = component::ComponentInstance {
                component_id: def.id,
                position: [def.position[0] + offset_x, def.position[1]],
            };
            self.component_instances.push(inst);
            let idx = self.component_instances.len() - 1;
            self.selected.clear();
            self.selected.push(HitTarget::ComponentInstance(idx));
            println!("Created instance of component {}", self.components[ci].name);
            self.sync_audio_clips();
        }
    }

    fn go_to_component_of_selected_instance(&mut self) {
        let inst_idx = self.selected.iter().find_map(|t| match t {
            HitTarget::ComponentInstance(i) => Some(*i),
            _ => None,
        });
        if let Some(ii) = inst_idx {
            if ii >= self.component_instances.len() {
                return;
            }
            let comp_id = self.component_instances[ii].component_id;
            if let Some((ci, def)) = self.components.iter().enumerate().find(|(_, c)| c.id == comp_id) {
                let (sw, sh, _) = self.screen_info();
                self.camera.position = [
                    def.position[0] + def.size[0] * 0.5 - sw * 0.5 / self.camera.zoom,
                    def.position[1] + def.size[1] * 0.5 - sh * 0.5 / self.camera.zoom,
                ];
                self.selected.clear();
                self.selected.push(HitTarget::ComponentDef(ci));
                println!("Navigated to component '{}'", def.name);
            }
        }
    }

    fn duplicate_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.push_undo();
        let mut new_selected: Vec<HitTarget> = Vec::new();

        for target in self.selected.clone() {
            match target {
                HitTarget::ComponentInstance(i) => {
                    if i < self.component_instances.len() {
                        let src = &self.component_instances[i];
                        let def = self.components.iter().find(|c| c.id == src.component_id);
                        let shift = def.map(|d| d.size[0]).unwrap_or(100.0);
                        let inst = component::ComponentInstance {
                            component_id: src.component_id,
                            position: [src.position[0] + shift, src.position[1]],
                        };
                        self.component_instances.push(inst);
                        new_selected.push(HitTarget::ComponentInstance(
                            self.component_instances.len() - 1,
                        ));
                    }
                }
                HitTarget::ComponentDef(i) => {
                    if i < self.components.len() {
                        let src = &self.components[i];
                        let shift = src.size[0];
                        let new_id = self.next_component_id;
                        self.next_component_id += 1;
                        let new_comp = component::ComponentDef {
                            id: new_id,
                            name: format!("{} copy", src.name),
                            position: [src.position[0] + shift, src.position[1]],
                            size: src.size,
                            waveform_indices: Vec::new(),
                        };
                        let src_indices = src.waveform_indices.clone();
                        let mut new_wf_indices = Vec::new();
                        for &wi in &src_indices {
                            if wi < self.waveforms.len() {
                                let mut wf = self.waveforms[wi].clone();
                                wf.position[0] += shift;
                                self.waveforms.push(wf);
                                let new_wi = self.waveforms.len() - 1;
                                new_wf_indices.push(new_wi);
                                if wi < self.audio_clips.len() {
                                    let clip = self.audio_clips[wi].clone();
                                    self.audio_clips.push(clip);
                                }
                            }
                        }
                        self.components.push(component::ComponentDef {
                            waveform_indices: new_wf_indices,
                            ..new_comp
                        });
                        new_selected
                            .push(HitTarget::ComponentDef(self.components.len() - 1));
                    }
                }
                HitTarget::Waveform(i) => {
                    if i < self.waveforms.len() {
                        let mut wf = self.waveforms[i].clone();
                        wf.position[0] += wf.size[0];
                        self.waveforms.push(wf);
                        let new_i = self.waveforms.len() - 1;
                        if i < self.audio_clips.len() {
                            let clip = self.audio_clips[i].clone();
                            self.audio_clips.push(clip);
                        }
                        new_selected.push(HitTarget::Waveform(new_i));
                    }
                }
                HitTarget::EffectRegion(i) => {
                    if i < self.effect_regions.len() {
                        let mut er = self.effect_regions[i].clone();
                        er.position[0] += er.size[0];
                        self.effect_regions.push(er);
                        new_selected
                            .push(HitTarget::EffectRegion(self.effect_regions.len() - 1));
                    }
                }
                HitTarget::Object(i) => {
                    if i < self.objects.len() {
                        let mut obj = self.objects[i].clone();
                        obj.position[0] += obj.size[0];
                        self.objects.push(obj);
                        new_selected.push(HitTarget::Object(self.objects.len() - 1));
                    }
                }
            }
        }

        self.selected = new_selected;
        self.sync_audio_clips();
    }

    fn copy_selected(&mut self) {
        self.clipboard.items.clear();
        for target in &self.selected {
            match target {
                HitTarget::Object(i) => {
                    if *i < self.objects.len() {
                        self.clipboard.items.push(ClipboardItem::Object(self.objects[*i].clone()));
                    }
                }
                HitTarget::Waveform(i) => {
                    if *i < self.waveforms.len() {
                        let clip = if *i < self.audio_clips.len() {
                            Some(self.audio_clips[*i].clone())
                        } else {
                            None
                        };
                        self.clipboard.items.push(ClipboardItem::Waveform(
                            self.waveforms[*i].clone(),
                            clip,
                        ));
                    }
                }
                HitTarget::EffectRegion(i) => {
                    if *i < self.effect_regions.len() {
                        self.clipboard.items.push(ClipboardItem::EffectRegion(
                            self.effect_regions[*i].clone(),
                        ));
                    }
                }
                HitTarget::ComponentDef(i) => {
                    if *i < self.components.len() {
                        let def = &self.components[*i];
                        let wfs: Vec<(WaveformObject, Option<AudioClipData>)> = def
                            .waveform_indices
                            .iter()
                            .filter_map(|&wi| {
                                if wi < self.waveforms.len() {
                                    let clip = if wi < self.audio_clips.len() {
                                        Some(self.audio_clips[wi].clone())
                                    } else {
                                        None
                                    };
                                    Some((self.waveforms[wi].clone(), clip))
                                } else {
                                    None
                                }
                            })
                            .collect();
                        self.clipboard.items.push(ClipboardItem::ComponentDef(def.clone(), wfs));
                    }
                }
                HitTarget::ComponentInstance(i) => {
                    if *i < self.component_instances.len() {
                        self.clipboard.items.push(ClipboardItem::ComponentInstance(
                            self.component_instances[*i].clone(),
                        ));
                    }
                }
            }
        }
    }

    fn paste_clipboard(&mut self) {
        if self.clipboard.items.is_empty() {
            return;
        }
        self.push_undo();
        let world = self.camera.screen_to_world(self.mouse_pos);

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        for item in &self.clipboard.items {
            let pos = match item {
                ClipboardItem::Object(o) => o.position,
                ClipboardItem::Waveform(w, _) => w.position,
                ClipboardItem::EffectRegion(e) => e.position,
                ClipboardItem::ComponentDef(d, _) => d.position,
                ClipboardItem::ComponentInstance(ci) => ci.position,
            };
            if pos[0] < min_x { min_x = pos[0]; }
            if pos[1] < min_y { min_y = pos[1]; }
        }

        let dx = world[0] - min_x;
        let dy = world[1] - min_y;
        let mut new_selected: Vec<HitTarget> = Vec::new();

        for item in self.clipboard.items.clone() {
            match item {
                ClipboardItem::Object(mut o) => {
                    o.position[0] += dx;
                    o.position[1] += dy;
                    self.objects.push(o);
                    new_selected.push(HitTarget::Object(self.objects.len() - 1));
                }
                ClipboardItem::Waveform(mut w, clip) => {
                    w.position[0] += dx;
                    w.position[1] += dy;
                    self.waveforms.push(w);
                    let idx = self.waveforms.len() - 1;
                    if let Some(c) = clip {
                        while self.audio_clips.len() < idx {
                            self.audio_clips.push(AudioClipData {
                                samples: std::sync::Arc::new(Vec::new()),
                                sample_rate: 44100,
                                duration_secs: 0.0,
                            });
                        }
                        self.audio_clips.push(c);
                    }
                    new_selected.push(HitTarget::Waveform(idx));
                }
                ClipboardItem::EffectRegion(mut e) => {
                    e.position[0] += dx;
                    e.position[1] += dy;
                    self.effect_regions.push(e);
                    new_selected.push(HitTarget::EffectRegion(self.effect_regions.len() - 1));
                }
                ClipboardItem::ComponentDef(mut d, wfs) => {
                    let new_id = self.next_component_id;
                    self.next_component_id += 1;
                    d.id = new_id;
                    d.position[0] += dx;
                    d.position[1] += dy;
                    d.name = format!("{} copy", d.name);
                    let mut new_wf_indices = Vec::new();
                    for (mut wf, clip) in wfs {
                        wf.position[0] += dx;
                        wf.position[1] += dy;
                        self.waveforms.push(wf);
                        let wi = self.waveforms.len() - 1;
                        new_wf_indices.push(wi);
                        if let Some(c) = clip {
                            while self.audio_clips.len() < wi {
                                self.audio_clips.push(AudioClipData {
                                    samples: std::sync::Arc::new(Vec::new()),
                                    sample_rate: 44100,
                                    duration_secs: 0.0,
                                });
                            }
                            self.audio_clips.push(c);
                        }
                    }
                    d.waveform_indices = new_wf_indices;
                    self.components.push(d);
                    new_selected.push(HitTarget::ComponentDef(self.components.len() - 1));
                }
                ClipboardItem::ComponentInstance(mut ci) => {
                    ci.position[0] += dx;
                    ci.position[1] += dy;
                    self.component_instances.push(ci);
                    new_selected.push(HitTarget::ComponentInstance(
                        self.component_instances.len() - 1,
                    ));
                }
            }
        }

        self.selected = new_selected;
        self.sync_audio_clips();
    }

    fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.push_undo();
        let mut obj_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::Object(i) => Some(*i),
                _ => None,
            })
            .collect();
        let mut wf_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::Waveform(i) => Some(*i),
                _ => None,
            })
            .collect();
        let mut er_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::EffectRegion(i) => Some(*i),
                _ => None,
            })
            .collect();
        let mut comp_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::ComponentDef(i) => Some(*i),
                _ => None,
            })
            .collect();
        let mut inst_indices: Vec<usize> = self
            .selected
            .iter()
            .filter_map(|t| match t {
                HitTarget::ComponentInstance(i) => Some(*i),
                _ => None,
            })
            .collect();

        obj_indices.sort_unstable_by(|a, b| b.cmp(a));
        wf_indices.sort_unstable_by(|a, b| b.cmp(a));
        er_indices.sort_unstable_by(|a, b| b.cmp(a));
        comp_indices.sort_unstable_by(|a, b| b.cmp(a));
        inst_indices.sort_unstable_by(|a, b| b.cmp(a));

        // Delete instances first
        for &i in &inst_indices {
            if i < self.component_instances.len() {
                self.component_instances.remove(i);
            }
        }

        // Delete component defs (also removes their instances and releases waveforms)
        for &i in &comp_indices {
            if i < self.components.len() {
                let comp = self.components.remove(i);
                // Remove instances that reference this component
                self.component_instances.retain(|inst| inst.component_id != comp.id);
                // Also delete the owned waveforms (sorted descending)
                let mut owned_wf: Vec<usize> = comp.waveform_indices;
                owned_wf.sort_unstable_by(|a, b| b.cmp(a));
                for &wi in &owned_wf {
                    if wi < self.waveforms.len() {
                        self.waveforms.remove(wi);
                    }
                    if wi < self.audio_clips.len() {
                        self.audio_clips.remove(wi);
                    }
                }
                // Fix waveform indices in remaining components
                for other in &mut self.components {
                    for idx in &mut other.waveform_indices {
                        for &removed in &owned_wf {
                            if *idx > removed {
                                *idx -= 1;
                            }
                        }
                    }
                }
            }
        }

        for &i in &obj_indices {
            if i < self.objects.len() {
                self.objects.remove(i);
            }
        }
        for &i in &wf_indices {
            if i < self.waveforms.len() {
                self.waveforms.remove(i);
            }
            if i < self.audio_clips.len() {
                self.audio_clips.remove(i);
            }
        }
        for &i in &er_indices {
            if i < self.effect_regions.len() {
                self.effect_regions.remove(i);
            }
        }

        self.selected.clear();
        self.sync_audio_clips();
        println!("Deleted selected items");
    }

    fn drop_audio_from_browser(&mut self, path: &std::path::Path) {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            return;
        }

        if let Some(loaded) = load_audio_file(path) {
            self.push_undo();
            let world = self.camera.screen_to_world(self.mouse_pos);
            let height = 150.0;
            let color_idx = self.waveforms.len() % WAVEFORM_COLORS.len();
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            println!(
                "  Loaded: {} ({:.1}s, {} Hz, {} samples/ch)",
                filename,
                loaded.duration_secs,
                loaded.sample_rate,
                loaded.left_samples.len(),
            );
            let left_peaks = Arc::new(WaveformPeaks::build(&loaded.left_samples));
            let right_peaks = Arc::new(WaveformPeaks::build(&loaded.right_samples));
            self.waveforms.push(WaveformObject {
                position: [world[0] - loaded.width * 0.5, world[1] - height * 0.5],
                size: [loaded.width, height],
                color: WAVEFORM_COLORS[color_idx],
                border_radius: 8.0,
                left_samples: loaded.left_samples,
                right_samples: loaded.right_samples,
                left_peaks,
                right_peaks,
                sample_rate: loaded.sample_rate,
                filename,
                fade_in_px: 0.0,
                fade_out_px: 0.0,
            });
            self.audio_clips.push(AudioClipData {
                samples: loaded.samples,
                sample_rate: loaded.sample_rate,
                duration_secs: loaded.duration_secs,
            });
            self.sync_audio_clips();
        }
    }

    fn open_add_folder_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.sample_browser.add_folder(folder);
            self.sample_browser.visible = true;
            self.request_redraw();
        }
    }

    fn plugin_section_y_offset(&self, _screen_h: f32, scale: f32) -> f32 {
        let header_h = browser::HEADER_HEIGHT * scale;
        let item_h = browser::ITEM_HEIGHT * scale;
        let total_items = self.sample_browser.entries.len() as f32;
        header_h + total_items * item_h - self.sample_browser.scroll_offset
    }

    fn ensure_plugins_scanned(&mut self) {
        if self.plugin_registry.is_scanned() {
            return;
        }
        self.plugin_registry.ensure_scanned();

        let entries: Vec<browser::PluginEntry> = self.plugin_registry
            .plugins
            .iter()
            .map(|e| browser::PluginEntry {
                unique_id: e.info.unique_id.clone(),
                name: e.info.name.clone(),
                manufacturer: e.info.manufacturer.clone(),
            })
            .collect();
        self.plugin_browser.set_plugins(entries);

        // Reload any saved plugins that were waiting for the scanner
        for region in &mut self.effect_regions {
            for slot in &mut region.chain {
                let has_instance = slot.instance.lock().ok().map_or(false, |g| g.is_some());
                if !has_instance {
                    if let Some(instance) = self.plugin_registry.load_plugin(&slot.plugin_id, 48000.0, 512) {
                        *slot.instance.lock().unwrap() = Some(instance);
                        println!("  Reloaded plugin '{}'", slot.plugin_name);
                    }
                }
            }
        }
        self.sync_audio_clips();
    }

    fn add_plugin_to_region(&mut self, region_idx: usize, plugin_id: &str, plugin_name: &str) {
        self.ensure_plugins_scanned();
        let sample_rate = 48000.0;
        let block_size = 512;

        if let Some(instance) = self.plugin_registry.load_plugin(plugin_id, sample_rate, block_size) {
            let slot = effects::PluginSlot {
                plugin_id: plugin_id.to_string(),
                plugin_name: plugin_name.to_string(),
                plugin_path: std::path::PathBuf::new(),
                bypass: false,
                instance: Arc::new(std::sync::Mutex::new(Some(instance))),
            };
            if region_idx < self.effect_regions.len() {
                self.effect_regions[region_idx].chain.push(slot);
                println!("  Added plugin '{}' to effect region {}", plugin_name, region_idx);
                self.sync_audio_clips();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Layers")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 800))
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true);

        let window = Arc::new(event_loop.create_window(attrs).unwrap());

        if !self.has_saved_state {
            self.camera.zoom = window.scale_factor() as f32;
        }

        self.gpu = Some(pollster::block_on(Gpu::new(window)));

        if let Some(ms) = &mut self.menu_state {
            if !ms.initialized {
                ms.menu.init_for_nsapp();
                ms.initialized = true;
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let is_playing = self.audio_engine.as_ref().map_or(false, |e| e.is_playing());

        if self.sample_browser.visible && self.sample_browser.tick_scroll() {
            self.request_redraw();
        }

        if is_playing || self.is_recording() {
            self.request_redraw();
        }

        if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            self.handle_menu_event(event.id);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.save_project();
                event_loop.exit();
            }

            WindowEvent::Resized(new_size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(new_size);
                    self.request_redraw();
                }
            }

            // --- drag & drop files ---
            WindowEvent::HoveredFile(_) => {
                self.file_hovering = true;
                self.request_redraw();
            }
            WindowEvent::HoveredFileCancelled => {
                self.file_hovering = false;
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                self.file_hovering = false;
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();

                if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                    if let Some(loaded) = load_audio_file(&path) {
                        self.push_undo();
                        let world = self.camera.screen_to_world(self.mouse_pos);
                        let height = 150.0;
                        let color_idx = self.waveforms.len() % WAVEFORM_COLORS.len();
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        println!(
                            "  Loaded: {} ({:.1}s, {} Hz, {} samples/ch)",
                            filename,
                            loaded.duration_secs,
                            loaded.sample_rate,
                            loaded.left_samples.len(),
                        );
                        let left_peaks = Arc::new(WaveformPeaks::build(&loaded.left_samples));
                        let right_peaks = Arc::new(WaveformPeaks::build(&loaded.right_samples));
                        self.waveforms.push(WaveformObject {
                            position: [world[0] - loaded.width * 0.5, world[1] - height * 0.5],
                            size: [loaded.width, height],
                            color: WAVEFORM_COLORS[color_idx],
                            border_radius: 8.0,
                            left_samples: loaded.left_samples,
                            right_samples: loaded.right_samples,
                            left_peaks,
                            right_peaks,
                            sample_rate: loaded.sample_rate,
                            filename,
                            fade_in_px: 0.0,
                            fade_out_px: 0.0,
                        });
                        self.audio_clips.push(AudioClipData {
                            samples: loaded.samples,
                            sample_rate: loaded.sample_rate,
                            duration_secs: loaded.duration_secs,
                        });
                        self.sync_audio_clips();
                    }
                }
                self.request_redraw();
            }

            // --- cursor ---
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = [position.x as f32, position.y as f32];

                // Plugin editor: slider drag
                {
                    let is_dragging_pe = self
                        .plugin_editor
                        .as_ref()
                        .map_or(false, |pe| pe.dragging_slider.is_some());
                    if is_dragging_pe {
                        let (scr_w, scr_h, scale) = self.screen_info();
                        let mx = self.mouse_pos[0];
                        if let Some(pe) = &mut self.plugin_editor {
                            let idx = pe.dragging_slider.unwrap();
                            let new_val = pe.slider_drag(idx, mx, scr_w, scr_h, scale);
                            let ri = pe.region_idx;
                            let si = pe.slot_idx;
                            if let Some(slot) = self.effect_regions.get(ri).and_then(|er| er.chain.get(si)) {
                                if let Ok(mut guard) = slot.instance.lock() {
                                    if let Some(inst) = guard.as_mut() {
                                        let _ = inst.set_parameter(idx, new_val);
                                    }
                                }
                            }
                        }
                        self.request_redraw();
                        return;
                    }
                }

                // Settings window: slider drag + hover
                {
                    let is_dragging_settings = self
                        .settings_window
                        .as_ref()
                        .map_or(false, |sw| sw.dragging_slider.is_some());
                    if is_dragging_settings {
                        let (scr_w, scr_h, scale) = self.screen_info();
                        let mx = self.mouse_pos[0];
                        if let Some(sw) = &self.settings_window {
                            let idx = sw.dragging_slider.unwrap();
                            sw.slider_drag(idx, mx, &mut self.settings, scr_w, scr_h, scale);
                        }
                        self.request_redraw();
                        return;
                    }
                    if self.settings_window.is_some() {
                        let (scr_w, scr_h, scale) = self.screen_info();
                        let pos = self.mouse_pos;
                        if let Some(sw) = &mut self.settings_window {
                            sw.update_hover(pos, scr_w, scr_h, scale);
                        }
                    }
                }

                if self.context_menu.is_some() {
                    let (sw, sh, scale) = self.screen_info();
                    if let Some(cm) = self.context_menu.as_mut() {
                        cm.update_hover(self.mouse_pos, sw, sh, scale);
                    }
                    self.request_redraw();
                }

                {
                    let is_dragging_fader = self
                        .command_palette
                        .as_ref()
                        .map_or(false, |p| p.fader_dragging);
                    if is_dragging_fader {
                        let (sw, sh, scale) = self.screen_info();
                        let mx = self.mouse_pos[0];
                        if let Some(p) = &mut self.command_palette {
                            p.fader_drag(mx, sw, sh, scale);
                            if let Some(engine) = &self.audio_engine {
                                engine.set_master_volume(p.fader_value);
                            }
                        }
                        self.request_redraw();
                        return;
                    }
                }

                // Update browser hover state
                if self.sample_browser.visible && !matches!(self.drag, DragState::ResizingBrowser) {
                    let (_, sh, scale) = self.screen_info();
                    if self.sample_browser.contains(self.mouse_pos, sh, scale) {
                        let plugin_y = self.plugin_section_y_offset(sh, scale);
                        let header_h = browser::HEADER_HEIGHT * scale;
                        let local_plugin_y = self.mouse_pos[1] - plugin_y;
                        if local_plugin_y >= 0.0 && plugin_y >= header_h && !self.plugin_browser.plugins.is_empty() {
                            self.plugin_browser.update_hover(local_plugin_y, scale);
                            self.sample_browser.hovered_entry = None;
                        } else {
                            self.plugin_browser.hovered_entry = None;
                            self.sample_browser.update_hover(self.mouse_pos, sh, scale);
                        }
                    } else {
                        self.sample_browser.hovered_entry = None;
                        self.sample_browser.add_button_hovered = false;
                        self.sample_browser.resize_hovered = false;
                        self.plugin_browser.hovered_entry = None;
                    }
                    self.update_cursor();
                }

                // If resizing browser panel, update width
                if matches!(self.drag, DragState::ResizingBrowser) {
                    let (_, _, scale) = self.screen_info();
                    self.sample_browser
                        .set_width_from_screen(self.mouse_pos[0], scale);
                    self.request_redraw();
                    return;
                }

                // If dragging from browser or plugin, just request redraw for ghost
                if matches!(self.drag, DragState::DraggingFromBrowser { .. } | DragState::DraggingPlugin { .. }) {
                    self.request_redraw();
                    return;
                }

                // Resizing component def
                if let DragState::ResizingComponentDef { comp_idx, anchor, .. } = self.drag {
                    let world = self.camera.screen_to_world(self.mouse_pos);
                    if comp_idx < self.components.len() {
                        let min_size = 40.0;
                        let x0 = anchor[0].min(world[0]);
                        let y0 = anchor[1].min(world[1]);
                        let x1 = anchor[0].max(world[0]);
                        let y1 = anchor[1].max(world[1]);
                        self.components[comp_idx].position = [x0, y0];
                        self.components[comp_idx].size = [(x1 - x0).max(min_size), (y1 - y0).max(min_size)];
                    }
                    self.request_redraw();
                    return;
                }

                // Resizing export region
                if let DragState::ResizingExportRegion { anchor, .. } = self.drag {
                    let world = self.camera.screen_to_world(self.mouse_pos);
                    if let Some(ref mut er) = self.export_region {
                        let min_size = 40.0;
                        let x0 = anchor[0].min(world[0]);
                        let y0 = anchor[1].min(world[1]);
                        let x1 = anchor[0].max(world[0]);
                        let y1 = anchor[1].max(world[1]);
                        er.position = [x0, y0];
                        er.size = [(x1 - x0).max(min_size), (y1 - y0).max(min_size)];
                    }
                    self.request_redraw();
                    return;
                }

                // Resizing effect region
                if let DragState::ResizingEffectRegion { region_idx, anchor, .. } = self.drag {
                    let world = self.camera.screen_to_world(self.mouse_pos);
                    if region_idx < self.effect_regions.len() {
                        let min_size = 40.0;
                        let x0 = anchor[0].min(world[0]);
                        let y0 = anchor[1].min(world[1]);
                        let x1 = anchor[0].max(world[0]);
                        let y1 = anchor[1].max(world[1]);
                        self.effect_regions[region_idx].position = [x0, y0];
                        self.effect_regions[region_idx].size = [(x1 - x0).max(min_size), (y1 - y0).max(min_size)];
                    }
                    self.request_redraw();
                    return;
                }

                // Moving export region
                if let DragState::MovingExportRegion { offset } = self.drag {
                    let world = self.camera.screen_to_world(self.mouse_pos);
                    if let Some(ref mut er) = self.export_region {
                        er.position = [world[0] - offset[0], world[1] - offset[1]];
                    }
                    self.request_redraw();
                    return;
                }

                // Dragging fade handle
                if let DragState::DraggingFade { waveform_idx, is_fade_in } = self.drag {
                    let world = self.camera.screen_to_world(self.mouse_pos);
                    if let Some(wf) = self.waveforms.get_mut(waveform_idx) {
                        let max_fade = wf.size[0] * 0.5;
                        if is_fade_in {
                            let new_val = (world[0] - wf.position[0]).clamp(0.0, max_fade);
                            wf.fade_in_px = new_val;
                        } else {
                            let new_val = (wf.position[0] + wf.size[0] - world[0]).clamp(0.0, max_fade);
                            wf.fade_out_px = new_val;
                        }
                    }
                    self.sync_audio_clips();
                    self.request_redraw();
                    return;
                }

                enum Action {
                    Pan([f32; 2], [f32; 2]),
                    MoveSelection(Vec<(HitTarget, [f32; 2])>),
                    Other,
                }
                let action = match &self.drag {
                    DragState::Panning {
                        start_mouse,
                        start_camera,
                    } => Action::Pan(*start_mouse, *start_camera),
                    DragState::MovingSelection { offsets } => {
                        Action::MoveSelection(offsets.clone())
                    }
                    _ => Action::Other,
                };

                match action {
                    Action::Pan(sm, sc) => {
                        self.camera.position[0] =
                            sc[0] - (self.mouse_pos[0] - sm[0]) / self.camera.zoom;
                        self.camera.position[1] =
                            sc[1] - (self.mouse_pos[1] - sm[1]) / self.camera.zoom;
                    }
                    Action::MoveSelection(offsets) => {
                        let world = self.camera.screen_to_world(self.mouse_pos);
                        let mut needs_sync = false;
                        for (target, offset) in &offsets {
                            let raw_x = world[0] - offset[0];
                            let snapped_x = snap_to_grid(raw_x, &self.settings, self.camera.zoom);
                            self.set_target_pos(
                                target,
                                [snapped_x, world[1] - offset[1]],
                            );
                            if matches!(target, HitTarget::Waveform(_) | HitTarget::EffectRegion(_) | HitTarget::ComponentDef(_) | HitTarget::ComponentInstance(_)) {
                                needs_sync = true;
                            }
                        }
                        if let Some(ec_idx) = self.editing_component {
                            self.update_component_bounds(ec_idx);
                        }
                        if needs_sync {
                            self.sync_audio_clips();
                        }
                    }
                    Action::Other => {}
                }

                self.update_hover();
                self.request_redraw();
            }

            // --- mouse buttons ---
            WindowEvent::MouseInput { state, button, .. } => match button {
                MouseButton::Middle => match state {
                    ElementState::Pressed => {
                        self.command_palette = None;
                        self.drag = DragState::Panning {
                            start_mouse: self.mouse_pos,
                            start_camera: self.camera.position,
                        };
                        self.update_cursor();
                        self.request_redraw();
                    }
                    ElementState::Released => {
                        self.drag = DragState::None;
                        self.update_cursor();
                        self.request_redraw();
                    }
                },

                MouseButton::Right => {
                    if state == ElementState::Pressed {
                        self.command_palette = None;
                        let world = self.camera.screen_to_world(self.mouse_pos);
                        let hit = hit_test(&self.objects, &self.waveforms, &self.effect_regions, &self.components, &self.component_instances, self.editing_component, world, &self.camera);
                        let menu_ctx = match hit {
                            Some(HitTarget::ComponentInstance(_)) => {
                                if !self.selected.contains(&hit.unwrap()) {
                                    self.selected.clear();
                                    self.selected.push(hit.unwrap());
                                }
                                MenuContext::ComponentInstance
                            }
                            Some(HitTarget::ComponentDef(_)) => {
                                if !self.selected.contains(&hit.unwrap()) {
                                    self.selected.clear();
                                    self.selected.push(hit.unwrap());
                                }
                                MenuContext::ComponentDef
                            }
                            Some(target) => {
                                if !self.selected.contains(&target) {
                                    self.selected.clear();
                                    self.selected.push(target);
                                }
                                let has_waveforms = self.selected.iter().any(|t| matches!(t, HitTarget::Waveform(_)));
                                let has_effect_region = self.selected.iter().any(|t| matches!(t, HitTarget::EffectRegion(_)));
                                MenuContext::Selection { has_waveforms, has_effect_region }
                            }
                            None => {
                                if self.selected.is_empty() {
                                    MenuContext::Grid
                                } else {
                                    let has_waveforms = self.selected.iter().any(|t| matches!(t, HitTarget::Waveform(_)));
                                    let has_effect_region = self.selected.iter().any(|t| matches!(t, HitTarget::EffectRegion(_)));
                                    MenuContext::Selection { has_waveforms, has_effect_region }
                                }
                            }
                        };
                        self.context_menu = Some(ContextMenu::new(self.mouse_pos, menu_ctx, &self.settings));
                        self.request_redraw();
                    }
                }

                MouseButton::Left => match state {
                    ElementState::Pressed => {
                        // Plugin editor click
                        if self.plugin_editor.is_some() {
                            let (scr_w, scr_h, scale) = self.screen_info();
                            let inside = self
                                .plugin_editor
                                .as_ref()
                                .map_or(false, |pe| pe.contains(self.mouse_pos, scr_w, scr_h, scale));
                            if inside {
                                let slider_hit = self
                                    .plugin_editor
                                    .as_ref()
                                    .and_then(|pe| pe.slider_hit_test(self.mouse_pos, scr_w, scr_h, scale));
                                if let Some(idx) = slider_hit {
                                    if let Some(pe) = &mut self.plugin_editor {
                                        pe.dragging_slider = Some(idx);
                                        let new_val = pe.slider_drag(idx, self.mouse_pos[0], scr_w, scr_h, scale);
                                        let ri = pe.region_idx;
                                        let si = pe.slot_idx;
                                        if let Some(slot) = self.effect_regions.get(ri).and_then(|er| er.chain.get(si)) {
                                            if let Ok(mut guard) = slot.instance.lock() {
                                                if let Some(inst) = guard.as_mut() {
                                                    let _ = inst.set_parameter(idx, new_val);
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                self.plugin_editor = None;
                            }
                            self.request_redraw();
                            return;
                        }

                        // Settings window click
                        if self.settings_window.is_some() {
                            let (scr_w, scr_h, scale) = self.screen_info();
                            let inside = self
                                .settings_window
                                .as_ref()
                                .map_or(false, |sw| sw.contains(self.mouse_pos, scr_w, scr_h, scale));
                            if inside {
                                // Try audio dropdown interaction first
                                let prev_output_device = self.settings.audio_output_device.clone();
                                let audio_consumed = self
                                    .settings_window
                                    .as_mut()
                                    .map_or(false, |sw| {
                                        sw.handle_audio_click(self.mouse_pos, &mut self.settings, scr_w, scr_h, scale)
                                    });
                                if audio_consumed {
                                    self.settings.save();

                                    if self.settings.audio_output_device != prev_output_device {
                                        println!("[audio] Output device changed: '{}' -> '{}'", prev_output_device, self.settings.audio_output_device);

                                        let old_pos = self.audio_engine.as_ref().map(|e| e.position_seconds());
                                        let old_vol = self.audio_engine.as_ref().map(|e| e.master_volume());
                                        let was_playing = self.audio_engine.as_ref().map_or(false, |e| e.is_playing());

                                        let device_name = if self.settings.audio_output_device == "No Device" {
                                            None
                                        } else {
                                            Some(self.settings.audio_output_device.as_str())
                                        };
                                        self.audio_engine = AudioEngine::new_with_device(device_name);

                                        if let Some(ref engine) = self.audio_engine {
                                            let actual = engine.device_name().to_string();
                                            if self.settings.audio_output_device != actual {
                                                println!("[audio] Device '{}' not available, using '{}'", self.settings.audio_output_device, actual);
                                                self.settings.audio_output_device = actual;
                                                self.settings.save();
                                            }
                                            if let Some(pos) = old_pos {
                                                engine.seek_to_seconds(pos);
                                            }
                                            if let Some(vol) = old_vol {
                                                engine.set_master_volume(vol);
                                            }
                                        } else {
                                            println!("[audio] Warning: failed to create audio engine for device");
                                        }

                                        self.sync_audio_clips();
                                        if was_playing {
                                            if let Some(engine) = &self.audio_engine {
                                                engine.toggle_playback();
                                            }
                                        }
                                    }

                                    self.request_redraw();
                                    return;
                                }

                                let slider_hit = self
                                    .settings_window
                                    .as_ref()
                                    .and_then(|sw| {
                                        sw.slider_hit_test(self.mouse_pos, &self.settings, scr_w, scr_h, scale)
                                    });
                                if let Some(idx) = slider_hit {
                                    if let Some(sw) = &mut self.settings_window {
                                        sw.dragging_slider = Some(idx);
                                    }
                                    if let Some(sw) = &self.settings_window {
                                        sw.slider_drag(idx, self.mouse_pos[0], &mut self.settings, scr_w, scr_h, scale);
                                    }
                                } else if let Some(cat_idx) = self
                                    .settings_window
                                    .as_ref()
                                    .and_then(|sw| sw.category_at(self.mouse_pos, scr_w, scr_h, scale))
                                {
                                    if let Some(sw) = &mut self.settings_window {
                                        sw.active_category = CATEGORIES[cat_idx];
                                        sw.open_dropdown = None;
                                    }
                                }
                            } else {
                                self.settings_window = None;
                            }
                            self.request_redraw();
                            return;
                        }

                        if self.context_menu.is_some() {
                            let (sw, sh, scale) = self.screen_info();
                            let inside = self
                                .context_menu
                                .as_ref()
                                .map_or(false, |cm| cm.contains(self.mouse_pos, sw, sh, scale));
                            let clicked_action = self.context_menu.as_ref().and_then(|cm| {
                                let idx = cm.item_at(self.mouse_pos, sw, sh, scale)?;
                                cm.action_at(idx)
                            });

                            if let Some(action) = clicked_action {
                                self.context_menu = None;
                                self.execute_command(action);
                            } else {
                                self.context_menu = None;
                            }
                            self.request_redraw();
                            if inside {
                                return;
                            }
                        }

                        if self.command_palette.is_some() {
                            let (sw, sh, scale) = self.screen_info();
                            let inside = self
                                .command_palette
                                .as_ref()
                                .map_or(false, |p| p.contains(self.mouse_pos, sw, sh, scale));

                            let is_fader = self
                                .command_palette
                                .as_ref()
                                .map_or(false, |p| p.mode == PaletteMode::VolumeFader);

                            if is_fader {
                                if inside {
                                    let hit = self
                                        .command_palette
                                        .as_ref()
                                        .map_or(false, |p| {
                                            p.fader_hit_test(self.mouse_pos, sw, sh, scale)
                                        });
                                    if hit {
                                        if let Some(p) = &mut self.command_palette {
                                            p.fader_dragging = true;
                                        }
                                    }
                                } else {
                                    self.command_palette = None;
                                }
                                self.request_redraw();
                                return;
                            }

                            let clicked_action = self.command_palette.as_ref().and_then(|p| {
                                let idx = p.item_at(self.mouse_pos, sw, sh, scale)?;
                                let mut cmd_i = 0;
                                for row in p.visible_rows() {
                                    if let PaletteRow::Command(ci) = row {
                                        if cmd_i == idx {
                                            return Some(COMMANDS[*ci].action);
                                        }
                                        cmd_i += 1;
                                    }
                                }
                                None
                            });

                            if let Some(action) = clicked_action {
                                if action == CommandAction::SetMasterVolume {
                                    self.execute_command(action);
                                } else {
                                    self.command_palette = None;
                                    self.execute_command(action);
                                }
                            } else if !inside {
                                self.command_palette = None;
                            }
                            self.request_redraw();
                            return;
                        }

                        // --- sample browser click ---
                        if self.sample_browser.visible {
                            let (_, sh, scale) = self.screen_info();
                            if self.sample_browser.contains(self.mouse_pos, sh, scale) {
                                if self.sample_browser.hit_resize_handle(self.mouse_pos, scale) {
                                    self.drag = DragState::ResizingBrowser;
                                    self.update_cursor();
                                    self.request_redraw();
                                    return;
                                } else if self.sample_browser.hit_add_button(self.mouse_pos, scale)
                                {
                                    self.open_add_folder_dialog();
                                } else {
                                    let plugin_section_y = self.plugin_section_y_offset(sh, scale);
                                    let header_h = browser::HEADER_HEIGHT * scale;
                                    let local_plugin_y = self.mouse_pos[1] - plugin_section_y;

                                    if local_plugin_y >= 0.0 && plugin_section_y >= header_h {
                                        if self.plugin_browser.hit_header(local_plugin_y, scale) {
                                            self.plugin_browser.expanded = !self.plugin_browser.expanded;
                                            self.plugin_browser.text_dirty = true;
                                            self.sample_browser.extra_content_height = self.plugin_browser.section_height(scale);
                                        } else if let Some(idx) = self.plugin_browser.item_at(local_plugin_y, scale) {
                                            let plugin = self.plugin_browser.plugins[idx].clone();
                                            self.drag = DragState::DraggingPlugin {
                                                plugin_id: plugin.unique_id,
                                                plugin_name: plugin.name,
                                            };
                                        }
                                    } else if let Some(idx) =
                                        self.sample_browser.item_at(self.mouse_pos, sh, scale)
                                    {
                                        let entry = self.sample_browser.entries[idx].clone();
                                        if entry.is_dir {
                                            self.sample_browser.toggle_expand(idx);
                                        } else {
                                            let ext = entry
                                                .path
                                                .extension()
                                                .map(|e| e.to_string_lossy().to_lowercase())
                                                .unwrap_or_default();
                                            if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                                                self.drag = DragState::DraggingFromBrowser {
                                                    path: entry.path.clone(),
                                                    filename: entry.name.clone(),
                                                };
                                            }
                                        }
                                    }
                                }
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- Export button click ---
                        {
                            let (sw, sh, scale) = self.screen_info();
                            if TransportPanel::hit_export_button(self.mouse_pos, sw, sh, scale) {
                                let center = self.camera.screen_to_world([sw * 0.5, sh * 0.5]);
                                self.export_region = Some(ExportRegion {
                                    position: [
                                        center[0] - EXPORT_REGION_DEFAULT_WIDTH * 0.5,
                                        center[1] - EXPORT_REGION_DEFAULT_HEIGHT * 0.5,
                                    ],
                                    size: [EXPORT_REGION_DEFAULT_WIDTH, EXPORT_REGION_DEFAULT_HEIGHT],
                                });
                                println!("  Created export region");
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- FX button click ---
                        {
                            let (sw, sh, scale) = self.screen_info();
                            if TransportPanel::hit_fx_button(self.mouse_pos, sw, sh, scale) {
                                let center = self.camera.screen_to_world([sw * 0.5, sh * 0.5]);
                                let w = effects::EFFECT_REGION_DEFAULT_WIDTH;
                                let h = effects::EFFECT_REGION_DEFAULT_HEIGHT;
                                self.push_undo();
                                self.effect_regions.push(effects::EffectRegion::new(
                                    [center[0] - w * 0.5, center[1] - h * 0.5],
                                    [w, h],
                                ));
                                let idx = self.effect_regions.len() - 1;
                                self.selected.clear();
                                self.selected.push(HitTarget::EffectRegion(idx));
                                println!("  Created effect region");
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- transport panel click ---
                        {
                            let (sw, sh, scale) = self.screen_info();
                            if TransportPanel::contains(self.mouse_pos, sw, sh, scale) {
                                if TransportPanel::hit_record_button(self.mouse_pos, sw, sh, scale)
                                {
                                    self.toggle_recording();
                                } else if let Some(engine) = &self.audio_engine {
                                    engine.toggle_playback();
                                }
                                self.request_redraw();
                                return;
                            }
                        }

                        let world = self.camera.screen_to_world(self.mouse_pos);
                        self.last_canvas_click_world = world;

                        // --- component def corner resize ---
                        {
                            let handle_sz = 12.0 / self.camera.zoom;
                            let mut corner_hit: Option<(usize, [f32; 2], bool)> = None;
                            for (ci, def) in self.components.iter().enumerate() {
                                let p = def.position;
                                let s = def.size;
                                let corners: [([f32; 2], [f32; 2], bool); 4] = [
                                    ([p[0], p[1]], [p[0] + s[0], p[1] + s[1]], true),
                                    ([p[0] + s[0], p[1]], [p[0], p[1] + s[1]], false),
                                    ([p[0], p[1] + s[1]], [p[0] + s[0], p[1]], false),
                                    ([p[0] + s[0], p[1] + s[1]], [p[0], p[1]], true),
                                ];
                                for (corner, anchor, is_nwse) in &corners {
                                    let hx = corner[0] - handle_sz * 0.5;
                                    let hy = corner[1] - handle_sz * 0.5;
                                    if point_in_rect(world, [hx, hy], [handle_sz, handle_sz]) {
                                        corner_hit = Some((ci, *anchor, *is_nwse));
                                        break;
                                    }
                                }
                                if corner_hit.is_some() {
                                    break;
                                }
                            }
                            if let Some((ci, anchor, nwse)) = corner_hit {
                                self.push_undo();
                                self.drag = DragState::ResizingComponentDef {
                                    comp_idx: ci,
                                    anchor,
                                    nwse,
                                };
                                self.update_cursor();
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- plugin label click (opens parameter editor) ---
                        if let EffectRegionHover::PluginLabel(ri, si) = self.effect_region_hover {
                            if ri < self.effect_regions.len() && si < self.effect_regions[ri].chain.len() {
                                let slot = &self.effect_regions[ri].chain[si];
                                let name = slot.plugin_name.clone();
                                let mut params = Vec::new();
                                if let Ok(guard) = slot.instance.lock() {
                                    if let Some(inst) = guard.as_ref() {
                                        let count = inst.parameter_count();
                                        for idx in 0..count {
                                            let info = inst.parameter_info(idx);
                                            let val = inst.get_parameter(idx).unwrap_or(0.0);
                                            let (pname, unit, default) = match info {
                                                Ok(pi) => (pi.name, pi.unit, pi.default),
                                                Err(_) => (format!("Param {}", idx), String::new(), 0.0),
                                            };
                                            params.push(plugin_editor::ParamEntry {
                                                name: pname,
                                                unit,
                                                value: val,
                                                default,
                                            });
                                        }
                                    }
                                }
                                self.plugin_editor = Some(plugin_editor::PluginEditorWindow::new(
                                    ri, si, name, params,
                                ));
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- effect region corner resize ---
                        {
                            let handle_sz = 12.0 / self.camera.zoom;
                            let mut corner_hit: Option<(usize, [f32; 2], bool)> = None;
                            for (i, er) in self.effect_regions.iter().enumerate() {
                                let p = er.position;
                                let s = er.size;
                                let corners: [([f32; 2], [f32; 2], bool); 4] = [
                                    ([p[0], p[1]], [p[0] + s[0], p[1] + s[1]], true),
                                    ([p[0] + s[0], p[1]], [p[0], p[1] + s[1]], false),
                                    ([p[0], p[1] + s[1]], [p[0] + s[0], p[1]], false),
                                    ([p[0] + s[0], p[1] + s[1]], [p[0], p[1]], true),
                                ];
                                for (corner, anchor, is_nwse) in &corners {
                                    let hx = corner[0] - handle_sz * 0.5;
                                    let hy = corner[1] - handle_sz * 0.5;
                                    if point_in_rect(world, [hx, hy], [handle_sz, handle_sz]) {
                                        corner_hit = Some((i, *anchor, *is_nwse));
                                        break;
                                    }
                                }
                                if corner_hit.is_some() {
                                    break;
                                }
                            }
                            if let Some((idx, anchor, nwse)) = corner_hit {
                                self.push_undo();
                                self.drag = DragState::ResizingEffectRegion {
                                    region_idx: idx,
                                    anchor,
                                    nwse,
                                };
                                self.update_cursor();
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- export region interaction ---
                        if let Some(ref er) = self.export_region {
                            let handle_sz = 12.0 / self.camera.zoom;
                            // (corner_center, anchor = opposite corner, nwse diagonal?)
                            let corners: [([f32; 2], [f32; 2], bool); 4] = [
                                ([er.position[0], er.position[1]],
                                 [er.position[0] + er.size[0], er.position[1] + er.size[1]], true),
                                ([er.position[0] + er.size[0], er.position[1]],
                                 [er.position[0], er.position[1] + er.size[1]], false),
                                ([er.position[0], er.position[1] + er.size[1]],
                                 [er.position[0] + er.size[0], er.position[1]], false),
                                ([er.position[0] + er.size[0], er.position[1] + er.size[1]],
                                 [er.position[0], er.position[1]], true),
                            ];
                            let mut hit_corner = false;
                            for (corner, anchor, is_nwse) in &corners {
                                let hx = corner[0] - handle_sz * 0.5;
                                let hy = corner[1] - handle_sz * 0.5;
                                if point_in_rect(world, [hx, hy], [handle_sz, handle_sz]) {
                                    self.drag = DragState::ResizingExportRegion { anchor: *anchor, nwse: *is_nwse };
                                    self.update_cursor();
                                    self.request_redraw();
                                    hit_corner = true;
                                    break;
                                }
                            }
                            if hit_corner {
                                return;
                            }

                            let pill_w = EXPORT_RENDER_PILL_W / self.camera.zoom;
                            let pill_h = EXPORT_RENDER_PILL_H / self.camera.zoom;
                            let pill_x = er.position[0] + 4.0 / self.camera.zoom;
                            let pill_y = er.position[1] + 4.0 / self.camera.zoom;
                            if point_in_rect(world, [pill_x, pill_y], [pill_w, pill_h]) {
                                self.trigger_export_render();
                                self.request_redraw();
                                return;
                            }
                            if point_in_rect(world, er.position, er.size) {
                                let offset = [world[0] - er.position[0], world[1] - er.position[1]];
                                self.drag = DragState::MovingExportRegion { offset };
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- fade handle drag ---
                        if let Some((wf_idx, is_fade_in)) =
                            hit_test_fade_handle(&self.waveforms, world, &self.camera)
                        {
                            self.push_undo();
                            self.drag = DragState::DraggingFade { waveform_idx: wf_idx, is_fade_in };
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        let hit = hit_test(&self.objects, &self.waveforms, &self.effect_regions, &self.components, &self.component_instances, self.editing_component, world, &self.camera);

                        // Double-click detection: enter component edit mode
                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(self.last_click_time);
                        let dist = ((world[0] - self.last_click_world[0]).powi(2) + (world[1] - self.last_click_world[1]).powi(2)).sqrt();
                        let is_double_click = elapsed.as_millis() < 400 && dist < 10.0 / self.camera.zoom;
                        self.last_click_time = now;
                        self.last_click_world = world;

                        if is_double_click {
                            if let Some(HitTarget::ComponentDef(ci)) = hit {
                                self.editing_component = Some(ci);
                                self.selected.clear();
                                println!("Entered component edit mode: {}", self.components[ci].name);
                                self.request_redraw();
                                return;
                            }
                        }

                        // Click outside the editing component exits edit mode
                        if let Some(ec_idx) = self.editing_component {
                            if let Some(def) = self.components.get(ec_idx) {
                                if !point_in_rect(world, def.position, def.size) {
                                    self.editing_component = None;
                                    self.selected.clear();
                                    println!("Exited component edit mode");
                                    // Re-do hit test without edit mode
                                    let hit2 = hit_test(&self.objects, &self.waveforms, &self.effect_regions, &self.components, &self.component_instances, None, world, &self.camera);
                                    if let Some(target) = hit2 {
                                        self.selected.push(target);
                                        self.begin_move_selection(world, self.modifiers.alt_key());
                                    } else {
                                        self.drag = DragState::Selecting { start_world: world };
                                    }
                                    self.update_cursor();
                                    self.request_redraw();
                                    return;
                                }
                            }
                        }

                        match hit {
                            Some(target) => {
                                if self.selected.contains(&target) {
                                    // Already selected -> drag whole selection
                                } else {
                                    self.selected.clear();
                                    self.selected.push(target);
                                }
                                self.begin_move_selection(world, self.modifiers.alt_key());
                            }
                            None => {
                                self.drag = DragState::Selecting { start_world: world };
                            }
                        }

                        self.update_cursor();
                        self.request_redraw();
                    }

                    ElementState::Released => {
                        // Finish plugin editor slider drag
                        if let Some(pe) = &mut self.plugin_editor {
                            if pe.dragging_slider.is_some() {
                                pe.dragging_slider = None;
                                self.request_redraw();
                                return;
                            }
                        }

                        // Finish settings slider drag
                        if let Some(sw) = &mut self.settings_window {
                            if sw.dragging_slider.is_some() {
                                sw.dragging_slider = None;
                                self.settings.save();
                                self.request_redraw();
                                return;
                            }
                        }

                        if let Some(p) = &mut self.command_palette {
                            if p.fader_dragging {
                                p.fader_dragging = false;
                                self.request_redraw();
                                return;
                            }
                        }

                        // --- finish browser resize ---
                        if matches!(self.drag, DragState::ResizingBrowser) {
                            self.drag = DragState::None;
                            self.update_hover();
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        // --- finish resizing component def ---
                        if matches!(self.drag, DragState::ResizingComponentDef { .. }) {
                            self.drag = DragState::None;
                            self.sync_audio_clips();
                            self.update_hover();
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        // --- finish resizing effect region ---
                        if matches!(self.drag, DragState::ResizingEffectRegion { .. }) {
                            self.drag = DragState::None;
                            self.sync_audio_clips();
                            self.update_hover();
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        // --- finish moving/resizing export region ---
                        if matches!(self.drag, DragState::MovingExportRegion { .. } | DragState::ResizingExportRegion { .. }) {
                            self.drag = DragState::None;
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        // --- finish fade handle drag ---
                        if matches!(self.drag, DragState::DraggingFade { .. }) {
                            self.drag = DragState::None;
                            self.sync_audio_clips();
                            self.update_hover();
                            self.update_cursor();
                            self.request_redraw();
                            return;
                        }

                        // --- drop from browser to canvas ---
                        if let DragState::DraggingFromBrowser { ref path, .. } = self.drag {
                            let (_, sh, scale) = self.screen_info();
                            let in_browser = self.sample_browser.visible
                                && self.sample_browser.contains(self.mouse_pos, sh, scale);
                            if !in_browser {
                                let path = path.clone();
                                self.drop_audio_from_browser(&path);
                            }
                            self.drag = DragState::None;
                            self.update_hover();
                            self.request_redraw();
                            return;
                        }

                        // --- drop plugin from browser to canvas/effect region ---
                        if let DragState::DraggingPlugin { ref plugin_id, ref plugin_name } = self.drag {
                            let plugin_id = plugin_id.clone();
                            let plugin_name = plugin_name.clone();
                            let (_, sh, scale) = self.screen_info();
                            let in_browser = self.sample_browser.visible
                                && self.sample_browser.contains(self.mouse_pos, sh, scale);
                            if !in_browser {
                                let world = self.camera.screen_to_world(self.mouse_pos);
                                let hit_er = self.effect_regions.iter().enumerate().rev().find(|(_, er)| {
                                    point_in_rect(world, er.position, er.size)
                                }).map(|(i, _)| i);

                                if let Some(er_idx) = hit_er {
                                    self.add_plugin_to_region(er_idx, &plugin_id, &plugin_name);
                                } else {
                                    self.push_undo();
                                    let w = effects::EFFECT_REGION_DEFAULT_WIDTH;
                                    let h = effects::EFFECT_REGION_DEFAULT_HEIGHT;
                                    self.effect_regions.push(effects::EffectRegion::new(
                                        [world[0] - w * 0.5, world[1] - h * 0.5],
                                        [w, h],
                                    ));
                                    let idx = self.effect_regions.len() - 1;
                                    self.add_plugin_to_region(idx, &plugin_id, &plugin_name);
                                    self.selected.clear();
                                    self.selected.push(HitTarget::EffectRegion(idx));
                                }
                            }
                            self.drag = DragState::None;
                            self.update_hover();
                            self.request_redraw();
                            return;
                        }

                        if let DragState::Selecting { start_world } = &self.drag {
                            let start = *start_world;
                            let current = self.camera.screen_to_world(self.mouse_pos);
                            let (rp, rs) = canonical_rect(start, current);

                            let min_sz = 5.0 / self.camera.zoom;
                            if rs[0] < min_sz && rs[1] < min_sz {
                                self.selected.clear();
                                if let Some(engine) = &self.audio_engine {
                                    let secs = current[0] as f64 / PIXELS_PER_SECOND as f64;
                                    engine.seek_to_seconds(secs);
                                }
                            } else {
                                self.selected =
                                    targets_in_rect(&self.objects, &self.waveforms, &self.effect_regions, &self.components, &self.component_instances, self.editing_component, rp, rs);
                            }
                        }

                        self.drag = DragState::None;
                        self.sync_audio_clips();
                        self.update_hover();
                        self.request_redraw();
                    }
                },
                _ => {}
            },

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if self.plugin_editor.is_some() {
                        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                            self.plugin_editor = None;
                            self.request_redraw();
                            return;
                        }
                        return;
                    }

                    if self.settings_window.is_some() {
                        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                            self.settings_window = None;
                            self.request_redraw();
                            return;
                        }
                        // Block other keyboard input while settings is open
                        if !self.modifiers.super_key() {
                            return;
                        }
                    }

                    if self.context_menu.is_some() {
                        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                            self.context_menu = None;
                            self.request_redraw();
                            return;
                        }
                    }

                    if self.editing_component.is_some() {
                        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                            self.editing_component = None;
                            self.selected.clear();
                            println!("Exited component edit mode");
                            self.request_redraw();
                            return;
                        }
                    }

                    // --- effect region name editing input ---
                    if self.editing_effect_name.is_some() {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.editing_effect_name = None;
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Enter) => {
                                if let Some((idx, text)) = self.editing_effect_name.take() {
                                    if idx < self.effect_regions.len() {
                                        self.push_undo();
                                        let name = if text.trim().is_empty() {
                                            "effects".to_string()
                                        } else {
                                            text
                                        };
                                        self.effect_regions[idx].name = name;
                                    }
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Backspace) => {
                                if let Some((_, ref mut text)) = self.editing_effect_name {
                                    text.pop();
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Space) => {
                                if let Some((_, ref mut text)) = self.editing_effect_name {
                                    text.push(' ');
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Character(ch) if !self.modifiers.super_key() => {
                                if let Some((_, ref mut text)) = self.editing_effect_name {
                                    text.push_str(ch.as_ref());
                                }
                                self.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // --- waveform name editing input ---
                    if self.editing_waveform_name.is_some() {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.editing_waveform_name = None;
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Enter) => {
                                if let Some((idx, text)) = self.editing_waveform_name.take() {
                                    if idx < self.waveforms.len() {
                                        self.push_undo();
                                        let name = if text.trim().is_empty() {
                                            self.waveforms[idx].filename.clone()
                                        } else {
                                            text
                                        };
                                        self.waveforms[idx].filename = name;
                                    }
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Backspace) => {
                                if let Some((_, ref mut text)) = self.editing_waveform_name {
                                    text.pop();
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Space) => {
                                if let Some((_, ref mut text)) = self.editing_waveform_name {
                                    text.push(' ');
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Character(ch) if !self.modifiers.super_key() => {
                                if let Some((_, ref mut text)) = self.editing_waveform_name {
                                    text.push_str(ch.as_ref());
                                }
                                self.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // --- command palette input ---
                    if self.command_palette.is_some() {
                        let is_fader = self
                            .command_palette
                            .as_ref()
                            .map_or(false, |p| p.mode == PaletteMode::VolumeFader);

                        if is_fader {
                            match &event.logical_key {
                                Key::Named(NamedKey::Escape) | Key::Named(NamedKey::Enter) => {
                                    self.command_palette = None;
                                    self.request_redraw();
                                    return;
                                }
                                _ => {
                                    self.request_redraw();
                                    return;
                                }
                            }
                        }

                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.command_palette = None;
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                if let Some(p) = &mut self.command_palette {
                                    p.move_selection(-1);
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                if let Some(p) = &mut self.command_palette {
                                    p.move_selection(1);
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Enter) => {
                                let action = self
                                    .command_palette
                                    .as_ref()
                                    .and_then(|p| p.selected_action());
                                if let Some(a) = action {
                                    if a == CommandAction::SetMasterVolume {
                                        self.execute_command(a);
                                    } else {
                                        self.command_palette = None;
                                        self.execute_command(a);
                                    }
                                } else {
                                    self.command_palette = None;
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Backspace) => {
                                if let Some(p) = &mut self.command_palette {
                                    p.search_text.pop();
                                    p.update_filter();
                                }
                                self.request_redraw();
                                return;
                            }
                            Key::Character(ch) if !self.modifiers.super_key() => {
                                if let Some(p) = &mut self.command_palette {
                                    p.search_text.push_str(ch.as_ref());
                                    p.update_filter();
                                }
                                self.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // --- Enter on selected effect region: show plugin info ---
                    if matches!(event.logical_key, Key::Named(NamedKey::Enter)) {
                        if let Some(HitTarget::EffectRegion(idx)) = self.selected.first().copied() {
                            if idx < self.effect_regions.len() {
                                let er = &self.effect_regions[idx];
                                if er.chain.is_empty() {
                                    println!("  Effect region {} has no plugins", idx);
                                } else {
                                    println!("  Effect region {} plugin chain:", idx);
                                    for (j, slot) in er.chain.iter().enumerate() {
                                        let param_count = slot.instance.lock().ok()
                                            .and_then(|g| g.as_ref().map(|p| p.parameter_count()))
                                            .unwrap_or(0);
                                        println!("    [{}] {} ({} params)", j, slot.plugin_name, param_count);
                                    }
                                }
                            }
                            self.request_redraw();
                        }
                    }

                    // --- global shortcuts ---
                    match &event.logical_key {
                        Key::Named(NamedKey::Space) => {
                            if self.is_recording() {
                                self.toggle_recording();
                                self.request_redraw();
                            } else if let Some(engine) = &self.audio_engine {
                                engine.toggle_playback();
                                self.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::Backspace) | Key::Named(NamedKey::Delete) => {
                            if !self.selected.is_empty() {
                                self.delete_selected();
                                self.request_redraw();
                            }
                        }
                        Key::Character(ch) if self.modifiers.super_key() => match ch.as_ref() {
                            "," => {
                                self.command_palette = None;
                                self.context_menu = None;
                                self.settings_window = if self.settings_window.is_some() {
                                    None
                                } else {
                                    Some(SettingsWindow::new())
                                };
                                self.request_redraw();
                            }
                            "k" | "t" => {
                                self.context_menu = None;
                                self.settings_window = None;
                                self.command_palette = if self.command_palette.is_some() {
                                    None
                                } else {
                                    Some(CommandPalette::new())
                                };
                                self.request_redraw();
                            }
                            "b" => {
                                self.sample_browser.visible = !self.sample_browser.visible;
                                if self.sample_browser.visible {
                                    self.ensure_plugins_scanned();
                                }
                                self.request_redraw();
                            }
                            "a" if self.modifiers.shift_key() => {
                                self.open_add_folder_dialog();
                            }
                            "r" => {
                                let has_er = self.selected.iter().any(|t| matches!(t, HitTarget::EffectRegion(_)));
                                let has_wf = self.selected.iter().any(|t| matches!(t, HitTarget::Waveform(_)));
                                if has_er {
                                    self.execute_command(CommandAction::RenameEffectRegion);
                                } else if has_wf {
                                    self.execute_command(CommandAction::RenameSample);
                                } else {
                                    self.toggle_recording();
                                }
                                self.request_redraw();
                            }
                            "c" => {
                                self.copy_selected();
                                self.request_redraw();
                            }
                            "v" => {
                                self.paste_clipboard();
                                self.request_redraw();
                            }
                            "d" => {
                                self.duplicate_selected();
                                self.request_redraw();
                            }
                            "s" => self.save_project(),
                            "z" => {
                                if self.modifiers.shift_key() {
                                    self.redo();
                                } else {
                                    self.undo();
                                }
                            }
                            "1" => {
                                self.execute_command(CommandAction::NarrowGrid);
                            }
                            "2" => {
                                self.execute_command(CommandAction::WidenGrid);
                            }
                            "3" => {
                                self.execute_command(CommandAction::ToggleTripletGrid);
                            }
                            "4" => {
                                self.execute_command(CommandAction::ToggleSnapToGrid);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }

            // --- scroll = pan, Cmd+scroll = zoom, pinch = zoom ---
            WindowEvent::MouseWheel { delta, .. } => {
                if self.command_palette.is_some() {
                    return;
                }
                let is_pixel_delta = matches!(delta, MouseScrollDelta::PixelDelta(_));
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 50.0, y * 50.0),
                    MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };

                if self.sample_browser.visible {
                    let (_, sh, scale) = self.screen_info();
                    if self.sample_browser.contains(self.mouse_pos, sh, scale) {
                        if is_pixel_delta {
                            self.sample_browser.scroll_direct(dy, sh, scale);
                        } else {
                            self.sample_browser.scroll(dy, sh, scale);
                        }
                        self.sample_browser.update_hover(self.mouse_pos, sh, scale);
                        self.request_redraw();
                        return;
                    }
                }

                if self.modifiers.super_key() {
                    let zoom_sensitivity = 0.005;
                    let factor = (1.0 + dy * zoom_sensitivity).clamp(0.5, 2.0);
                    self.camera.zoom_at(self.mouse_pos, factor);
                } else {
                    self.camera.position[0] -= dx / self.camera.zoom;
                    self.camera.position[1] -= dy / self.camera.zoom;
                }

                self.update_hover();
                self.request_redraw();
            }

            WindowEvent::PinchGesture { delta, .. } => {
                if self.command_palette.is_some() {
                    return;
                }
                let factor = (1.0 + delta as f32).clamp(0.5, 2.0);
                self.camera.zoom_at(self.mouse_pos, factor);
                self.update_hover();
                self.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                self.update_recording_waveform();
                let (_pre_w, pre_h, pre_scale) = self.screen_info();
                self.sample_browser.extra_content_height = self.plugin_browser.section_height(pre_scale);
                let plugin_section_y = self.plugin_section_y_offset(pre_h, pre_scale);
                let plugin_panel_w = self.sample_browser.panel_width(pre_scale);
                let clip_top = browser::HEADER_HEIGHT * pre_scale;
                if self.sample_browser.visible && !self.plugin_browser.plugins.is_empty() {
                    self.plugin_browser.get_text_entries(plugin_panel_w, plugin_section_y, pre_scale, clip_top, pre_h);
                }
                if let Some(gpu) = &mut self.gpu {
                    let w = gpu.config.width as f32;
                    let h = gpu.config.height as f32;

                    let sel_rect = if let DragState::Selecting { start_world } = &self.drag {
                        Some((*start_world, self.camera.screen_to_world(self.mouse_pos)))
                    } else {
                        None
                    };

                    let playhead_world_x = self
                        .audio_engine
                        .as_ref()
                        .map(|e| (e.position_seconds() * PIXELS_PER_SECOND as f64) as f32);

                    let render_ctx = RenderContext {
                        camera: &self.camera,
                        screen_w: w,
                        screen_h: h,
                        objects: &self.objects,
                        waveforms: &self.waveforms,
                        effect_regions: &self.effect_regions,
                        hovered: self.hovered,
                        selected: &self.selected,
                        selection_rect: sel_rect,
                        file_hovering: self.file_hovering,
                        playhead_world_x,
                        export_region: self.export_region.as_ref(),
                        components: &self.components,
                        component_instances: &self.component_instances,
                        editing_component: self.editing_component,
                        settings: &self.settings,
                    };
                    let instances = build_instances(&render_ctx);
                    let wf_verts = build_waveform_vertices(&render_ctx);

                    if self.sample_browser.visible {
                        self.sample_browser.get_text_entries(h, gpu.scale_factor);
                    }
                    let browser_ref = if self.sample_browser.visible {
                        Some(&self.sample_browser)
                    } else {
                        None
                    };

                    let drag_ghost =
                        if let DragState::DraggingFromBrowser { ref filename, .. } = self.drag {
                            Some((filename.as_str(), self.mouse_pos))
                        } else if let DragState::DraggingPlugin { ref plugin_name, .. } = self.drag {
                            Some((plugin_name.as_str(), self.mouse_pos))
                        } else {
                            None
                        };

                    if let Some(p) = &mut self.command_palette {
                        if p.mode == PaletteMode::VolumeFader {
                            p.fader_rms =
                                self.audio_engine.as_ref().map_or(0.0, |e| e.rms_peak());
                        }
                    }

                    let is_playing = self.audio_engine.as_ref().map_or(false, |e| e.is_playing());
                    let playback_pos = self
                        .audio_engine
                        .as_ref()
                        .map_or(0.0, |e| e.position_seconds());
                    let is_recording = self.recorder.as_ref().map_or(false, |r| r.is_recording());

                    let plugin_browser_ref = if self.sample_browser.visible && !self.plugin_browser.plugins.is_empty() {
                        Some((&self.plugin_browser, plugin_section_y))
                    } else {
                        None
                    };

                    gpu.render(
                        &self.camera,
                        &instances,
                        &wf_verts,
                        self.command_palette.as_ref(),
                        self.context_menu.as_ref(),
                        browser_ref,
                        plugin_browser_ref,
                        drag_ghost,
                        is_playing,
                        is_recording,
                        playback_pos,
                        self.export_region.as_ref(),
                        &self.effect_regions,
                        self.editing_effect_name.as_ref().map(|(idx, s)| (*idx, s.as_str())),
                        &self.waveforms,
                        self.editing_waveform_name.as_ref().map(|(idx, s)| (*idx, s.as_str())),
                        self.plugin_editor.as_ref(),
                        self.settings_window.as_ref(),
                        &self.settings,
                    );
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Native macOS menu bar
// ---------------------------------------------------------------------------

fn build_app_menu(storage: Option<&Storage>) -> MenuState {
    use muda::{
        accelerator::{Accelerator, Code, Modifiers},
        Menu, MenuItem, PredefinedMenuItem, Submenu,
    };

    let menu = Menu::new();

    // -- App menu (Layers) --
    let app_menu = Submenu::new("Layers", true);
    let _ = app_menu.append(&PredefinedMenuItem::about(None, None));
    let _ = app_menu.append(&PredefinedMenuItem::separator());
    let settings_item = MenuItem::new(
        "Settings...",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)),
    );
    let _ = app_menu.append(&settings_item);
    let _ = app_menu.append(&PredefinedMenuItem::separator());
    let _ = app_menu.append(&PredefinedMenuItem::quit(None));
    let _ = menu.append(&app_menu);

    // -- File menu --
    let file_menu = Submenu::new("File", true);
    let new_project_item = MenuItem::new(
        "New Project",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyN)),
    );
    let _ = file_menu.append(&new_project_item);
    let _ = file_menu.append(&PredefinedMenuItem::separator());
    let save_project_item = MenuItem::new(
        "Save Project",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyS)),
    );
    let _ = file_menu.append(&save_project_item);
    let _ = file_menu.append(&PredefinedMenuItem::separator());

    let open_submenu = Submenu::new("Open Project", true);
    let mut open_items: Vec<(MenuId, String)> = Vec::new();
    if let Some(s) = storage {
        for entry in s.list_projects() {
            let item = MenuItem::new(&entry.name, true, None);
            open_items.push((item.id().clone(), entry.project_id));
            let _ = open_submenu.append(&item);
        }
    }
    if open_items.is_empty() {
        let _ = open_submenu.append(&MenuItem::new("No Projects", false, None));
    }
    let _ = file_menu.append(&open_submenu);
    let _ = menu.append(&file_menu);

    // -- Edit menu --
    let edit_menu = Submenu::new("Edit", true);
    let _ = edit_menu.append(&PredefinedMenuItem::undo(None));
    let _ = edit_menu.append(&PredefinedMenuItem::redo(None));
    let _ = edit_menu.append(&PredefinedMenuItem::separator());
    let _ = edit_menu.append(&PredefinedMenuItem::copy(None));
    let _ = edit_menu.append(&PredefinedMenuItem::paste(None));
    let _ = edit_menu.append(&PredefinedMenuItem::separator());
    let _ = edit_menu.append(&PredefinedMenuItem::select_all(None));
    let _ = menu.append(&edit_menu);

    MenuState {
        menu,
        new_project: new_project_item.id().clone(),
        save_project: save_project_item.id().clone(),
        settings: settings_item.id().clone(),
        open_project_items: open_items,
        open_submenu,
        initialized: false,
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    env_logger::init();

    println!("╔════════════════════════════════════════════╗");
    println!("║              Layers                         ║");
    println!("╠════════════════════════════════════════════╣");
    println!("║  Space              →  Play / Pause        ║");
    println!("║  Click background   →  Seek playhead       ║");
    println!("║  Drop audio file    →  Add to canvas       ║");
    println!("║  Two-finger scroll  →  Pan canvas          ║");
    println!("║  Cmd + scroll       →  Zoom in/out         ║");
    println!("║  Pinch              →  Zoom in/out         ║");
    println!("║  Middle drag        →  Pan canvas          ║");
    println!("║  Left drag empty    →  Selection rectangle ║");
    println!("║  Left drag object   →  Move (+ selection)  ║");
    println!("║  Cmd + K / Right-click → Command palette   ║");
    println!("║  Backspace / Delete →  Delete selected     ║");
    println!("║  Cmd + Z / ⇧⌘Z     →  Undo / Redo         ║");
    println!("║  Cmd + S            →  Save project        ║");
    println!("║  Cmd + B            →  Toggle browser      ║");
    println!("║  Cmd + Shift + A    →  Add folder           ║");
    println!("╚════════════════════════════════════════════╝");

    let event_loop = EventLoop::new().unwrap();

    let mut app = App::new();
    let menu_state = build_app_menu(app.storage.as_ref());
    app.menu_state = Some(menu_state);

    event_loop.run_app(&mut app).unwrap();
}
