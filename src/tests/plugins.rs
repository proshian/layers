use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::effects::{self, EffectRegion, PluginBlock};
use crate::{App, HitTarget};

/// Create a dummy plugin block (no real VST3, gui=None) for structural tests.
fn make_plugin_block(x: f32, y: f32, id: &str, name: &str) -> PluginBlock {
    PluginBlock::new([x, y], id.to_string(), name.to_string(), PathBuf::new())
}

/// Known-good plugin names that open reliably (FabFilter, etc).
const PREFERRED_PLUGINS: &[&str] = &[
    "Pro-Q 4",
    "Pro-Q 3",
    "Pro-C 2",
    "Pro-L 2",
    "Pro-R",
];

/// Returns (plugin_id, plugin_name, plugin_path) of a known-good effect plugin.
/// Prefers FabFilter plugins which are known to work well.
/// Returns None if no VST3 effects are installed.
fn first_available_effect(app: &mut App) -> Option<(String, String, PathBuf)> {
    app.ensure_plugins_scanned();

    // Try preferred plugins first
    for preferred in PREFERRED_PLUGINS {
        if let Some(entry) = app.plugin_registry.plugins.iter().find(|e| e.info.name == *preferred) {
            return Some((
                entry.info.unique_id.clone(),
                entry.info.name.clone(),
                entry.info.path.clone(),
            ));
        }
    }

    // Fallback to first available
    app.plugin_registry.plugins.first().map(|e| {
        (
            e.info.unique_id.clone(),
            e.info.name.clone(),
            e.info.path.clone(),
        )
    })
}

/// Open a plugin headlessly (no GUI window, no main thread required).
fn open_headless(path: &PathBuf, id: &str) -> Option<vst3_gui::Vst3Gui> {
    let path_str = path.to_string_lossy().to_string();
    vst3_gui::Vst3Gui::open_headless(&path_str, id)
}

/// Helper: check if the plugin block at index has a live GUI.
fn has_gui(app: &App, idx: usize) -> bool {
    app.plugin_blocks[idx]
        .gui
        .lock()
        .ok()
        .map_or(false, |g| g.is_some())
}

// =========================================================================
// Integration tests using real VST3 plugins (e.g. FabFilter Pro-Q 4)
// Uses headless mode — no GUI window, works from any thread.
// Skip on CI / machines without VST3 effects installed.
// =========================================================================

#[test]
fn test_plugin_block_create_with_real_vst3() {
    let mut app = App::new_headless();
    let Some((id, name, path)) = first_available_effect(&mut app) else {
        println!("SKIP: no VST3 effect plugins found");
        return;
    };

    // Open headless and attach to a plugin block
    let gui = open_headless(&path, &id).expect(&format!("should open '{}' headlessly", name));
    let mut pb = PluginBlock::new([100.0, 200.0], id.clone(), name.clone(), path);
    *pb.gui.lock().unwrap() = Some(gui);
    app.plugin_blocks.push(pb);

    assert_eq!(app.plugin_blocks.len(), 1);
    assert_eq!(app.plugin_blocks[0].position, [100.0, 200.0]);
    assert_eq!(app.plugin_blocks[0].plugin_name, name);
    assert!(has_gui(&app, 0), "gui should be Some for '{}'", name);
}

#[test]
fn test_plugin_block_gui_has_parameters() {
    let mut app = App::new_headless();
    let Some((id, name, path)) = first_available_effect(&mut app) else {
        println!("SKIP: no VST3 effect plugins found");
        return;
    };

    let gui = open_headless(&path, &id).expect(&format!("should open '{}' headlessly", name));
    let count = gui.parameter_count();
    assert!(count > 0, "'{}' should have at least 1 parameter, got 0", name);
    let val = gui.get_parameter(0);
    assert!(val.is_some(), "get_parameter(0) should return Some for '{}'", name);
}

#[test]
fn test_plugin_block_state_save_restore() {
    let mut app = App::new_headless();
    let Some((id, name, path)) = first_available_effect(&mut app) else {
        println!("SKIP: no VST3 effect plugins found");
        return;
    };

    // Open first instance and tweak a parameter
    let gui1 = open_headless(&path, &id).expect(&format!("should open '{}' headlessly", name));
    gui1.setup_processing(48000.0, 512);
    gui1.set_parameter(0, 0.75);

    // Save state + params
    let state = gui1.get_state().expect("get_state should return Some");
    assert!(!state.is_empty(), "state should not be empty for '{}'", name);
    let params = gui1.get_all_parameters();

    // Open second instance and restore state
    let gui2 = open_headless(&path, &id).expect("should open second instance");
    gui2.setup_processing(48000.0, 512);
    gui2.set_state(&state);
    gui2.set_all_parameters(&params);

    // Verify the parameter was restored
    let restored = gui2.get_parameter(0).unwrap_or(0.0);
    assert!(
        (restored - 0.75).abs() < 0.05,
        "'{}' parameter 0 should be ~0.75 after restore, got {}",
        name, restored
    );
}

