use crate::entity_id::EntityId;
use crate::InstanceRaw;
use crate::ui::palette::{gain_to_db, gain_to_fader_pos, fader_pos_to_gain};

pub const RIGHT_WINDOW_WIDTH: f32 = 200.0;
const HEADER_HEIGHT: f32 = 36.0;
const KNOB_R: f32 = 22.0;
const KNOB_DOT_R: f32 = 2.5;
const KNOB_INDICATOR_R: f32 = 3.5;
const ARC_DOTS: usize = 30;

const BG_COLOR: [f32; 4] = [0.11, 0.11, 0.14, 1.0];
const HEADER_BG: [f32; 4] = [0.13, 0.13, 0.17, 1.0];
const BLUE: [f32; 4] = [0.25, 0.55, 1.0, 1.0];
const DOT_INACTIVE: [f32; 4] = [0.25, 0.25, 0.30, 1.0];

pub struct RightWindow {
    pub waveform_id: EntityId,
    pub volume: f32,
    pub pan: f32,
    pub vol_dragging: bool,
    pub pan_dragging: bool,
    pub drag_start_y: f32,
    pub drag_start_value: f32,
}

impl RightWindow {
    pub fn panel_rect(screen_w: f32, screen_h: f32, scale: f32) -> ([f32; 2], [f32; 2]) {
        let w = RIGHT_WINDOW_WIDTH * scale;
        let h = screen_h;
        ([screen_w - w, 0.0], [w, h])
    }

    fn vol_knob_center(screen_w: f32, screen_h: f32, scale: f32) -> [f32; 2] {
        let (pp, ps) = Self::panel_rect(screen_w, screen_h, scale);
        [pp[0] + ps[0] * 0.5, pp[1] + HEADER_HEIGHT * scale + 70.0 * scale]
    }

    fn pan_knob_center(screen_w: f32, screen_h: f32, scale: f32) -> [f32; 2] {
        let (pp, ps) = Self::panel_rect(screen_w, screen_h, scale);
        [pp[0] + ps[0] * 0.5, pp[1] + HEADER_HEIGHT * scale + 190.0 * scale]
    }

    pub fn hit_test_vol_knob(&self, pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let c = Self::vol_knob_center(screen_w, screen_h, scale);
        let r = (KNOB_R + 8.0) * scale;
        let dx = pos[0] - c[0];
        let dy = pos[1] - c[1];
        dx * dx + dy * dy <= r * r
    }

    pub fn hit_test_pan_knob(&self, pos: [f32; 2], screen_w: f32, screen_h: f32, scale: f32) -> bool {
        let c = Self::pan_knob_center(screen_w, screen_h, scale);
        let r = (KNOB_R + 8.0) * scale;
        let dx = pos[0] - c[0];
        let dy = pos[1] - c[1];
        dx * dx + dy * dy <= r * r
    }

    fn value_to_angle(v: f32) -> f32 {
        // 225° (7-o'clock) to 315° (5-o'clock), 270° sweep
        let deg = 225.0 + v.clamp(0.0, 1.0) * 270.0;
        deg.to_radians()
    }

    fn push_knob(out: &mut Vec<InstanceRaw>, cx: f32, cy: f32, value: f32, scale: f32) {
        let kr = KNOB_R * scale;
        let dot_r = KNOB_DOT_R * scale;
        let ind_r = KNOB_INDICATOR_R * scale;

        // Knob background circle
        out.push(InstanceRaw {
            position: [cx - kr, cy - kr],
            size: [kr * 2.0, kr * 2.0],
            color: [0.18, 0.18, 0.22, 1.0],
            border_radius: kr,
        });

        // Arc dots
        for i in 0..ARC_DOTS {
            let t = i as f32 / ARC_DOTS as f32;
            let angle = Self::value_to_angle(t);
            let arc_r = (KNOB_R - 6.0) * scale;
            let dx = angle.cos() * arc_r;
            let dy = angle.sin() * arc_r;
            let color = if t < value { BLUE } else { DOT_INACTIVE };
            out.push(InstanceRaw {
                position: [cx + dx - dot_r, cy + dy - dot_r],
                size: [dot_r * 2.0, dot_r * 2.0],
                color,
                border_radius: dot_r,
            });
        }

        // Indicator dot
        let ind_angle = Self::value_to_angle(value);
        let ind_arc_r = (KNOB_R - 6.0) * scale;
        let idx = ind_angle.cos() * ind_arc_r;
        let idy = ind_angle.sin() * ind_arc_r;
        out.push(InstanceRaw {
            position: [cx + idx - ind_r, cy + idy - ind_r],
            size: [ind_r * 2.0, ind_r * 2.0],
            color: [1.0, 1.0, 1.0, 0.95],
            border_radius: ind_r,
        });
    }

