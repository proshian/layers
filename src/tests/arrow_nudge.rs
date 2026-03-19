use std::sync::Arc;

use crate::automation::AutomationData;
use crate::entity_id::new_id;
use crate::grid;
use crate::ui::waveform::{AudioData, WarpMode, WaveformPeaks, WaveformView, DEFAULT_AUTO_FADE_PX};
use crate::{App, HitTarget};

fn make_waveform(x: f32, y: f32) -> WaveformView {
    WaveformView {
        audio: Arc::new(AudioData {
            left_samples: Arc::new(Vec::new()),
            right_samples: Arc::new(Vec::new()),
            left_peaks: Arc::new(WaveformPeaks::empty()),
            right_peaks: Arc::new(WaveformPeaks::empty()),
            sample_rate: 48000,
            filename: "test.wav".to_string(),
        }),
        filename: "test.wav".to_string(),
        position: [x, y],
        size: [200.0, 80.0],
        color: [0.0, 1.0, 0.0, 1.0],
        border_radius: 4.0,
        fade_in_px: 0.0,
        fade_out_px: 0.0,
        fade_in_curve: 0.5,
        fade_out_curve: 0.5,
        volume: 1.0,
        pan: 0.5,
        warp_mode: WarpMode::Off,
        sample_bpm: 120.0,
        pitch_semitones: 0.0,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
    }
}

#[test]
fn test_horizontal_nudge() {
    let mut app = App::new_headless();
    let id = new_id();
    let start_x = 100.0;
    app.waveforms.insert(id, make_waveform(start_x, 200.0));
    app.selected.push(HitTarget::Waveform(id));

    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    app.nudge_selection(step, 0.0);

    let pos = app.waveforms[&id].position;
    assert!((pos[0] - (start_x + step)).abs() < 0.01, "x should be shifted right by one grid step");
    assert!((pos[1] - 200.0).abs() < 0.01, "y should be unchanged");
}

#[test]
fn test_vertical_nudge() {
    let mut app = App::new_headless();
    let id = new_id();
    let start_y = 300.0;
    app.waveforms.insert(id, make_waveform(100.0, start_y));
    app.selected.push(HitTarget::Waveform(id));

    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    app.nudge_selection(0.0, -step);

    let pos = app.waveforms[&id].position;
    assert!((pos[0] - 100.0).abs() < 0.01, "x should be unchanged");
    assert!((pos[1] - (start_y - step)).abs() < 0.01, "y should be shifted up by one grid step");
}

#[test]
fn test_multi_select_nudge() {
    let mut app = App::new_headless();
    let id1 = new_id();
    let id2 = new_id();
    app.waveforms.insert(id1, make_waveform(100.0, 200.0));
    app.waveforms.insert(id2, make_waveform(400.0, 500.0));
    app.selected.push(HitTarget::Waveform(id1));
    app.selected.push(HitTarget::Waveform(id2));

    let dx = 50.0;
    app.nudge_selection(dx, 0.0);

    assert!((app.waveforms[&id1].position[0] - 150.0).abs() < 0.01);
    assert!((app.waveforms[&id2].position[0] - 450.0).abs() < 0.01);
    // Y unchanged
    assert!((app.waveforms[&id1].position[1] - 200.0).abs() < 0.01);
    assert!((app.waveforms[&id2].position[1] - 500.0).abs() < 0.01);
}

#[test]
fn test_undo_coalescing() {
    let mut app = App::new_headless();
    let id = new_id();
    app.waveforms.insert(id, make_waveform(100.0, 200.0));
    app.selected.push(HitTarget::Waveform(id));

    let undo_len_before = app.op_undo_stack.len();

    // Two rapid nudges (within 500ms — immediate calls coalesce)
    app.nudge_selection(10.0, 0.0);
    app.nudge_selection(10.0, 0.0);

    // Position should reflect both nudges
    assert!((app.waveforms[&id].position[0] - 120.0).abs() < 0.01);

    // Commit the coalesced nudge
    app.commit_arrow_nudge();

    // Should be exactly one new undo entry
    assert_eq!(app.op_undo_stack.len(), undo_len_before + 1);

    // Undo should restore original position and preserve selection
    app.undo_op();
    assert!((app.waveforms[&id].position[0] - 100.0).abs() < 0.01);
    assert_eq!(app.selected.len(), 1, "selection should be preserved after undo");
    assert_eq!(app.selected[0], HitTarget::Waveform(id));
}
