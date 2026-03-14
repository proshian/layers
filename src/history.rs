use std::sync::Arc;

use crate::audio::AudioClipData;
use crate::component;
use crate::effects;
use crate::regions::{ExportRegion, LoopRegion};
use crate::{App, CanvasObject, WaveformView};

pub(crate) const MAX_UNDO_HISTORY: usize = 50;

#[derive(Clone)]
pub(crate) struct EffectRegionSnapshot {
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) name: String,
}

#[derive(Clone)]
pub(crate) struct LoopRegionSnapshot {
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) enabled: bool,
}

#[derive(Clone)]
pub(crate) struct ExportRegionSnapshot {
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) objects: Vec<CanvasObject>,
    pub(crate) waveforms: Vec<WaveformView>,
    pub(crate) audio_clips: Vec<AudioClipData>,
    pub(crate) effect_regions: Vec<EffectRegionSnapshot>,
    pub(crate) plugin_blocks: Vec<effects::PluginBlockSnapshot>,
    pub(crate) loop_regions: Vec<LoopRegionSnapshot>,
    pub(crate) export_regions: Vec<ExportRegionSnapshot>,
    pub(crate) components: Vec<component::ComponentDef>,
    pub(crate) component_instances: Vec<component::ComponentInstance>,
}

impl App {
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.objects.clone(),
            waveforms: self.waveforms.clone(),
            audio_clips: self.audio_clips.clone(),
            effect_regions: self
                .effect_regions
                .iter()
                .map(|er| EffectRegionSnapshot {
                    position: er.position,
                    size: er.size,
                    name: er.name.clone(),
                })
                .collect(),
            plugin_blocks: self
                .plugin_blocks
                .iter()
                .map(|pb| effects::PluginBlockSnapshot {
                    position: pb.position,
                    size: pb.size,
                    color: pb.color,
                    plugin_id: pb.plugin_id.clone(),
                    plugin_name: pb.plugin_name.clone(),
                    plugin_path: pb.plugin_path.clone(),
                    bypass: pb.bypass,
                })
                .collect(),
            loop_regions: self
                .loop_regions
                .iter()
                .map(|lr| LoopRegionSnapshot {
                    position: lr.position,
                    size: lr.size,
                    enabled: lr.enabled,
                })
                .collect(),
            export_regions: self
                .export_regions
                .iter()
                .map(|xr| ExportRegionSnapshot {
                    position: xr.position,
                    size: xr.size,
                })
                .collect(),
            components: self.components.clone(),
            component_instances: self.component_instances.clone(),
        }
    }

    pub(crate) fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.mark_dirty();
    }

    pub(crate) fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.objects = prev.objects;
            self.waveforms = prev.waveforms;
            self.audio_clips = prev.audio_clips;
            self.restore_effect_regions(prev.effect_regions);
            self.restore_plugin_blocks(prev.plugin_blocks);
            self.restore_loop_regions(prev.loop_regions);
            self.restore_export_regions(prev.export_regions);
            self.components = prev.components;
            self.component_instances = prev.component_instances;
            self.selected.clear();
            self.mark_dirty();
            self.sync_audio_clips();
            self.sync_loop_region();
            self.request_redraw();
        }
    }

    pub(crate) fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.objects = next.objects;
            self.waveforms = next.waveforms;
            self.audio_clips = next.audio_clips;
            self.restore_effect_regions(next.effect_regions);
            self.restore_plugin_blocks(next.plugin_blocks);
            self.restore_loop_regions(next.loop_regions);
            self.restore_export_regions(next.export_regions);
            self.components = next.components;
            self.component_instances = next.component_instances;
            self.selected.clear();
            self.mark_dirty();
            self.sync_audio_clips();
            self.sync_loop_region();
            self.request_redraw();
        }
    }

    fn restore_effect_regions(&mut self, snapshots: Vec<EffectRegionSnapshot>) {
        self.effect_regions = snapshots
            .into_iter()
            .map(|snap| {
                let mut region = effects::EffectRegion::new(snap.position, snap.size);
                region.name = snap.name;
                region
            })
            .collect();
    }

    fn restore_plugin_blocks(&mut self, snapshots: Vec<effects::PluginBlockSnapshot>) {
        self.plugin_blocks = snapshots
            .into_iter()
            .map(|snap| {
                let instance = if self.plugin_registry.is_scanned() {
                    self.plugin_registry.load_plugin(&snap.plugin_id, 48000.0, 512)
                } else {
                    None
                };
                effects::PluginBlock {
                    position: snap.position,
                    size: snap.size,
                    color: snap.color,
                    plugin_id: snap.plugin_id,
                    plugin_name: snap.plugin_name,
                    plugin_path: snap.plugin_path,
                    bypass: snap.bypass,
                    instance: Arc::new(std::sync::Mutex::new(instance)),
                    gui: Arc::new(std::sync::Mutex::new(None)),
                    pending_state: None,
                    pending_params: None,
                }
            })
            .collect();
    }

    fn restore_loop_regions(&mut self, snapshots: Vec<LoopRegionSnapshot>) {
        self.loop_regions = snapshots
            .into_iter()
            .map(|snap| LoopRegion {
                position: snap.position,
                size: snap.size,
                enabled: snap.enabled,
            })
            .collect();
    }

    fn restore_export_regions(&mut self, snapshots: Vec<ExportRegionSnapshot>) {
        self.export_regions = snapshots
            .into_iter()
            .map(|snap| ExportRegion {
                position: snap.position,
                size: snap.size,
            })
            .collect();
    }
}
