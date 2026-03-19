// ---------------------------------------------------------------------------
// src/theme.rs — centralized color palette
// ---------------------------------------------------------------------------
// Every visual color used in the app lives here as a named constant so the
// entire color scheme can be changed in one place.
//
// Groups:
//   Backgrounds · Accents · Interactive · Transport · Scrollbars · RMS
//   Regions · Entity colors · Waveform palette · Helper

// --- Backgrounds ---
pub const BG_BASE: [f32; 4]    = [0.11, 0.11, 0.14, 1.0]; // main canvas, panels
pub const BG_SURFACE: [f32; 4] = [0.13, 0.13, 0.17, 1.0]; // headers, elevated panels
pub const BG_MENU: [f32; 4]    = [0.16, 0.16, 0.19, 1.0]; // context menu background
pub const BG_OVERLAY: [f32; 4] = [0.14, 0.14, 0.17, 0.98]; // palette, modal overlays

// --- Accents ---
pub const ACCENT: [f32; 4]       = [0.25, 0.55, 1.0, 1.0];  // primary blue
pub const ACCENT_MUTED: [f32; 4] = [0.25, 0.50, 0.90, 0.60]; // badges, pills
pub const ACCENT_FAINT: [f32; 4] = [0.25, 0.55, 1.0, 0.08]; // loop region fill

// --- Interactive ---
pub const HOVER: [f32; 4]     = [1.0, 1.0, 1.0, 0.06];
pub const SELECTION: [f32; 4] = [0.35, 0.65, 1.0, 0.8];

// --- Playhead & Transport ---
pub const PLAYHEAD: [f32; 4]      = [0.20, 0.80, 0.60, 0.9];
pub const RECORD_ACTIVE: [f32; 4] = [0.95, 0.20, 0.20, 1.0];
pub const RECORD_DIM: [f32; 4]    = [0.85, 0.25, 0.25, 0.9];

// --- Scrollbars ---
pub const SCROLLBAR_BG: [f32; 4]    = [1.0, 1.0, 1.0, 0.08];
pub const SCROLLBAR_THUMB: [f32; 4] = [1.0, 1.0, 1.0, 0.20];

// --- RMS meter ---
pub const RMS_LOW: [f32; 4]  = [0.45, 0.92, 0.55, 1.0]; // green
pub const RMS_MID: [f32; 4]  = [1.0, 0.85, 0.32, 1.0];  // yellow
pub const RMS_HIGH: [f32; 4] = [1.0, 0.35, 0.30, 1.0];  // red

// --- Browser-specific UI ---
pub const CHEVRON: [f32; 4]          = [1.0, 1.0, 1.0, 0.40];
pub const ADD_BTN_NORMAL: [f32; 4]   = [1.0, 1.0, 1.0, 0.50];
pub const ADD_BTN_HOVER: [f32; 4]    = [1.0, 1.0, 1.0, 0.80];
pub const BG_PLUGIN: [f32; 4]        = [0.10, 0.12, 0.16, 1.0];
pub const BG_PLUGIN_HEADER: [f32; 4] = [0.11, 0.14, 0.20, 1.0];

// --- Region colors (export / loop) ---
pub const EXPORT_FILL_COLOR: [f32; 4]       = [0.15, 0.70, 0.55, 0.10];
pub const EXPORT_BORDER_COLOR: [f32; 4]     = [0.20, 0.80, 0.60, 0.50];
pub const EXPORT_RENDER_PILL_COLOR: [f32; 4] = [0.15, 0.65, 0.50, 0.85];
pub const LOOP_FILL_COLOR: [f32; 4]         = [0.25, 0.55, 0.95, 0.08];
pub const LOOP_BORDER_COLOR: [f32; 4]       = [0.30, 0.60, 1.0, 0.50];
pub const LOOP_BADGE_COLOR: [f32; 4]        = [0.20, 0.50, 0.95, 0.85];

// --- Component entity ---
pub const COMPONENT_BORDER_COLOR: [f32; 4]  = [0.85, 0.55, 0.20, 0.50];
pub const COMPONENT_FILL_COLOR: [f32; 4]    = [0.85, 0.55, 0.20, 0.06];
pub const COMPONENT_BADGE_COLOR: [f32; 4]   = [0.85, 0.55, 0.20, 0.70];
pub const INSTANCE_FILL_COLOR: [f32; 4]     = [0.85, 0.55, 0.20, 0.04];
pub const INSTANCE_BORDER_COLOR: [f32; 4]   = [0.85, 0.55, 0.20, 0.30];
pub const LOCK_ICON_COLOR: [f32; 4]         = [0.85, 0.55, 0.20, 0.60];

// --- Effect entity ---
pub const EFFECT_BORDER_COLOR: [f32; 4]    = [0.25, 0.50, 0.90, 0.50];
pub const EFFECT_ACTIVE_BORDER: [f32; 4]   = [0.35, 0.60, 1.00, 0.70];
pub const PLUGIN_BLOCK_DEFAULT_COLOR: [f32; 4] = [0.25, 0.50, 0.90, 0.70];

