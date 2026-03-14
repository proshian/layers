use std::collections::{HashMap, HashSet};

use crate::component;
use crate::effects;
use crate::grid::{grid_spacing_for_settings, pixels_per_beat};
use crate::hit_testing::canonical_rect;
use crate::regions::{
    ExportRegion, LoopRegion, SelectArea, EXPORT_BORDER_COLOR, EXPORT_FILL_COLOR,
    EXPORT_RENDER_PILL_COLOR, EXPORT_RENDER_PILL_H, EXPORT_RENDER_PILL_W, LOOP_BADGE_COLOR,
    LOOP_BADGE_H, LOOP_BADGE_W, LOOP_BORDER_COLOR, LOOP_FILL_COLOR,
};
use crate::settings::Settings;
use crate::ui;
use crate::ui::waveform::WaveformVertex;
use crate::{push_border, Camera, CanvasObject, HitTarget, InstanceRaw, WaveformView};

pub(crate) const SEL_COLOR: [f32; 4] = [0.35, 0.65, 1.0, 0.8];

pub(crate) struct RenderContext<'a> {
    pub(crate) camera: &'a Camera,
    pub(crate) screen_w: f32,
    pub(crate) screen_h: f32,
    pub(crate) objects: &'a [CanvasObject],
    pub(crate) waveforms: &'a [WaveformView],
    pub(crate) effect_regions: &'a [effects::EffectRegion],
    pub(crate) plugin_blocks: &'a [effects::PluginBlock],
    pub(crate) hovered: Option<HitTarget>,
    pub(crate) selected: &'a HashSet<HitTarget>,
    pub(crate) selection_rect: Option<([f32; 2], [f32; 2])>,
    pub(crate) select_area: Option<&'a SelectArea>,
    pub(crate) file_hovering: bool,
    pub(crate) playhead_world_x: Option<f32>,
    pub(crate) export_regions: &'a [ExportRegion],
    pub(crate) loop_regions: &'a [LoopRegion],
    pub(crate) components: &'a [component::ComponentDef],
    pub(crate) component_instances: &'a [component::ComponentInstance],
    pub(crate) editing_component: Option<usize>,
    pub(crate) settings: &'a Settings,
    pub(crate) component_map: &'a HashMap<component::ComponentId, usize>,
    pub(crate) fade_curve_hovered: Option<(usize, bool)>,
    pub(crate) fade_curve_dragging: Option<(usize, bool)>,
    pub(crate) mouse_world: [f32; 2],
    pub(crate) bpm: f32,
}

