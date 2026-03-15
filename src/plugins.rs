use super::*;

impl App {
    pub(crate) fn open_add_folder_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.sample_browser.add_folder(folder);
            self.sample_browser.visible = true;
            self.save_browser_folders_to_settings();
            self.mark_dirty();
            self.request_redraw();
        }
    }

    pub(crate) fn save_browser_folders_to_settings(&self) {
        let mut settings = crate::settings::Settings::load();
        settings.sample_library_folders = self
            .sample_browser
            .root_folders
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        settings.save();
    }

    pub(crate) fn ensure_plugins_scanned(&mut self) {
        if self.plugin_registry.is_scanned() {
            return;
        }
        self.plugin_registry.ensure_scanned();

        let entries: Vec<ui::browser::PluginEntry> = self
            .plugin_registry
            .plugins
            .iter()
            .map(|e| ui::browser::PluginEntry {
                unique_id: e.info.unique_id.clone(),
                name: e.info.name.clone(),
                manufacturer: e.info.manufacturer.clone(),
            })
            .collect();
        self.sample_browser.set_plugins(entries);

        // Reload any saved plugin blocks that were waiting for the scanner.
        // Open with full GUI but immediately hide — state is restored, user can show() later.
        for pb in &mut self.plugin_blocks {
            let has_gui = pb.gui.lock().ok().map_or(false, |g| g.is_some());
            if !has_gui {
                // Update plugin_path from registry
                if let Some(entry) = self.plugin_registry.plugins.iter().find(|e| e.info.unique_id == pb.plugin_id) {
                    pb.plugin_path = entry.info.path.clone();
                }
                let path = pb.plugin_path.to_string_lossy().to_string();
                if !path.is_empty() {
                    if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &pb.plugin_id, &pb.plugin_name) {
                        gui.hide();
                        gui.setup_processing(48000.0, 512);
                        if let Some(state) = &pb.pending_state {
                            gui.set_state(state);
                            println!("  Restored plugin state ({} bytes)", state.len());
                        }
                        if let Some(params) = &pb.pending_params {
                            gui.set_all_parameters(params);
                            println!("  Restored {} plugin parameters", params.len());
                        }
                        if let Ok(mut g) = pb.gui.lock() {
                            *g = Some(gui);
                        }
                        println!("  Reloaded plugin '{}'", pb.plugin_name);
                    }
                }
            }
        }
        // Reload any saved instrument regions that were waiting for the scanner
        for ir in &mut self.instrument_regions {
            if ir.plugin_id.is_empty() {
                continue;
            }
            let has_gui = ir.gui.lock().ok().map_or(false, |g| g.is_some());
            if !has_gui {
                if let Some(entry) = self.plugin_registry.instruments.iter().find(|e| e.info.unique_id == ir.plugin_id) {
                    ir.plugin_path = entry.info.path.clone();
                }
                let path = ir.plugin_path.to_string_lossy().to_string();
                if !path.is_empty() {
                    if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &ir.plugin_id, &ir.plugin_name) {
                        gui.hide();
                        gui.setup_processing(48000.0, 512);
                        if let Some(state) = &ir.pending_state {
                            gui.set_state(state);
                        }
                        if let Ok(mut g) = ir.gui.lock() {
                            *g = Some(gui);
                        }
                        println!("  Reloaded instrument '{}'", ir.plugin_name);
                    }
                }
            }
        }
        self.sync_audio_clips();
    }

    pub(crate) fn add_plugin_block(&mut self, position: [f32; 2], plugin_id: &str, plugin_name: &str) {
        self.ensure_plugins_scanned();
        let sample_rate = 48000.0;
        let block_size = 512;

        // Look up plugin path from registry
        let plugin_path = self
            .plugin_registry
            .plugins
            .iter()
            .find(|e| e.info.unique_id == plugin_id)
            .map(|e| e.info.path.clone())
            .unwrap_or_default();

        let pb = effects::PluginBlock::new(
            position,
            plugin_id.to_string(),
            plugin_name.to_string(),
            plugin_path,
        );

        // Open vst3-gui instance (single instance for both GUI and audio)
        let path = pb.plugin_path.to_string_lossy().to_string();
        if !path.is_empty() {
            if let Some(gui) = vst3_gui::Vst3Gui::open(&path, plugin_id, plugin_name) {
                gui.setup_processing(sample_rate, block_size as i32);
                println!("  Opened native GUI for '{}'", plugin_name);
                if let Ok(mut g) = pb.gui.lock() {
                    *g = Some(gui);
                }
            }
        }

        self.plugin_blocks.push(pb);
        println!("  Added plugin block '{}'", plugin_name);
        self.sync_audio_clips();
    }

    pub(crate) fn add_plugin_to_selected_effect_region(&mut self, plugin_id: &str, plugin_name: &str) {
        // Find the selected effect region
        let region_idx = self.selected.iter().find_map(|t| {
            if let HitTarget::EffectRegion(i) = t {
                Some(*i)
            } else {
                None
            }
        });
        let Some(region_idx) = region_idx else {
            println!("  No effect region selected, cannot add plugin");
            return;
        };
        if region_idx >= self.effect_regions.len() {
            return;
        }

        // Find the rightmost plugin block already in this region to place after it
        let region = &self.effect_regions[region_idx];
        let existing = effects::collect_plugins_for_region(region, &self.plugin_blocks);
        let position = if let Some(&last_idx) = existing.last() {
            let last = &self.plugin_blocks[last_idx];
            [
                last.position[0] + last.size[0] + 10.0,
                last.position[1],
            ]
        } else {
            // Place at the top-left of the region with some padding
            [
                region.position[0] + 10.0,
                region.position[1] + 30.0,
            ]
        };

        self.add_plugin_block(position, plugin_id, plugin_name);
    }

    pub(crate) fn open_plugin_block_gui(&mut self, idx: usize) {
        if idx >= self.plugin_blocks.len() {
            return;
        }

        // Ensure plugin is loaded and path is resolved before opening GUI
        self.ensure_plugins_scanned();
        {
            let pb = &mut self.plugin_blocks[idx];
            if pb.plugin_path.as_os_str().is_empty() {
                if let Some(entry) = self.plugin_registry.plugins.iter().find(|e| e.info.unique_id == pb.plugin_id) {
                    pb.plugin_path = entry.info.path.clone();
                }
            }
        }

        let saved_state = self.plugin_blocks[idx].pending_state.take();
        let saved_params = self.plugin_blocks[idx].pending_params.take();
        let pb = &self.plugin_blocks[idx];
        let path = pb.plugin_path.to_string_lossy().to_string();
        let uid = pb.plugin_id.clone();
        let name = pb.plugin_name.clone();

        if !path.is_empty() {
            // Check if we already have a GUI handle (open or hidden)
            let has_gui = pb.gui.lock().ok().map_or(false, |g| g.is_some());
            if has_gui {
                let is_visible = pb.gui.lock()
                    .ok()
                    .map_or(false, |g| g.as_ref().map_or(false, |gui| gui.is_open()));
                if !is_visible {
                    // GUI exists but hidden — just show it
                    if let Ok(g) = pb.gui.lock() {
                        if let Some(gui) = g.as_ref() {
                            gui.show();
                            println!("  Showed native GUI for '{}'", name);
                        }
                    }
                }
                return;
            }

            // No GUI yet — create one
            if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &uid, &name) {
                gui.setup_processing(48000.0, 512);
                // Restore saved state blob first (handles preset name, etc.)
                if let Some(state) = saved_state {
                    if !state.is_empty() {
                        gui.set_state(&state);
                    }
                }
                // Then restore individual parameter values (more reliable for some plugins)
                if let Some(params) = saved_params {
                    gui.set_all_parameters(&params);
                    println!("  Restored {} GUI parameters", params.len());
                }
                println!("  Opened native GUI for '{}'", name);
                if let Ok(mut g) = pb.gui.lock() {
                    *g = Some(gui);
                }
                return;
            }
        }

        // Fallback: open parameter editor using gui instance
        let mut params = Vec::new();
        if let Ok(guard) = pb.gui.lock() {
            if let Some(gui) = guard.as_ref() {
                let count = gui.parameter_count();
                for param_idx in 0..count {
                    let val = gui.get_parameter(param_idx).unwrap_or(0.0);
                    params.push(ui::plugin_editor::ParamEntry {
                        name: format!("Param {}", param_idx),
                        unit: String::new(),
                        value: val as f32,
                        default: 0.0,
                    });
                }
            }
        }
        self.plugin_editor = Some(ui::plugin_editor::PluginEditorWindow::new(
            idx, 0, name, params,
        ));
    }

    /// Create a new instrument region with the given plugin already assigned,
    /// plus an empty MIDI clip inside it ready for editing.
    pub(crate) fn add_instrument(&mut self, plugin_id: &str, plugin_name: &str) {
        self.push_undo();
        self.ensure_plugins_scanned();

        let padding = instruments::INSTRUMENT_REGION_PADDING;

        // Compute MIDI clip dimensions (same logic as add_midi_clip)
        let ppb = grid::pixels_per_beat(self.bpm);
        let beats_per_bar = 4.0;
        let clip_w = ppb * beats_per_bar * midi::MIDI_CLIP_DEFAULT_BARS as f32;
        let clip_h = midi::MIDI_CLIP_DEFAULT_HEIGHT;

        // Region = clip + padding on each side
        let region_w = clip_w + padding * 2.0;
        let region_h = clip_h + padding * 2.0;

        let (sw, sh, _) = self.screen_info();
        let center = self.camera.screen_to_world([sw * 0.5, sh * 0.5]);
        let region_pos = [center[0] - region_w * 0.5, center[1] - region_h * 0.5];
        let clip_pos = [region_pos[0] + padding, region_pos[1] + padding];

        // Create instrument region
        let mut ir = instruments::InstrumentRegion::new(region_pos, [region_w, region_h]);
        ir.plugin_id = plugin_id.to_string();
        ir.plugin_name = plugin_name.to_string();

        // Look up plugin path from registry
        let plugin_path = self
            .plugin_registry
            .instruments
            .iter()
            .find(|e| e.info.unique_id == plugin_id)
            .map(|e| e.info.path.clone())
            .unwrap_or_default();
        ir.plugin_path = plugin_path.clone();

        // Open vst3-gui instance
        let path_str = plugin_path.to_string_lossy().to_string();
        if !path_str.is_empty() {
            if let Some(gui) = vst3_gui::Vst3Gui::open(&path_str, plugin_id, plugin_name) {
                if gui.setup_processing(48000.0, 512) {
                    println!("  Set up audio processing for instrument '{}'", plugin_name);
                } else {
                    println!("  Warning: audio processing setup failed for '{}'", plugin_name);
                }
                println!("  Opened native GUI for instrument '{}'", plugin_name);
                if let Ok(mut g) = ir.gui.lock() {
                    *g = Some(gui);
                }
            }
        }

        self.instrument_regions.push(ir);
        let region_idx = self.instrument_regions.len() - 1;

        // Create MIDI clip inside the region
        let mut clip = midi::MidiClip::new(clip_pos, &self.settings);
        clip.size = [clip_w, clip_h];
        self.midi_clips.push(clip);
        let clip_idx = self.midi_clips.len() - 1;

        // Select the region and enter MIDI edit mode on the clip
        self.selected.clear();
        self.selected.push(HitTarget::InstrumentRegion(region_idx));
        self.editing_midi_clip = Some(clip_idx);
        self.selected_midi_notes.clear();

        self.mark_dirty();
        self.request_redraw();
        println!("  Added instrument '{}'", plugin_name);
    }

    pub(crate) fn open_instrument_region_gui(&mut self, idx: usize) {
        if idx >= self.instrument_regions.len() {
            return;
        }
        let ir = &self.instrument_regions[idx];
        if ir.plugin_id.is_empty() {
            return;
        }

        let path = ir.plugin_path.to_string_lossy().to_string();
        let uid = ir.plugin_id.clone();
        let name = ir.plugin_name.clone();

        if !path.is_empty() {
            let has_gui = ir.gui.lock().ok().map_or(false, |g| g.is_some());
            if has_gui {
                let is_visible = ir.gui.lock()
                    .ok()
                    .map_or(false, |g| g.as_ref().map_or(false, |gui| gui.is_open()));
                if !is_visible {
                    if let Ok(g) = ir.gui.lock() {
                        if let Some(gui) = g.as_ref() {
                            gui.show();
                        }
                    }
                }
                return;
            }

            if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &uid, &name) {
                println!("  Opened native GUI for instrument '{}'", name);
                if let Ok(mut g) = ir.gui.lock() {
                    *g = Some(gui);
                }
            }
        }
    }
}
