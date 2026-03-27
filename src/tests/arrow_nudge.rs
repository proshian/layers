use std::sync::Arc;

use crate::automation::AutomationData;
use crate::entity_id::new_id;
use crate::grid::{self, PIXELS_PER_SECOND};
use crate::ui::hit_testing::WAVEFORM_MIN_WIDTH_PX;
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
            paulstretch_factor: 8.0,
        is_reversed: false,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
    effect_chain_id: None,
    take_group: None,
    }
}

#[test]
fn test_horizontal_nudge() {
    let mut app = App::new_headless();
    let id = new_id();
    // Start on a grid line so nudge by one step lands on the next grid line
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    let start_x = step * 2.0; // on grid
    app.waveforms.insert(id, make_waveform(start_x, 200.0));
    app.selected.push(HitTarget::Waveform(id));

    app.nudge_selection(step, 0.0);

    let pos = app.waveforms[&id].position;
    assert!((pos[0] - (start_x + step)).abs() < 0.01, "x should be shifted right by one grid step");
    assert!((pos[1] - 200.0).abs() < 0.01, "y should be unchanged");
}

#[test]
fn test_vertical_nudge() {
    let mut app = App::new_headless();
    let id = new_id();
    // Vertical snap is off by default, so raw delta is applied
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
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    // Place clips on grid lines so snapping is predictable
    let x1 = step * 2.0;
    let x2 = step * 5.0;
    app.waveforms.insert(id1, make_waveform(x1, 200.0));
    app.waveforms.insert(id2, make_waveform(x2, 500.0));
    app.selected.push(HitTarget::Waveform(id1));
    app.selected.push(HitTarget::Waveform(id2));

    // Nudge by one grid step — both clips should move by exactly one step (group snap)
    app.nudge_selection(step, 0.0);

    assert!((app.waveforms[&id1].position[0] - (x1 + step)).abs() < 0.01,
        "first clip should move by one grid step");
    assert!((app.waveforms[&id2].position[0] - (x2 + step)).abs() < 0.01,
        "second clip should move by the same amount (group snap)");
    // Y unchanged
    assert!((app.waveforms[&id1].position[1] - 200.0).abs() < 0.01);
    assert!((app.waveforms[&id2].position[1] - 500.0).abs() < 0.01);
}

#[test]
fn test_undo_coalescing() {
    let mut app = App::new_headless();
    let id = new_id();
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    let start_x = step * 3.0; // on grid
    app.waveforms.insert(id, make_waveform(start_x, 200.0));
    app.selected.push(HitTarget::Waveform(id));

    let undo_len_before = app.op_undo_stack.len();

    // Two rapid nudges by one grid step each (within 500ms — immediate calls coalesce)
    app.nudge_selection(step, 0.0);
    app.nudge_selection(step, 0.0);

    // Position should reflect both nudges (two grid steps)
    let expected = start_x + step * 2.0;
    assert!((app.waveforms[&id].position[0] - expected).abs() < 0.01);

    // Commit the coalesced nudge
    app.commit_arrow_nudge();

    // Should be exactly one new undo entry
    assert_eq!(app.op_undo_stack.len(), undo_len_before + 1);

    // Undo should restore original position and preserve selection
    app.undo_op();
    assert!((app.waveforms[&id].position[0] - start_x).abs() < 0.01);
    assert_eq!(app.selected.len(), 1, "selection should be preserved after undo");
    assert_eq!(app.selected[0], HitTarget::Waveform(id));
}

#[test]
fn test_multi_nudge_preserves_relative_spacing() {
    let mut app = App::new_headless();
    let id1 = new_id();
    let id2 = new_id();
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    // Place first clip on grid, second off-grid (half step offset)
    let x1 = step * 2.0;
    let x2 = step * 2.0 + step * 0.5; // off-grid by half a step
    app.waveforms.insert(id1, make_waveform(x1, 200.0));
    app.waveforms.insert(id2, make_waveform(x2, 200.0));
    app.selected.push(HitTarget::Waveform(id1));
    app.selected.push(HitTarget::Waveform(id2));

    let gap_before = app.waveforms[&id2].position[0] - app.waveforms[&id1].position[0];
    app.nudge_selection(step, 0.0);
    let gap_after = app.waveforms[&id2].position[0] - app.waveforms[&id1].position[0];

    assert!((gap_before - gap_after).abs() < 0.01,
        "relative spacing must be preserved: before={gap_before}, after={gap_after}");
}

// --- Shift+Left/Right resize tests ---