pub(crate) fn build_instances(out: &mut Vec<InstanceRaw>, ctx: &RenderContext) {
    out.clear();

    let camera = ctx.camera;
    let world_left = camera.position[0];
    let world_top = camera.position[1];
    let world_right = world_left + ctx.screen_w / camera.zoom;
    let world_bottom = world_top + ctx.screen_h / camera.zoom;

    // --- musical grid ---
    if ctx.settings.grid_enabled {
        let spacing = grid_spacing_for_settings(ctx.settings, camera.zoom, ctx.bpm);
        let ppb = pixels_per_beat(ctx.bpm);
        let bar_spacing = ppb * 4.0;
        let line_w = 1.0 / camera.zoom;
        let major_line_w = 2.0 / camera.zoom;
        let bar_line_w = 2.5 / camera.zoom;
        let grid_i = ctx.settings.grid_line_intensity;
        let minor_color = [1.0, 1.0, 1.0, grid_i * 0.20];
        let beat_color = [1.0, 1.0, 1.0, grid_i * 0.38];
        let bar_color = [1.0, 1.0, 1.0, grid_i * 0.60];

        let first_xi = (world_left / spacing).floor() as i64;
        let last_xi = (world_right / spacing).ceil() as i64;
        for i in first_xi..=last_xi {
            let x = i as f32 * spacing;
            let on_bar = (x / bar_spacing).round() * bar_spacing;
            let on_beat = (x / ppb).round() * ppb;
            let is_bar = (x - on_bar).abs() < 0.01;
            let is_beat = !is_bar && (x - on_beat).abs() < 0.01;
            let (w, c) = if is_bar {
                (bar_line_w, bar_color)
            } else if is_beat {
                (major_line_w, beat_color)
            } else {
                (line_w, minor_color)
            };
            out.push(InstanceRaw {
                position: [x - w * 0.5, world_top],
                size: [w, world_bottom - world_top],
                color: c,
                border_radius: 0.0,
            });
        }

        let first_yi = (world_top / spacing).floor() as i64;
        let last_yi = (world_bottom / spacing).ceil() as i64;
        for i in first_yi..=last_yi {
            let y = i as f32 * spacing;
            let is_major = i % 4 == 0;
            let w = if is_major { major_line_w } else { line_w };
            let c = if is_major { beat_color } else { minor_color };
            out.push(InstanceRaw {
                position: [world_left, y - w * 0.5],
                size: [world_right - world_left, w],
                color: c,
                border_radius: 0.0,
            });
        }
    }

    // --- origin axes ---
    let axis_w = 2.0 / camera.zoom;
    if world_top <= 0.0 && world_bottom >= 0.0 {
        out.push(InstanceRaw {
            position: [world_left, -axis_w * 0.5],
            size: [world_right - world_left, axis_w],
            color: [0.85, 0.25, 0.25, 0.5],
            border_radius: 0.0,
        });
    }
    if world_left <= 0.0 && world_right >= 0.0 {
        out.push(InstanceRaw {
            position: [-axis_w * 0.5, world_top],
            size: [axis_w, world_bottom - world_top],
            color: [0.25, 0.85, 0.25, 0.5],
            border_radius: 0.0,
        });
    }

    // --- effect regions (rendered behind everything) ---
    for (i, er) in ctx.effect_regions.iter().enumerate() {
        let er_right = er.position[0] + er.size[0];
        let er_bottom = er.position[1] + er.size[1];
        if er_right < world_left
            || er.position[0] > world_right
            || er_bottom < world_top
            || er.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::EffectRegion(i));
        let is_hov = ctx.hovered == Some(HitTarget::EffectRegion(i));
        let is_active = ctx.playhead_world_x.map_or(false, |px| {
            px >= er.position[0] && px <= er.position[0] + er.size[0]
        });
        out.extend(effects::build_effect_region_instances(
            er, camera, is_hov, is_sel, is_active,
        ));
    }

    // --- plugin blocks ---
    for (i, pb) in ctx.plugin_blocks.iter().enumerate() {
        let pb_right = pb.position[0] + pb.size[0];
        let pb_bottom = pb.position[1] + pb.size[1];
        if pb_right < world_left
            || pb.position[0] > world_right
            || pb_bottom < world_top
            || pb.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::PluginBlock(i));
        let is_hov = ctx.hovered == Some(HitTarget::PluginBlock(i));
        out.extend(effects::build_plugin_block_instances(
            pb, camera, is_hov, is_sel,
        ));
    }

    // --- export regions ---
    for (i, er) in ctx.export_regions.iter().enumerate() {
        let p = er.position;
        let s = er.size;
        let er_right = p[0] + s[0];
        let er_bottom = p[1] + s[1];
        if er_right < world_left || p[0] > world_right || er_bottom < world_top || p[1] > world_bottom {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::ExportRegion(i));
        let is_hov = ctx.hovered == Some(HitTarget::ExportRegion(i));

        out.push(InstanceRaw {
            position: p,
            size: s,
            color: EXPORT_FILL_COLOR,
            border_radius: 6.0 / camera.zoom,
        });

        let bw = if is_sel { 2.5 } else if is_hov { 2.0 } else { 1.5 } / camera.zoom;
        push_border(out, p, s, bw, EXPORT_BORDER_COLOR);

        let dash_h = 3.0 / camera.zoom;
        let dash_w = 20.0 / camera.zoom;
        let gap = 10.0 / camera.zoom;
        let dy = p[1] - dash_h - 2.0 / camera.zoom;
        let mut dx = p[0];
        while dx < er_right {
            let w = dash_w.min(er_right - dx);
            out.push(InstanceRaw {
                position: [dx, dy],
                size: [w, dash_h],
                color: EXPORT_BORDER_COLOR,
                border_radius: 1.0 / camera.zoom,
            });
            dx += dash_w + gap;
        }

        let pill_w = EXPORT_RENDER_PILL_W / camera.zoom;
        let pill_h = EXPORT_RENDER_PILL_H / camera.zoom;
        let pill_x = p[0] + 4.0 / camera.zoom;
        let pill_y = p[1] + 4.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [pill_x, pill_y],
            size: [pill_w, pill_h],
            color: EXPORT_RENDER_PILL_COLOR,
            border_radius: pill_h * 0.5,
        });

        if is_sel {
            let handle_sz = 8.0 / camera.zoom;
            for &hx in &[p[0] - handle_sz * 0.5, er_right - handle_sz * 0.5] {
                for &hy in &[p[1] - handle_sz * 0.5, er_bottom - handle_sz * 0.5] {
                    out.push(InstanceRaw {
                        position: [hx, hy],
                        size: [handle_sz, handle_sz],
                        color: [0.20, 0.80, 0.60, 0.9],
                        border_radius: 2.0 / camera.zoom,
                    });
                }
            }
        }
    }

    // --- loop regions ---
    for (i, lr) in ctx.loop_regions.iter().enumerate() {
        let p = lr.position;
        let s = lr.size;
        let lr_right = p[0] + s[0];
        let lr_bottom = p[1] + s[1];
        if lr_right < world_left || p[0] > world_right || lr_bottom < world_top || p[1] > world_bottom {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::LoopRegion(i));
        let is_hov = ctx.hovered == Some(HitTarget::LoopRegion(i));
        let alpha_mul = if lr.enabled { 1.0 } else { 0.25 };

        let fill = [LOOP_FILL_COLOR[0], LOOP_FILL_COLOR[1], LOOP_FILL_COLOR[2], LOOP_FILL_COLOR[3] * alpha_mul];
        let border = [LOOP_BORDER_COLOR[0], LOOP_BORDER_COLOR[1], LOOP_BORDER_COLOR[2], LOOP_BORDER_COLOR[3] * alpha_mul];
        let badge = [LOOP_BADGE_COLOR[0], LOOP_BADGE_COLOR[1], LOOP_BADGE_COLOR[2], LOOP_BADGE_COLOR[3] * alpha_mul];

        out.push(InstanceRaw {
            position: p,
            size: s,
            color: fill,
            border_radius: 6.0 / camera.zoom,
        });

        let bw = if is_sel { 2.5 } else if is_hov { 2.0 } else { 1.5 } / camera.zoom;
        push_border(out, p, s, bw, border);

        let dash_h = 3.0 / camera.zoom;
        let dash_w = 20.0 / camera.zoom;
        let gap = 10.0 / camera.zoom;
        let dy = p[1] - dash_h - 2.0 / camera.zoom;
        let mut dx = p[0];
        while dx < lr_right {
            let w = dash_w.min(lr_right - dx);
            out.push(InstanceRaw {
                position: [dx, dy],
                size: [w, dash_h],
                color: border,
                border_radius: 1.0 / camera.zoom,
            });
            dx += dash_w + gap;
        }

        let pill_w = LOOP_BADGE_W / camera.zoom;
        let pill_h = LOOP_BADGE_H / camera.zoom;
        let pill_x = p[0] + 4.0 / camera.zoom;
        let pill_y = p[1] + 4.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [pill_x, pill_y],
            size: [pill_w, pill_h],
            color: badge,
            border_radius: pill_h * 0.5,
        });

        if is_sel {
            let handle_sz = 8.0 / camera.zoom;
            for &hx in &[p[0] - handle_sz * 0.5, lr_right - handle_sz * 0.5] {
                for &hy in &[p[1] - handle_sz * 0.5, lr_bottom - handle_sz * 0.5] {
                    out.push(InstanceRaw {
                        position: [hx, hy],
                        size: [handle_sz, handle_sz],
                        color: [0.30, 0.60, 1.0, 0.9 * alpha_mul],
                        border_radius: 2.0 / camera.zoom,
                    });
                }
            }
        }
    }

    // --- canvas objects ---
    let ci = ctx.settings.color_intensity;
    for (i, obj) in ctx.objects.iter().enumerate() {
        let obj_right = obj.position[0] + obj.size[0];
        let obj_bottom = obj.position[1] + obj.size[1];
        if obj_right < world_left
            || obj.position[0] > world_right
            || obj_bottom < world_top
            || obj.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::Object(i));
        let is_hov = ctx.hovered == Some(HitTarget::Object(i));
        let mut color = obj.color;
        if ci > 0.001 {
            let lum = 0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2];
            let boost = 1.0 + ci * 2.0;
            color[0] = (lum + (color[0] - lum) * boost).clamp(0.0, 1.0);
            color[1] = (lum + (color[1] - lum) * boost).clamp(0.0, 1.0);
            color[2] = (lum + (color[2] - lum) * boost).clamp(0.0, 1.0);
        }
        if is_sel || is_hov {
            color[0] = (color[0] + 0.10).min(1.0);
            color[1] = (color[1] + 0.10).min(1.0);
            color[2] = (color[2] + 0.10).min(1.0);
        }
        out.push(InstanceRaw {
            position: obj.position,
            size: obj.size,
            color,
            border_radius: obj.border_radius,
        });
    }

    // --- waveforms ---
    for (i, wf) in ctx.waveforms.iter().enumerate() {
        let wf_right = wf.position[0] + wf.size[0];
        let wf_bottom = wf.position[1] + wf.size[1];
        if wf_right < world_left
            || wf.position[0] > world_right
            || wf_bottom < world_top
            || wf.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::Waveform(i));
        let is_hov = ctx.hovered == Some(HitTarget::Waveform(i));
        out.extend(ui::waveform::build_waveform_instances(
            wf,
            camera,
            world_left,
            world_right,
            is_hov,
            is_sel,
        ));
    }

    // --- component definitions ---
    for (i, def) in ctx.components.iter().enumerate() {
        let def_right = def.position[0] + def.size[0];
        let def_bottom = def.position[1] + def.size[1];
        if def_right < world_left
            || def.position[0] > world_right
            || def_bottom < world_top
            || def.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::ComponentDef(i));
        let is_hov = ctx.hovered == Some(HitTarget::ComponentDef(i));
        let is_editing = ctx.editing_component == Some(i);
        out.extend(component::build_component_def_instances(
            def,
            camera,
            is_hov,
            is_sel || is_editing,
        ));
    }

    // --- component instances ---
    for (i, inst) in ctx.component_instances.iter().enumerate() {
        if let Some(&def_idx) = ctx.component_map.get(&inst.component_id) {
            let def = &ctx.components[def_idx];
            let inst_right = inst.position[0] + def.size[0];
            let inst_bottom = inst.position[1] + def.size[1];
            if inst_right < world_left
                || inst.position[0] > world_right
                || inst_bottom < world_top
                || inst.position[1] > world_bottom
            {
                continue;
            }
            let is_sel = ctx.selected.contains(&HitTarget::ComponentInstance(i));
            let is_hov = ctx.hovered == Some(HitTarget::ComponentInstance(i));
            out.extend(component::build_component_instance_instances(
                inst,
                def,
                ctx.waveforms,
                camera,
                world_left,
                world_right,
                is_hov,
                is_sel,
            ));
        }
    }

    // --- edit mode dimming overlay ---
    if let Some(ec_idx) = ctx.editing_component {
        if let Some(def) = ctx.components.get(ec_idx) {
            // Dim everything outside the component with 4 dark rectangles
            let dim_color = [0.0, 0.0, 0.0, 0.50];
            // Top strip
            out.push(InstanceRaw {
                position: [world_left, world_top],
                size: [world_right - world_left, def.position[1] - world_top],
                color: dim_color,
                border_radius: 0.0,
            });
            // Bottom strip
            let bot_y = def.position[1] + def.size[1];
            out.push(InstanceRaw {
                position: [world_left, bot_y],
                size: [world_right - world_left, world_bottom - bot_y],
                color: dim_color,
                border_radius: 0.0,
            });
            // Left strip
            out.push(InstanceRaw {
                position: [world_left, def.position[1]],
                size: [def.position[0] - world_left, def.size[1]],
                color: dim_color,
                border_radius: 0.0,
            });
            // Right strip
            let right_x = def.position[0] + def.size[0];
            out.push(InstanceRaw {
                position: [right_x, def.position[1]],
                size: [world_right - right_x, def.size[1]],
                color: dim_color,
                border_radius: 0.0,
            });
        }
    }

    // --- selection highlights (rendered on top of everything) ---
    let sel_bw = 2.0 / camera.zoom;
    let handle_sz = 8.0 / camera.zoom;
    for target in ctx.selected.iter() {
        let (pos, size) = target_rect(
            ctx.objects,
            ctx.waveforms,
            ctx.effect_regions,
            ctx.plugin_blocks,
            ctx.loop_regions,
            ctx.export_regions,
            ctx.components,
            ctx.component_instances,
            ctx.component_map,
            target,
        );
        push_border(out, pos, size, sel_bw, SEL_COLOR);

        for &hx in &[pos[0] - handle_sz * 0.5, pos[0] + size[0] - handle_sz * 0.5] {
            for &hy in &[pos[1] - handle_sz * 0.5, pos[1] + size[1] - handle_sz * 0.5] {
                out.push(InstanceRaw {
                    position: [hx, hy],
                    size: [handle_sz, handle_sz],
                    color: [1.0, 1.0, 1.0, 1.0],
                    border_radius: 2.0 / camera.zoom,
                });
            }
        }
    }

    // --- selection rectangle (transient, during drag) ---
    if let Some((start, current)) = ctx.selection_rect {
        let (rp, rs) = canonical_rect(start, current);
        out.push(InstanceRaw {
            position: rp,
            size: rs,
            color: [0.30, 0.55, 1.0, 0.10],
            border_radius: 0.0,
        });
        let bw = 1.0 / camera.zoom;
        push_border(out, rp, rs, bw, [0.35, 0.65, 1.0, 0.5]);
    } else if let Some(sa) = ctx.select_area {
        out.push(InstanceRaw {
            position: sa.position,
            size: sa.size,
            color: [0.30, 0.55, 1.0, 0.10],
            border_radius: 0.0,
        });
        let bw = 1.0 / camera.zoom;
        push_border(out, sa.position, sa.size, bw, [0.35, 0.65, 1.0, 0.5]);
    }

    // --- playback cursor ---
    if let Some(px) = ctx.playhead_world_x {
        let line_w = 2.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [px - line_w * 0.5, world_top],
            size: [line_w, world_bottom - world_top],
            color: [1.0, 1.0, 1.0, 0.85],
            border_radius: 0.0,
        });
        let head_sz = 10.0 / camera.zoom;
        out.push(InstanceRaw {
            position: [px - head_sz * 0.5, world_top],
            size: [head_sz, head_sz],
            color: [1.0, 1.0, 1.0, 0.95],
            border_radius: 2.0 / camera.zoom,
        });
    }

    // --- file drop zone overlay ---
    if ctx.file_hovering {
        out.push(InstanceRaw {
            position: [world_left, world_top],
            size: [world_right - world_left, world_bottom - world_top],
            color: [0.25, 0.50, 1.0, 0.10],
            border_radius: 0.0,
        });
        let bw = 3.0 / camera.zoom;
        push_border(
            out,
            [world_left, world_top],
            [world_right - world_left, world_bottom - world_top],
            bw,
            [0.35, 0.65, 1.0, 0.7],
        );
    }
}