// --- Instrument entity ---
pub const INSTRUMENT_BORDER_COLOR: [f32; 4] = [0.60, 0.30, 0.90, 0.50];
pub const INSTRUMENT_ACTIVE_BORDER: [f32; 4] = [0.70, 0.40, 1.00, 0.70];

// --- MIDI ---
pub const MIDI_CLIP_DEFAULT_COLOR: [f32; 4] = [0.60, 0.30, 0.90, 0.70];

// --- Waveform palette (16 slot color wheel) ---
pub const WAVEFORM_COLORS: &[[f32; 4]] = &[
    [1.00, 0.24, 0.19, 1.0], // red
    [1.00, 0.42, 0.24, 1.0], // orange-red
    [1.00, 0.58, 0.00, 1.0], // orange
    [1.00, 0.72, 0.00, 1.0], // amber
    [1.00, 0.84, 0.00, 1.0], // yellow
    [0.78, 0.90, 0.19, 1.0], // lime
    [0.30, 0.85, 0.39, 1.0], // green
    [0.19, 0.84, 0.55, 1.0], // mint
    [0.19, 0.78, 0.71, 1.0], // teal
    [0.19, 0.78, 0.90, 1.0], // cyan
    [0.35, 0.78, 0.98, 1.0], // sky blue
    [0.00, 0.48, 1.00, 1.0], // blue
    [0.35, 0.34, 0.84, 1.0], // indigo
    [0.69, 0.32, 0.87, 1.0], // violet
    [0.88, 0.25, 0.63, 1.0], // magenta
    [1.00, 0.18, 0.33, 1.0], // rose
];

// --- Helper ---
/// Return a copy of `c` with alpha replaced by `a`.
#[inline]
pub fn with_alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

// ---------------------------------------------------------------------------
// Runtime theme (hue-parameterized)
// ---------------------------------------------------------------------------

const PRIMARY_HUE: f32 = 216.0;
const LIGHTNESS_OFFSET: f32 = 0.0;
const OFFSET_COMPONENT: f32 = 30.0;

/// Convert HSL + alpha to a linear [f32; 4] colour.
/// h in [0, 360), s and l in [0, 1], a in [0, 1].
pub fn hsl(h: f32, s: f32, l: f32, a: f32) -> [f32; 4] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    [r1 + m, g1 + m, b1 + m, a]
}

pub fn wrap_hue(h: f32) -> f32 {
    ((h % 360.0) + 360.0) % 360.0
}

#[derive(Clone)]
pub struct RuntimeTheme {
    pub bg_base: [f32; 4],
    pub bg_surface: [f32; 4],
    pub bg_menu: [f32; 4],
    pub bg_overlay: [f32; 4],
    pub bg_elevated: [f32; 4],
    pub bg_input: [f32; 4],
    pub bg_dropdown: [f32; 4],
    pub bg_panel: [f32; 4],
    pub bg_window: [f32; 4],
    pub bg_sidebar: [f32; 4],
    pub bg_window_header: [f32; 4],
    pub bg_plugin: [f32; 4],
    pub bg_plugin_header: [f32; 4],
    pub accent: [f32; 4],
    pub accent_muted: [f32; 4],
    pub accent_faint: [f32; 4],
    pub selection: [f32; 4],
    pub border_subtle: [f32; 4],
    pub item_hover: [f32; 4],
    pub item_active: [f32; 4],
    pub option_highlight: [f32; 4],
    pub pill_active: [f32; 4],
    pub pill_inactive: [f32; 4],
    pub slider_fill: [f32; 4],
    pub knob_inactive: [f32; 4],
    pub drop_zone_fill: [f32; 4],
    pub drop_zone_border: [f32; 4],
    pub select_rect_fill: [f32; 4],
    pub select_rect_border: [f32; 4],
    pub select_outline: [f32; 4],
    pub loop_fill_color: [f32; 4],
    pub loop_border_color: [f32; 4],
    pub loop_badge_color: [f32; 4],
    pub export_fill_color: [f32; 4],
    pub export_border_color: [f32; 4],
    pub export_render_pill_color: [f32; 4],
    pub component_border_color: [f32; 4],
    pub component_fill_color: [f32; 4],
    pub component_badge_color: [f32; 4],
    pub instance_fill_color: [f32; 4],
    pub instance_border_color: [f32; 4],
    pub lock_icon_color: [f32; 4],
    pub effect_border_color: [f32; 4],
    pub effect_active_border: [f32; 4],
    pub plugin_block_default_color: [f32; 4],
    pub instrument_border_color: [f32; 4],
    pub instrument_active_border: [f32; 4],
    pub midi_clip_default_color: [f32; 4],
    pub playhead: [f32; 4],
    pub category_dot: [f32; 4],
    pub pill_instrument: [f32; 4],
    pub pill_effect: [f32; 4],
}