    pub fn build_instances(&self, screen_w: f32, screen_h: f32, scale: f32) -> Vec<InstanceRaw> {
        let mut out = Vec::new();
        let (pp, ps) = Self::panel_rect(screen_w, screen_h, scale);

        // Left separator line
        out.push(InstanceRaw {
            position: [pp[0], pp[1]],
            size: [1.0 * scale, ps[1]],
            color: [1.0, 1.0, 1.0, 0.06],
            border_radius: 0.0,
        });

        // Panel background
        out.push(InstanceRaw {
            position: [pp[0] + 1.0 * scale, pp[1]],
            size: [ps[0] - 1.0 * scale, ps[1]],
            color: BG_COLOR,
            border_radius: 0.0,
        });

        // Header background
        out.push(InstanceRaw {
            position: [pp[0] + 1.0 * scale, pp[1]],
            size: [ps[0] - 1.0 * scale, HEADER_HEIGHT * scale],
            color: HEADER_BG,
            border_radius: 0.0,
        });

        // Header divider
        out.push(InstanceRaw {
            position: [pp[0] + 1.0 * scale, pp[1] + HEADER_HEIGHT * scale],
            size: [ps[0] - 1.0 * scale, 1.0 * scale],
            color: [1.0, 1.0, 1.0, 0.06],
            border_radius: 0.0,
        });

        // Volume knob
        let vol_pos = gain_to_fader_pos(self.volume);
        let vc = Self::vol_knob_center(screen_w, screen_h, scale);
        Self::push_knob(&mut out, vc[0], vc[1], vol_pos, scale);

        // Pan knob
        let pc = Self::pan_knob_center(screen_w, screen_h, scale);
        Self::push_knob(&mut out, pc[0], pc[1], self.pan, scale);

        out
    }

    /// Format volume value as dB string
    pub fn vol_text(&self) -> String {
        if self.volume < 0.00001 {
            "Mute".to_string()
        } else {
            let db = gain_to_db(self.volume);
            if db >= 0.0 {
                format!("+{:.1} dB", db)
            } else {
                format!("{:.1} dB", db)
            }
        }
    }

    /// Format pan value as string
    pub fn pan_text(&self) -> String {
        let p = self.pan;
        if (p - 0.5).abs() < 0.01 {
            "C".to_string()
        } else if p < 0.5 {
            let pct = ((0.5 - p) * 200.0).round() as u32;
            format!("L {}%", pct)
        } else {
            let pct = ((p - 0.5) * 200.0).round() as u32;
            format!("R {}%", pct)
        }
    }

    pub fn vol_knob_center_pub(screen_w: f32, screen_h: f32, scale: f32) -> [f32; 2] {
        Self::vol_knob_center(screen_w, screen_h, scale)
    }

    pub fn pan_knob_center_pub(screen_w: f32, screen_h: f32, scale: f32) -> [f32; 2] {
        Self::pan_knob_center(screen_w, screen_h, scale)
    }

    /// Compute new volume from drag delta (up = increase)
    pub fn drag_vol_delta(drag_start_y: f32, mouse_y: f32, drag_start_value: f32, scale: f32) -> f32 {
        let delta = (drag_start_y - mouse_y) / (200.0 * scale);
        let new_pos = (drag_start_value + delta).clamp(0.0, 1.0);
        fader_pos_to_gain(new_pos)
    }

    /// Compute new pan from drag delta (up = increase)
    pub fn drag_pan_delta(drag_start_y: f32, mouse_y: f32, drag_start_value: f32, scale: f32) -> f32 {
        let delta = (drag_start_y - mouse_y) / (200.0 * scale);
        (drag_start_value + delta).clamp(0.0, 1.0)
    }
}