pub(crate) fn build_waveform_vertices(verts: &mut Vec<WaveformVertex>, ctx: &RenderContext) {
    verts.clear();
    let camera = ctx.camera;
    let world_left = camera.position[0];
    let world_right = world_left + ctx.screen_w / camera.zoom;
    let world_top = camera.position[1];
    let world_bottom = world_top + ctx.screen_h / camera.zoom;
    for (i, wf) in ctx.waveforms.iter().enumerate() {
        let wf_right = wf.position[0] + wf.size[0];
        let wf_bottom = wf.position[1] + wf.size[1];
        if wf_right < world_left
            || wf.position[0] > world_right
            || wf_bottom < world_top
            || wf.position[1] > world_bottom
        {
            continue;
        }
        let is_sel = ctx.selected.contains(&HitTarget::Waveform(i));
        let is_hov = ctx.hovered == Some(HitTarget::Waveform(i));
        verts.extend(ui::waveform::build_waveform_triangles(
            wf,
            camera,
            world_left,
            world_right,
            is_hov,
            is_sel,
        ));
        // Fade curve lines as smooth triangles (line only when cursor is near)
        let mx = ctx.mouse_world[0];
        let my = ctx.mouse_world[1];
        let in_wf_y = my >= wf.position[1] && my <= wf.position[1] + wf.size[1];
        let mouse_in_fi = wf.fade_in_px > 0.0 && in_wf_y
            && mx >= wf.position[0] && mx <= wf.position[0] + wf.fade_in_px;
        let mouse_in_fo = wf.fade_out_px > 0.0 && in_wf_y
            && mx >= wf.position[0] + wf.size[0] - wf.fade_out_px && mx <= wf.position[0] + wf.size[0];
        let show_fi_line = mouse_in_fi
            || matches!(ctx.fade_curve_hovered, Some((idx, true)) if idx == i)
            || matches!(ctx.fade_curve_dragging, Some((idx, true)) if idx == i);
        let show_fo_line = mouse_in_fo
            || matches!(ctx.fade_curve_hovered, Some((idx, false)) if idx == i)
            || matches!(ctx.fade_curve_dragging, Some((idx, false)) if idx == i);
        verts.extend(ui::waveform::build_fade_curve_triangles(wf, camera, show_fi_line, show_fo_line));
    }
}