impl RuntimeTheme {
    pub fn from_hue(h: f32) -> Self {
        let lo = LIGHTNESS_OFFSET;
        let accent = hsl(h, 1.0, 0.625, 1.0);
        let ch = wrap_hue(h + OFFSET_COMPONENT);
        Self {
            bg_base:          hsl(h, 0.12, 0.125 + lo, 1.0),
            bg_surface:       hsl(h, 0.12, 0.145 + lo, 1.0),
            bg_menu:          hsl(h, 0.10, 0.165 + lo, 1.0),
            bg_overlay:       hsl(h, 0.10, 0.155 + lo, 0.98),
            bg_elevated:      hsl(h, 0.14, 0.175 + lo, 1.0),
            bg_input:         hsl(h, 0.14, 0.165 + lo, 1.0),
            bg_dropdown:      hsl(h, 0.12, 0.170 + lo, 1.0),
            bg_panel:         hsl(h, 0.10, 0.145 + lo, 0.85),
            bg_window:        hsl(h, 0.13, 0.155 + lo, 0.98),
            bg_sidebar:       hsl(h, 0.12, 0.135 + lo, 1.0),
            bg_window_header: hsl(h, 0.16, 0.185 + lo, 1.0),
            bg_plugin:        hsl(h, 0.20, 0.120 + lo, 1.0),
            bg_plugin_header: hsl(h, 0.22, 0.140 + lo, 1.0),
            accent,
            accent_muted:     hsl(h, 0.70, 0.560, 0.60),
            accent_faint:     hsl(h, 1.00, 0.625, 0.08),
            selection:        hsl(h, 0.65, 0.600, 0.80),
            border_subtle:    hsl(h, 0.15, 0.300, 0.12),
            item_hover:       [1.0, 1.0, 1.0, 0.06],
            item_active:      hsl(h, 0.50, 0.300, 0.25),
            option_highlight: hsl(h, 0.50, 0.250, 0.30),
            pill_active:      hsl(h, 0.65, 0.500, 0.85),
            pill_inactive:    hsl(h, 0.15, 0.350, 0.40),
            slider_fill:      hsl(h, 0.65, 0.550, 0.80),
            knob_inactive:    hsl(h, 0.20, 0.350, 0.60),
            drop_zone_fill:   hsl(h, 0.65, 0.580, 0.10),
            drop_zone_border: hsl(h, 0.65, 0.620, 0.60),
            select_rect_fill: hsl(h, 0.65, 0.580, 0.08),
            select_rect_border: hsl(h, 0.60, 0.600, 0.50),
            select_outline:   hsl(h, 0.65, 0.600, 0.70),
            loop_fill_color:  hsl(h, 0.80, 0.580, 0.08),
            loop_border_color: hsl(h, 0.75, 0.620, 0.50),
            loop_badge_color: hsl(h, 0.75, 0.570, 0.85),
            export_fill_color:       hsl(wrap_hue(h + 150.0), 0.70, 0.550, 0.10),
            export_border_color:     hsl(wrap_hue(h + 150.0), 0.75, 0.600, 0.50),
            export_render_pill_color: hsl(wrap_hue(h + 150.0), 0.65, 0.500, 0.85),
            component_border_color:  hsl(ch, 0.75, 0.525, 0.50),
            component_fill_color:    hsl(ch, 0.75, 0.525, 0.06),
            component_badge_color:   hsl(ch, 0.75, 0.525, 0.70),
            instance_fill_color:     hsl(ch, 0.75, 0.525, 0.04),
            instance_border_color:   hsl(ch, 0.75, 0.525, 0.30),
            lock_icon_color:         hsl(ch, 0.75, 0.525, 0.60),
            effect_border_color:     hsl(h, 0.65, 0.560, 0.50),
            effect_active_border:    hsl(h, 0.70, 0.600, 0.70),
            plugin_block_default_color: hsl(h, 0.65, 0.560, 0.70),
            instrument_border_color: hsl(wrap_hue(h + 60.0), 0.65, 0.580, 0.50),
            instrument_active_border: hsl(wrap_hue(h + 60.0), 0.70, 0.620, 0.70),
            midi_clip_default_color: hsl(wrap_hue(h + 60.0), 0.65, 0.580, 0.70),
            playhead:          hsl(wrap_hue(h + 160.0), 0.70, 0.580, 0.90),
            category_dot:      hsl(h, 0.65, 0.580, 0.70),
            pill_instrument:   hsl(wrap_hue(h + 60.0), 0.65, 0.520, 0.85),
            pill_effect:       hsl(h, 0.65, 0.520, 0.85),
        }
    }
}

impl Default for RuntimeTheme {
    fn default() -> Self { Self::from_hue(PRIMARY_HUE) }
}
