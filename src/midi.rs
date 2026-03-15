use crate::hit_testing::point_in_rect;
use crate::{push_border, Camera, InstanceRaw};

// ---------------------------------------------------------------------------
// MIDI data types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct MidiNote {
    pub pitch: u8,       // MIDI note 0-127
    pub start_px: f32,   // relative to clip left edge, in pixels
    pub duration_px: f32, // in pixels
    pub velocity: u8,    // 0-127, default 100
}

#[derive(Clone)]
pub struct MidiClip {
    pub position: [f32; 2],
    pub size: [f32; 2],     // [width, height]
    pub color: [f32; 4],
    pub notes: Vec<MidiNote>,
    pub pitch_range: (u8, u8), // (low, high) e.g. (48, 84) = C3-C6
}

pub const MIDI_CLIP_DEFAULT_SIZE: [f32; 2] = [480.0, 200.0];
pub const MIDI_CLIP_DEFAULT_COLOR: [f32; 4] = [0.60, 0.30, 0.90, 0.70];
pub const MIDI_CLIP_DEFAULT_PITCH_RANGE: (u8, u8) = (48, 84); // C3-C6
pub const DEFAULT_NOTE_DURATION_PX: f32 = 30.0;

impl MidiClip {
    pub fn new(position: [f32; 2]) -> Self {
        Self {
            position,
            size: MIDI_CLIP_DEFAULT_SIZE,
            color: MIDI_CLIP_DEFAULT_COLOR,
            notes: Vec::new(),
            pitch_range: MIDI_CLIP_DEFAULT_PITCH_RANGE,
        }
    }

    pub fn note_height(&self) -> f32 {
        let range = (self.pitch_range.1 - self.pitch_range.0) as f32;
        if range > 0.0 {
            self.size[1] / range
        } else {
            self.size[1]
        }
    }

    /// Convert a pitch to Y position (world coords) within this clip.
    pub fn pitch_to_y(&self, pitch: u8) -> f32 {
        let nh = self.note_height();
        self.position[1] + self.size[1] - (pitch as f32 - self.pitch_range.0 as f32) * nh - nh
    }

    /// Convert a world Y position to pitch.
    pub fn y_to_pitch(&self, y: f32) -> u8 {
        let nh = self.note_height();
        let relative = self.position[1] + self.size[1] - y;
        let pitch = (relative / nh) as u8 + self.pitch_range.0;
        pitch.clamp(self.pitch_range.0, self.pitch_range.1 - 1)
    }

    /// Convert a world X position to start_px (relative to clip left).
    pub fn x_to_start_px(&self, x: f32) -> f32 {
        (x - self.position[0]).max(0.0)
    }

    pub fn contains(&self, world_pos: [f32; 2]) -> bool {
        point_in_rect(world_pos, self.position, self.size)
    }
}

// ---------------------------------------------------------------------------
// Hit testing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MidiNoteHitZone {
    Body,
    RightEdge,
}

const NOTE_EDGE_HIT_PX: f32 = 5.0;

pub fn hit_test_midi_note(
    clip: &MidiClip,
    world_pos: [f32; 2],
    camera: &Camera,
) -> Option<(usize, MidiNoteHitZone)> {
    let nh = clip.note_height();
    let edge_margin = NOTE_EDGE_HIT_PX / camera.zoom;

    for (i, note) in clip.notes.iter().enumerate().rev() {
        let nx = clip.position[0] + note.start_px;
        let ny = clip.pitch_to_y(note.pitch);
        let nw = note.duration_px;
        if point_in_rect(world_pos, [nx, ny], [nw, nh]) {
            if (world_pos[0] - (nx + nw)).abs() < edge_margin {
                return Some((i, MidiNoteHitZone::RightEdge));
            }
            return Some((i, MidiNoteHitZone::Body));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn build_midi_clip_instances(
    clip: &MidiClip,
    camera: &Camera,
    is_hovered: bool,
    is_selected: bool,
) -> Vec<InstanceRaw> {
    let mut out = Vec::new();

    // Background rectangle
    let mut bg_color = [clip.color[0], clip.color[1], clip.color[2], 0.15];
    if is_hovered && !is_selected {
        bg_color[3] = 0.22;
    }
    out.push(InstanceRaw {
        position: clip.position,
        size: clip.size,
        color: bg_color,
        border_radius: 4.0 / camera.zoom,
    });

    // Border
    let bw = if is_selected { 2.5 } else { 1.5 } / camera.zoom;
    let mut bc = clip.color;
    bc[3] = 0.60;
    if is_hovered && !is_selected {
        bc[3] = 0.75;
    }
    push_border(&mut out, clip.position, clip.size, bw, bc);

    // Pitch grid lines (for black keys)
    let nh = clip.note_height();
    for pitch in clip.pitch_range.0..clip.pitch_range.1 {
        let y = clip.pitch_to_y(pitch);
        let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        if is_black {
            out.push(InstanceRaw {
                position: [clip.position[0], y],
                size: [clip.size[0], nh],
                color: [0.0, 0.0, 0.0, 0.10],
                border_radius: 0.0,
            });
        }
        // Subtle line at each C note
        if pitch % 12 == 0 {
            let line_h = 1.0 / camera.zoom;
            out.push(InstanceRaw {
                position: [clip.position[0], y + nh - line_h * 0.5],
                size: [clip.size[0], line_h],
                color: [1.0, 1.0, 1.0, 0.12],
                border_radius: 0.0,
            });
        }
    }

    out
}

pub fn build_midi_note_instances(
    clip: &MidiClip,
    camera: &Camera,
    selected_notes: &[usize],
    _editing: bool,
) -> Vec<InstanceRaw> {
    let mut out = Vec::new();
    let nh = clip.note_height();

    for (i, note) in clip.notes.iter().enumerate() {
        let nx = clip.position[0] + note.start_px;
        let ny = clip.pitch_to_y(note.pitch);
        let nw = note.duration_px;

        // Velocity-based brightness
        let vel_factor = note.velocity as f32 / 127.0;
        let brightness = 0.5 + vel_factor * 0.5;
        let note_color = [
            clip.color[0] * brightness,
            clip.color[1] * brightness,
            (clip.color[2] * brightness).min(1.0),
            0.85,
        ];

        out.push(InstanceRaw {
            position: [nx, ny],
            size: [nw, nh.max(2.0 / camera.zoom)],
            color: note_color,
            border_radius: 2.0 / camera.zoom,
        });

        // Selection highlight
        if selected_notes.contains(&i) {
            let sel_bw = 1.5 / camera.zoom;
            push_border(&mut out, [nx, ny], [nw, nh], sel_bw, [1.0, 1.0, 1.0, 0.8]);
        }
    }

    out
}

/// Get note name from MIDI pitch (for piano roll labels)
pub fn note_name(pitch: u8) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (pitch / 12) as i32 - 1;
    let name = names[(pitch % 12) as usize];
    format!("{}{}", name, octave)
}
