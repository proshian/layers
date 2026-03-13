use std::ffi::CString;

extern "C" {
    fn vst3_gui_open(
        path: *const std::ffi::c_char,
        uid: *const std::ffi::c_char,
        title: *const std::ffi::c_char,
    ) -> *mut std::ffi::c_void;
    fn vst3_gui_close(handle: *mut std::ffi::c_void);
    fn vst3_gui_destroy(handle: *mut std::ffi::c_void);
    fn vst3_gui_show(handle: *mut std::ffi::c_void);
    fn vst3_gui_is_open(handle: *mut std::ffi::c_void) -> i32;
    fn vst3_gui_get_size(handle: *mut std::ffi::c_void, w: *mut f32, h: *mut f32) -> i32;
    fn vst3_gui_get_state(handle: *mut std::ffi::c_void, data: *mut u8, capacity: i32) -> i32;
    fn vst3_gui_set_state(handle: *mut std::ffi::c_void, data: *const u8, size: i32) -> i32;
    fn vst3_gui_get_parameter_count(handle: *mut std::ffi::c_void) -> i32;
    fn vst3_gui_get_parameter(handle: *mut std::ffi::c_void, index: i32, value: *mut f64) -> i32;
    fn vst3_gui_set_parameter(handle: *mut std::ffi::c_void, index: i32, value: f64) -> i32;
}

pub struct Vst3Gui {
    handle: *mut std::ffi::c_void,
}

// The handle is a pointer to a C++ struct that manages its own thread safety.
// GUI operations must happen on the main thread (macOS requirement), which is
// where we always call these functions from.
unsafe impl Send for Vst3Gui {}

impl Vst3Gui {
    /// Open a VST3 plugin's native GUI window.
    pub fn open(vst3_path: &str, uid: &str, title: &str) -> Option<Self> {
        let c_path = CString::new(vst3_path).ok()?;
        let c_uid = CString::new(uid).ok()?;
        let c_title = CString::new(title).ok()?;

        let handle =
            unsafe { vst3_gui_open(c_path.as_ptr(), c_uid.as_ptr(), c_title.as_ptr()) };

        if handle.is_null() {
            None
        } else {
            Some(Vst3Gui { handle })
        }
    }

    /// Returns true if the plugin window is still visible.
    pub fn is_open(&self) -> bool {
        unsafe { vst3_gui_is_open(self.handle) != 0 }
    }

    /// Hide the plugin window (keeps the plugin instance alive).
    pub fn hide(&self) {
        unsafe { vst3_gui_close(self.handle) }
    }

    /// Show a previously hidden plugin window.
    pub fn show(&self) {
        unsafe { vst3_gui_show(self.handle) }
    }

    /// Get the current view size, or `None` on error.
    pub fn get_size(&self) -> Option<(f32, f32)> {
        let mut w: f32 = 0.0;
        let mut h: f32 = 0.0;
        let ret = unsafe { vst3_gui_get_size(self.handle, &mut w, &mut h) };
        if ret == 0 {
            Some((w, h))
        } else {
            None
        }
    }

    /// Get the number of parameters on the GUI's controller.
    pub fn parameter_count(&self) -> usize {
        let n = unsafe { vst3_gui_get_parameter_count(self.handle) };
        if n < 0 { 0 } else { n as usize }
    }

    /// Get a normalized parameter value by index.
    pub fn get_parameter(&self, index: usize) -> Option<f64> {
        let mut value: f64 = 0.0;
        let ret = unsafe { vst3_gui_get_parameter(self.handle, index as i32, &mut value) };
        if ret == 0 { Some(value) } else { None }
    }

    /// Set a normalized parameter value by index.
    pub fn set_parameter(&self, index: usize, value: f64) -> bool {
        unsafe { vst3_gui_set_parameter(self.handle, index as i32, value) == 0 }
    }

    /// Get the GUI component's current state as bytes.
    pub fn get_state(&self) -> Option<Vec<u8>> {
        unsafe {
            let size = vst3_gui_get_state(self.handle, std::ptr::null_mut(), 0);
            if size <= 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let read = vst3_gui_get_state(self.handle, buf.as_mut_ptr(), size);
            if read <= 0 {
                return None;
            }
            buf.resize(read as usize, 0);
            Some(buf)
        }
    }

    /// Restore component + controller state from bytes previously saved via `get_state()`.
    pub fn set_state(&self, data: &[u8]) -> bool {
        unsafe { vst3_gui_set_state(self.handle, data.as_ptr(), data.len() as i32) == 0 }
    }

    /// Get all parameter values as a Vec of (index, normalized_value) pairs.
    pub fn get_all_parameters(&self) -> Vec<f64> {
        let count = self.parameter_count();
        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            values.push(self.get_parameter(i).unwrap_or(0.0));
        }
        values
    }

    /// Set all parameter values from a slice of normalized values.
    pub fn set_all_parameters(&self, values: &[f64]) {
        let count = self.parameter_count().min(values.len());
        for i in 0..count {
            self.set_parameter(i, values[i]);
        }
    }
}

impl Drop for Vst3Gui {
    fn drop(&mut self) {
        unsafe { vst3_gui_destroy(self.handle) }
    }
}
