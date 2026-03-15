use crate::App;
use crate::midi;
use crate::instruments;
use crate::HitTarget;

// ---------------------------------------------------------------------------
// MIDI Clip CRUD
// ---------------------------------------------------------------------------

#[test]
fn test_add_midi_clip() {
    let mut app = App::new_headless();
    assert!(app.midi_clips.is_empty());
    app.add_midi_clip();
    assert_eq!(app.midi_clips.len(), 1);
    assert_eq!(app.selected, vec![HitTarget::MidiClip(0)]);
    // Check defaults
    let mc = &app.midi_clips[0];
    assert_eq!(mc.size, midi::MIDI_CLIP_DEFAULT_SIZE);
    assert_eq!(mc.pitch_range, midi::MIDI_CLIP_DEFAULT_PITCH_RANGE);
    assert!(mc.notes.is_empty());
}

#[test]
fn test_delete_midi_clip() {
    let mut app = App::new_headless();
    app.add_midi_clip();
    app.add_midi_clip();
    assert_eq!(app.midi_clips.len(), 2);
    app.selected = vec![HitTarget::MidiClip(0)];
    app.delete_selected();
    assert_eq!(app.midi_clips.len(), 1);
}

#[test]
fn test_move_midi_clip() {
    let mut app = App::new_headless();
    app.add_midi_clip();
    let target = HitTarget::MidiClip(0);
    app.set_target_pos(&target, [100.0, 200.0]);
    assert_eq!(app.midi_clips[0].position, [100.0, 200.0]);
    assert_eq!(app.get_target_pos(&target), [100.0, 200.0]);
}

#[test]
fn test_add_remove_midi_notes() {
    let mut app = App::new_headless();
    app.add_midi_clip();
    let mc = &mut app.midi_clips[0];
    mc.notes.push(midi::MidiNote {
        pitch: 60,
        start_px: 0.0,
        duration_px: 30.0,
        velocity: 100,
    });
    mc.notes.push(midi::MidiNote {
        pitch: 64,
        start_px: 30.0,
        duration_px: 30.0,
        velocity: 80,
    });
    assert_eq!(app.midi_clips[0].notes.len(), 2);
    assert_eq!(app.midi_clips[0].notes[0].pitch, 60);
    assert_eq!(app.midi_clips[0].notes[1].pitch, 64);

    // Remove first note
    app.midi_clips[0].notes.remove(0);
    assert_eq!(app.midi_clips[0].notes.len(), 1);
    assert_eq!(app.midi_clips[0].notes[0].pitch, 64);
}

#[test]
fn test_midi_clip_pitch_to_y_and_back() {
    let mc = midi::MidiClip::new([0.0, 0.0]);
    // Round-trip: pitch -> y -> pitch
    for pitch in mc.pitch_range.0..mc.pitch_range.1 {
        let y = mc.pitch_to_y(pitch);
        let back = mc.y_to_pitch(y + mc.note_height() * 0.5); // center of note
        assert_eq!(back, pitch, "Round-trip failed for pitch {}", pitch);
    }
}

// ---------------------------------------------------------------------------
// Instrument Region
// ---------------------------------------------------------------------------

#[test]
fn test_add_instrument_region() {
    let mut app = App::new_headless();
    assert!(app.instrument_regions.is_empty());
    app.add_instrument_area();
    assert_eq!(app.instrument_regions.len(), 1);
    assert_eq!(app.selected, vec![HitTarget::InstrumentRegion(0)]);
    let ir = &app.instrument_regions[0];
    assert_eq!(ir.size, [instruments::INSTRUMENT_REGION_DEFAULT_WIDTH, instruments::INSTRUMENT_REGION_DEFAULT_HEIGHT]);
    assert!(!ir.has_plugin());
}

#[test]
fn test_delete_instrument_region() {
    let mut app = App::new_headless();
    app.add_instrument_area();
    app.add_instrument_area();
    assert_eq!(app.instrument_regions.len(), 2);
    app.selected = vec![HitTarget::InstrumentRegion(1)];
    app.delete_selected();
    assert_eq!(app.instrument_regions.len(), 1);
}