fn make_waveform_with_audio(x: f32, y: f32, size_w: f32, sample_rate: u32, num_samples: usize) -> WaveformView {
    let samples = Arc::new(vec![0.0f32; num_samples]);
    WaveformView {
        audio: Arc::new(AudioData {
            left_samples: samples.clone(),
            right_samples: samples,
            left_peaks: Arc::new(WaveformPeaks::empty()),
            right_peaks: Arc::new(WaveformPeaks::empty()),
            sample_rate,
            filename: "test.wav".to_string(),
        }),
        filename: "test.wav".to_string(),
        position: [x, y],
        size: [size_w, 80.0],
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
        paulstretch_factor: 8.0,
        is_reversed: false,
        disabled: false,
        sample_offset_px: 0.0,
        automation: AutomationData::new(),
        effect_chain_id: None,
        take_group: None,
    }
}

#[test]
fn test_shift_right_extends_waveform() {
    let mut app = App::new_headless();
    let id = new_id();
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    // Enough audio for extension: full_w = num_samples / (sample_rate / PPS) = 192000/400 = 480px
    let wf = make_waveform_with_audio(100.0, 200.0, 200.0, 48000, 192000);
    app.waveforms.insert(id, wf);
    app.selected.push(HitTarget::Waveform(id));

    app.resize_selected_waveforms(step);

    let new_size = app.waveforms[&id].size[0];
    assert!((new_size - (200.0 + step)).abs() < 0.01,
        "size should grow by one grid step: got {new_size}, expected {}", 200.0 + step);
}

#[test]
fn test_shift_left_shrinks_waveform() {
    let mut app = App::new_headless();
    let id = new_id();
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    let wf = make_waveform_with_audio(100.0, 200.0, 200.0, 48000, 192000);
    app.waveforms.insert(id, wf);
    app.selected.push(HitTarget::Waveform(id));

    app.resize_selected_waveforms(-step);

    let new_size = app.waveforms[&id].size[0];
    assert!((new_size - (200.0 - step)).abs() < 0.01,
        "size should shrink by one grid step: got {new_size}, expected {}", 200.0 - step);
}

#[test]
fn test_shift_resize_clamped_at_audio_end() {
    let mut app = App::new_headless();
    let id = new_id();
    // full_audio_width_px = 48000 * 2 / (48000 / 120.0) = 240px
    let num_samples = (240.0 * 48000.0 / PIXELS_PER_SECOND) as usize;
    // Clip already fills 200px starting at offset 0; max extension = 40px
    let wf = make_waveform_with_audio(100.0, 200.0, 200.0, 48000, num_samples);
    app.waveforms.insert(id, wf);
    app.selected.push(HitTarget::Waveform(id));

    // Try to extend by 100px, but only 40px is available
    app.resize_selected_waveforms(100.0);

    let new_size = app.waveforms[&id].size[0];
    assert!(new_size <= 240.0 + 0.01,
        "size must not exceed full audio width: got {new_size}");
}

#[test]
fn test_shift_resize_clamped_at_min_width() {
    let mut app = App::new_headless();
    let id = new_id();
    let wf = make_waveform_with_audio(100.0, 200.0, 20.0, 48000, 192000);
    app.waveforms.insert(id, wf);
    app.selected.push(HitTarget::Waveform(id));

    // Shrink by a large amount — should clamp to WAVEFORM_MIN_WIDTH_PX
    app.resize_selected_waveforms(-1000.0);

    let new_size = app.waveforms[&id].size[0];
    assert!((new_size - WAVEFORM_MIN_WIDTH_PX).abs() < 0.01,
        "size must not go below minimum: got {new_size}");
}

#[test]
fn test_shift_resize_coalescing() {
    let mut app = App::new_headless();
    let id = new_id();
    let step = grid::grid_spacing_for_settings(&app.settings, app.camera.zoom, app.bpm);
    let wf = make_waveform_with_audio(100.0, 200.0, 200.0, 48000, 192000);
    app.waveforms.insert(id, wf);
    app.selected.push(HitTarget::Waveform(id));

    let undo_len_before = app.op_undo_stack.len();

    // Two rapid resizes (within 500ms — immediate calls coalesce)
    app.resize_selected_waveforms(step);
    app.resize_selected_waveforms(step);

    // Size should reflect both resizes
    let new_size = app.waveforms[&id].size[0];
    assert!((new_size - (200.0 + step * 2.0)).abs() < 0.01);

    // Commit the coalesced resize
    app.commit_arrow_resize();

    // Should be exactly one new undo entry
    assert_eq!(app.op_undo_stack.len(), undo_len_before + 1,
        "two rapid resizes should coalesce into one undo entry");
}
