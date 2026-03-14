use super::*;

impl App {
    pub(crate) fn open_add_folder_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.sample_browser.add_folder(folder);
            self.sample_browser.visible = true;
            self.request_redraw();
        }
    }

    pub(crate) fn plugin_section_y_offset(&self, _screen_h: f32, scale: f32) -> f32 {
        let header_h = ui::browser::HEADER_HEIGHT * scale;
        let item_h = ui::browser::ITEM_HEIGHT * scale;
        let total_items = self.sample_browser.entries.len() as f32;
        header_h + total_items * item_h - self.sample_browser.scroll_offset
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
        self.plugin_browser.set_plugins(entries);

        // Reload any saved plugin blocks that were waiting for the scanner
        for pb in &mut self.plugin_blocks {
            let has_instance = pb.instance.lock().ok().map_or(false, |g| g.is_some());
            if !has_instance {
                if let Some(instance) =
                    self.plugin_registry
                        .load_plugin(&pb.plugin_id, 48000.0, 512)
                {
                    {
                        let mut g = pb.instance.lock().unwrap();
                        *g = Some(instance);
                        // Try to restore rack instance state (keep pending_state for GUI)
                        if let Some(state) = &pb.pending_state {
                            if let Some(inst) = g.as_mut() {
                                match inst.set_state(state) {
                                    Ok(()) => println!("  Restored plugin state ({} bytes)", state.len()),
                                    Err(e) => println!("  Failed to restore rack state: {} (will restore via GUI)", e),
                                }
                            }
                        }
                    }
                    // Also update plugin_path from registry
                    if let Some(entry) = self.plugin_registry.plugins.iter().find(|e| e.info.unique_id == pb.plugin_id) {
                        pb.plugin_path = entry.info.path.clone();
                    }
                    println!("  Reloaded plugin '{}'", pb.plugin_name);
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

        let mut pb = effects::PluginBlock::new(
            position,
            plugin_id.to_string(),
            plugin_name.to_string(),
            plugin_path,
        );

        if let Some(instance) = self
            .plugin_registry
            .load_plugin(plugin_id, sample_rate, block_size)
        {
            pb.instance = Arc::new(std::sync::Mutex::new(Some(instance)));
        }

        self.plugin_blocks.push(pb);
        let idx = self.plugin_blocks.len() - 1;
        println!("  Added plugin block '{}'", plugin_name);
        self.sync_audio_clips();

        // Try to open native VST3 GUI
        let pb = &self.plugin_blocks[idx];
        let path = pb.plugin_path.to_string_lossy().to_string();
        if !path.is_empty() {
            let uid = pb.plugin_id.clone();
            let name = pb.plugin_name.clone();
            if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &uid, &name) {
                println!("  Opened native GUI for '{}'", name);
                if let Ok(mut g) = pb.gui.lock() {
                    *g = Some(gui);
                }
            }
        }
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
            let has_instance = pb.instance.lock().ok().map_or(false, |g| g.is_some());
            if !has_instance {
                if let Some(instance) = self.plugin_registry.load_plugin(&pb.plugin_id, 48000.0, 512) {
                    let mut g = pb.instance.lock().unwrap();
                    *g = Some(instance);
                    // Try to restore rack instance state (may fail for some plugins, that's ok —
                    // the GUI will get state directly from pending_state)
                    if let Some(state) = &pb.pending_state {
                        if let Some(inst) = g.as_mut() {
                            let _ = inst.set_state(state);
                        }
                    }
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
                    // GUI exists but hidden — just show it again
                    if let Ok(g) = pb.gui.lock() {
                        if let Some(gui) = g.as_ref() {
                            gui.show();
                            println!("  Re-showed native GUI for '{}'", name);
                        }
                    }
                }
                // Already visible or just shown — done
                return;
            }

            // No GUI yet — create one
            if let Some(gui) = vst3_gui::Vst3Gui::open(&path, &uid, &name) {
                // Restore saved state blob first (handles preset name, etc.)
                let state_to_restore = saved_state
                    .or_else(|| pb.instance.lock().ok()
                        .and_then(|g| g.as_ref().and_then(|inst| inst.get_state().ok())));
                if let Some(state) = state_to_restore {
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

        // Fallback: open parameter editor
        let mut params = Vec::new();
        if let Ok(guard) = pb.instance.lock() {
            if let Some(inst) = guard.as_ref() {
                let count = inst.parameter_count();
                for param_idx in 0..count {
                    let info = inst.parameter_info(param_idx);
                    let val = inst.get_parameter(param_idx).unwrap_or(0.0);
                    let (pname, unit, default) = match info {
                        Ok(pi) => (pi.name, pi.unit, pi.default),
                        Err(_) => (format!("Param {}", param_idx), String::new(), 0.0),
                    };
                    params.push(ui::plugin_editor::ParamEntry {
                        name: pname,
                        unit,
                        value: val,
                        default,
                    });
                }
            }
        }
        self.plugin_editor = Some(ui::plugin_editor::PluginEditorWindow::new(
            idx, 0, name, params,
        ));
    }
}
