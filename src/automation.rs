#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AutomationParam {
    Volume,
    Pan,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AutomationPoint {
    pub t: f32,
    pub value: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AutomationLane {
    pub param: AutomationParam,
    pub points: Vec<AutomationPoint>,
    pub default_value: f32,
}

impl AutomationLane {
    pub fn new(param: AutomationParam) -> Self {
        let default_value = match param {
            AutomationParam::Volume => 0.5,
            AutomationParam::Pan => 0.5,
        };
        Self {
            param,
            points: Vec::new(),
            default_value,
        }
    }

    pub fn value_at(&self, t: f32) -> f32 {
        if self.points.is_empty() {
            return self.default_value;
        }
        if t <= self.points[0].t {
            return self.points[0].value;
        }
        let last = self.points.len() - 1;
        if t >= self.points[last].t {
            return self.points[last].value;
        }
        // Binary search for the segment containing t
        let idx = match self.points.binary_search_by(|p| p.t.partial_cmp(&t).unwrap()) {
            Ok(i) => return self.points[i].value,
            Err(i) => i,
        };
        let a = &self.points[idx - 1];
        let b = &self.points[idx];
        let frac = (t - a.t) / (b.t - a.t);
        a.value + (b.value - a.value) * frac
    }

    pub fn is_default(&self) -> bool {
        self.points.is_empty()
    }

    pub fn insert_point(&mut self, t: f32, value: f32) -> usize {
        let t = t.clamp(0.0, 1.0);
        let value = value.clamp(0.0, 1.0);
        let idx = self
            .points
            .binary_search_by(|p| p.t.partial_cmp(&t).unwrap())
            .unwrap_or_else(|i| i);
        self.points.insert(idx, AutomationPoint { t, value });
        idx
    }

    pub fn remove_point(&mut self, idx: usize) {
        if idx < self.points.len() {
            self.points.remove(idx);
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AutomationData {
    pub lanes: Vec<AutomationLane>,
}

impl AutomationData {
    pub fn new() -> Self {
        Self {
            lanes: vec![
                AutomationLane::new(AutomationParam::Volume),
                AutomationLane::new(AutomationParam::Pan),
            ],
        }
    }

    pub fn volume_lane(&self) -> &AutomationLane {
        &self.lanes[0]
    }

    pub fn volume_lane_mut(&mut self) -> &mut AutomationLane {
        &mut self.lanes[0]
    }

    pub fn pan_lane(&self) -> &AutomationLane {
        &self.lanes[1]
    }

    pub fn pan_lane_mut(&mut self) -> &mut AutomationLane {
        &mut self.lanes[1]
    }

    pub fn lane_for(&self, param: AutomationParam) -> &AutomationLane {
        match param {
            AutomationParam::Volume => &self.lanes[0],
            AutomationParam::Pan => &self.lanes[1],
        }
    }

    pub fn lane_for_mut(&mut self, param: AutomationParam) -> &mut AutomationLane {
        match param {
            AutomationParam::Volume => &mut self.lanes[0],
            AutomationParam::Pan => &mut self.lanes[1],
        }
    }
}

impl AutomationData {
    pub fn from_stored(volume_pts: &[[f32; 2]], pan_pts: &[[f32; 2]]) -> Self {
        let mut data = Self::new();
        for &[t, v] in volume_pts {
            data.volume_lane_mut().insert_point(t, v);
        }
        for &[t, v] in pan_pts {
            data.pan_lane_mut().insert_point(t, v);
        }
        data
    }
}

/// Convert automation value (0.0–1.0) to gain.
/// 0.0 → silence, 0.5 → 1.0 (0dB), 1.0 → 4.0 (+12dB).
pub fn volume_value_to_gain(value: f32) -> f32 {
    let v2 = value * 2.0;
    v2 * v2
}

/// Linear interpolation for automation pairs (used in audio thread).
pub fn interp_automation(t: f32, pairs: &[(f32, f32)], default: f32) -> f32 {
    if pairs.is_empty() {
        return default;
    }
    if t <= pairs[0].0 {
        return pairs[0].1;
    }
    let last = pairs.len() - 1;
    if t >= pairs[last].0 {
        return pairs[last].1;
    }
    let idx = match pairs.binary_search_by(|p| p.0.partial_cmp(&t).unwrap()) {
        Ok(i) => return pairs[i].1,
        Err(i) => i,
    };
    let (at, av) = pairs[idx - 1];
    let (bt, bv) = pairs[idx];
    let frac = (t - at) / (bt - at);
    av + (bv - av) * frac
}