#[test]
fn test_plugin_block_pending_state_restored_on_reopen() {
    let mut app = App::new_headless();
    let Some((id, name, path)) = first_available_effect(&mut app) else {
        println!("SKIP: no VST3 effect plugins found");
        return;
    };

    // Open headless, tweak param, get state
    let gui = open_headless(&path, &id).expect(&format!("should open '{}' headlessly", name));
    gui.setup_processing(48000.0, 512);
    gui.set_parameter(0, 0.6);
    let state = gui.get_state();
    let params = gui.get_all_parameters();
    drop(gui);

    // Simulate what ensure_plugins_scanned does: open headless and restore pending state
    let gui2 = open_headless(&path, &id).expect("should reopen headlessly");
    gui2.setup_processing(48000.0, 512);
    if let Some(state) = &state {
        gui2.set_state(state);
    }
    gui2.set_all_parameters(&params);

    let restored = gui2.get_parameter(0).unwrap_or(0.0);
    assert!(
        (restored - params[0]).abs() < 0.05,
        "'{}' parameter 0 should match saved value after reopen, got {}",
        name, restored
    );
}

#[test]
fn test_plugin_block_audio_processing() {
    let mut app = App::new_headless();
    let Some((id, name, path)) = first_available_effect(&mut app) else {
        println!("SKIP: no VST3 effect plugins found");
        return;
    };

    let gui = open_headless(&path, &id).expect(&format!("should open '{}' headlessly", name));
    gui.setup_processing(48000.0, 512);

    let num_frames = 512;
    let input_l: Vec<f32> = vec![0.5; num_frames];
    let input_r: Vec<f32> = vec![0.5; num_frames];
    let mut output_l: Vec<f32> = vec![0.0; num_frames];
    let mut output_r: Vec<f32> = vec![0.0; num_frames];

    let inputs: Vec<&[f32]> = vec![&input_l, &input_r];
    let mut outputs: Vec<&mut [f32]> = vec![&mut output_l, &mut output_r];

    let ok = gui.process(&inputs, &mut outputs, num_frames);
    assert!(ok, "process() should succeed for '{}'", name);

    let has_output = output_l.iter().any(|&s| s != 0.0) || output_r.iter().any(|&s| s != 0.0);
    assert!(
        has_output,
        "'{}' output should not be all zeros after processing",
        name
    );
}

// =========================================================================
// Tests using dummy plugin blocks (no real VST3 needed, always run)
// =========================================================================

#[test]
fn test_plugin_block_bypass_excludes_from_region() {
    let region = EffectRegion::new([0.0, 0.0], [500.0, 300.0]);

    let mut blocks = vec![
        make_plugin_block(50.0, 50.0, "id-a", "PluginA"),
        make_plugin_block(200.0, 50.0, "id-b", "PluginB"),
    ];
    blocks[0].bypass = true;

    let chain = effects::collect_plugins_for_region(&region, &blocks);
    assert_eq!(chain.len(), 1, "only non-bypassed plugin should be in chain");
    assert_eq!(chain[0], 1, "the second plugin block should be in the chain");
}

#[test]
fn test_plugin_block_spatial_chain_ordering() {
    let region = EffectRegion::new([0.0, 0.0], [800.0, 300.0]);

    let blocks = vec![
        make_plugin_block(400.0, 50.0, "id-a", "Right"),   // index 0
        make_plugin_block(100.0, 50.0, "id-b", "Left"),    // index 1
        make_plugin_block(250.0, 50.0, "id-c", "Middle"),  // index 2
    ];

    let chain = effects::collect_plugins_for_region(&region, &blocks);
    assert_eq!(chain.len(), 3);
    // Sorted by X: index 1 (x=100), index 2 (x=250), index 0 (x=400)
    assert_eq!(chain, vec![1, 2, 0]);
}

#[test]
fn test_plugin_block_delete() {
    let mut app = App::new_headless();

    app.plugin_blocks.push(make_plugin_block(0.0, 0.0, "id-a", "PluginA"));
    app.plugin_blocks.push(make_plugin_block(200.0, 0.0, "id-b", "PluginB"));

    app.selected.push(HitTarget::PluginBlock(0));
    app.delete_selected();

    assert_eq!(app.plugin_blocks.len(), 1);
    assert_eq!(app.plugin_blocks[0].plugin_id, "id-b");
}

#[test]
fn test_plugin_block_undo_redo() {
    let mut app = App::new_headless();

    app.plugin_blocks.push(make_plugin_block(0.0, 0.0, "id-a", "PluginA"));
    app.push_undo();

    // Delete it
    app.selected.push(HitTarget::PluginBlock(0));
    app.delete_selected();
    assert_eq!(app.plugin_blocks.len(), 0);

    // Undo should restore it
    app.undo();
    assert_eq!(app.plugin_blocks.len(), 1);
    assert_eq!(app.plugin_blocks[0].plugin_name, "PluginA");
    assert_eq!(app.plugin_blocks[0].plugin_id, "id-a");
}

#[test]
fn test_plugin_block_sync_audio_clips() {
    let mut app = App::new_headless();

    let region = EffectRegion::new([0.0, 0.0], [500.0, 300.0]);
    app.effect_regions.push(region);
    app.plugin_blocks.push(make_plugin_block(50.0, 50.0, "id-a", "PluginA"));

    // sync_audio_clips should not panic even with no audio engine (headless)
    app.sync_audio_clips();
}

#[test]
fn test_plugin_block_outside_region_not_collected() {
    let region = EffectRegion::new([0.0, 0.0], [200.0, 200.0]);

    let blocks = vec![
        make_plugin_block(50.0, 50.0, "id-a", "Inside"),
        make_plugin_block(500.0, 500.0, "id-b", "Outside"),
    ];

    let chain = effects::collect_plugins_for_region(&region, &blocks);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0], 0);
}