pub(crate) fn target_rect(
    objects: &[CanvasObject],
    waveforms: &[WaveformView],
    effect_regions: &[effects::EffectRegion],
    plugin_blocks: &[effects::PluginBlock],
    loop_regions: &[LoopRegion],
    export_regions: &[ExportRegion],
    components: &[component::ComponentDef],
    component_instances: &[component::ComponentInstance],
    component_map: &HashMap<component::ComponentId, usize>,
    target: &HitTarget,
) -> ([f32; 2], [f32; 2]) {
    match target {
        HitTarget::Object(i) => (objects[*i].position, objects[*i].size),
        HitTarget::Waveform(i) => (waveforms[*i].position, waveforms[*i].size),
        HitTarget::EffectRegion(i) => (effect_regions[*i].position, effect_regions[*i].size),
        HitTarget::PluginBlock(i) => (plugin_blocks[*i].position, plugin_blocks[*i].size),
        HitTarget::LoopRegion(i) => (loop_regions[*i].position, loop_regions[*i].size),
        HitTarget::ExportRegion(i) => (export_regions[*i].position, export_regions[*i].size),
        HitTarget::ComponentDef(i) => (components[*i].position, components[*i].size),
        HitTarget::ComponentInstance(i) => {
            let inst = &component_instances[*i];
            let def = component_map
                .get(&inst.component_id)
                .map(|&idx| &components[idx]);
            match def {
                Some(d) => (inst.position, d.size),
                None => (inst.position, [100.0, 100.0]),
            }
        }
    }
}

pub(crate) fn default_objects() -> Vec<CanvasObject> {
    vec![]
}
