use crate::App;

#[test]
fn tooltip_visibility_lifecycle() {
    let mut app = App::new_headless();

    // Initially no tooltip visible
    assert!(!app.tooltip.is_pending());

    // Set a target — should be pending but not visible
    app.tooltip.set_target("transport:metronome", "Metronome", ([100.0, 100.0], [20.0, 20.0]));
    assert!(app.tooltip.is_pending());
    app.tooltip.tick();
    // Still not visible (delay hasn't elapsed)
    assert!(app.tooltip.is_pending());

    // Force elapsed and tick — should become visible
    app.tooltip.force_elapsed();
    app.tooltip.tick();
    assert!(!app.tooltip.is_pending());

    // Clear — no longer visible or pending
    app.tooltip.clear();
    assert!(!app.tooltip.is_pending());
}

#[test]
fn tooltip_target_change_resets_timer() {
    let mut app = App::new_headless();

    // Set target A, make it visible
    app.tooltip.set_target("transport:metronome", "Metronome", ([100.0, 100.0], [20.0, 20.0]));
    app.tooltip.force_elapsed();
    app.tooltip.tick();
    assert!(!app.tooltip.is_pending()); // visible

    // Change to target B — timer resets, not visible
    app.tooltip.set_target("transport:play_pause", "Play", ([130.0, 100.0], [24.0, 24.0]));
    assert!(app.tooltip.is_pending()); // pending again
}