#[test]
fn test_move_instrument_region() {
    let mut app = App::new_headless();
    app.add_instrument_area();
    let target = HitTarget::InstrumentRegion(0);
    app.set_target_pos(&target, [50.0, 75.0]);
    assert_eq!(app.instrument_regions[0].position, [50.0, 75.0]);
}

// ---------------------------------------------------------------------------
// MIDI Audio Sync
// ---------------------------------------------------------------------------

#[test]
fn test_sync_instrument_regions_produces_events() {
    let mut app = App::new_headless();

    // Add instrument region
    app.add_instrument_area();
    app.instrument_regions[0].position = [0.0, 0.0];
    app.instrument_regions[0].size = [1000.0, 500.0];
    app.instrument_regions[0].plugin_id = "test-synth".to_string();
    app.instrument_regions[0].plugin_name = "Test Synth".to_string();

    // Add MIDI clip inside the region
    app.midi_clips.push(midi::MidiClip {
        position: [100.0, 100.0],
        size: [200.0, 150.0],
        color: midi::MIDI_CLIP_DEFAULT_COLOR,
        notes: vec![
            midi::MidiNote { pitch: 60, start_px: 0.0, duration_px: 60.0, velocity: 100 },
            midi::MidiNote { pitch: 64, start_px: 60.0, duration_px: 30.0, velocity: 80 },
        ],
        pitch_range: (48, 84),
    });

    // No audio engine in headless, but sync_instrument_regions shouldn't panic
    app.sync_instrument_regions();
    // Just verify it doesn't crash - actual audio processing requires AudioEngine
}

#[test]
fn test_undo_redo_midi_clip() {
    let mut app = App::new_headless();
    app.add_midi_clip();
    assert_eq!(app.midi_clips.len(), 1);

    // push_undo was called by add_midi_clip, so undo should remove it
    app.undo();
    assert_eq!(app.midi_clips.len(), 0);

    app.redo();
    assert_eq!(app.midi_clips.len(), 1);
}

#[test]
fn test_undo_redo_instrument_region() {
    let mut app = App::new_headless();
    app.add_instrument_area();
    assert_eq!(app.instrument_regions.len(), 1);

    app.undo();
    assert_eq!(app.instrument_regions.len(), 0);

    app.redo();
    assert_eq!(app.instrument_regions.len(), 1);
}

// ---------------------------------------------------------------------------
// Alt+drag duplication of MIDI notes
// ---------------------------------------------------------------------------

#[test]
fn test_alt_duplicate_midi_notes() {
    let mut app = App::new_headless();
    app.add_midi_clip();
    let mc_idx = 0;

    // Add two notes to the clip
    app.midi_clips[mc_idx].notes.push(midi::MidiNote {
        pitch: 60,
        start_px: 10.0,
        duration_px: 30.0,
        velocity: 100,
    });
    app.midi_clips[mc_idx].notes.push(midi::MidiNote {
        pitch: 64,
        start_px: 50.0,
        duration_px: 20.0,
        velocity: 80,
    });
    assert_eq!(app.midi_clips[mc_idx].notes.len(), 2);

    // Simulate what alt+drag does: clone selected notes and push them
    app.push_undo();
    let selected = vec![0usize, 1usize];
    let mut new_indices: Vec<usize> = Vec::new();
    for &ni in &selected {
        let cloned = app.midi_clips[mc_idx].notes[ni].clone();
        app.midi_clips[mc_idx].notes.push(cloned);
        new_indices.push(app.midi_clips[mc_idx].notes.len() - 1);
    }
    app.selected_midi_notes = new_indices.clone();

    // Should now have 4 notes (original 2 + 2 duplicates)
    assert_eq!(app.midi_clips[mc_idx].notes.len(), 4);

    // Duplicates have the same pitch and duration as originals
    assert_eq!(app.midi_clips[mc_idx].notes[2].pitch, 60);
    assert_eq!(app.midi_clips[mc_idx].notes[2].duration_px, 30.0);
    assert_eq!(app.midi_clips[mc_idx].notes[3].pitch, 64);
    assert_eq!(app.midi_clips[mc_idx].notes[3].duration_px, 20.0);

    // selected_midi_notes points to the new duplicates
    assert_eq!(app.selected_midi_notes, vec![2, 3]);

    // Undo should revert to 2 notes
    app.undo();
    assert_eq!(app.midi_clips[mc_idx].notes.len(), 2);
}
