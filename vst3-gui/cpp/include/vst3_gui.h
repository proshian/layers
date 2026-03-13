#ifndef VST3_GUI_H
#define VST3_GUI_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Vst3GuiHandle Vst3GuiHandle;

// Create GUI from a .vst3 bundle path and plugin UID string.
// Returns NULL if plugin has no GUI or loading fails.
Vst3GuiHandle* vst3_gui_open(const char* vst3_path, const char* uid, const char* title);

// Hide the GUI window (does NOT destroy the plugin instance).
void vst3_gui_close(Vst3GuiHandle* handle);

// Destroy the GUI handle: detach view, terminate component/controller, close window.
void vst3_gui_destroy(Vst3GuiHandle* handle);

// Show a previously hidden GUI window.
void vst3_gui_show(Vst3GuiHandle* handle);

// Check if window is still visible. Returns 1 if visible, 0 if hidden/closed.
int vst3_gui_is_open(Vst3GuiHandle* handle);

// Get view size. Returns 0 on success, -1 on error.
int vst3_gui_get_size(Vst3GuiHandle* handle, float* width, float* height);

// Get component state as a byte buffer. Returns number of bytes written, or -1 on error.
// Pass NULL/0 to query required size.
int vst3_gui_get_state(Vst3GuiHandle* handle, unsigned char* data, int capacity);

// Restore component + controller state from a byte buffer (previously obtained via get_state).
// Returns 0 on success, -1 on error.
int vst3_gui_set_state(Vst3GuiHandle* handle, const unsigned char* data, int size);

// Get total number of parameters on the controller.
int vst3_gui_get_parameter_count(Vst3GuiHandle* handle);

// Get normalized parameter value by index. Returns 0 on success, -1 on error.
int vst3_gui_get_parameter(Vst3GuiHandle* handle, int index, double* value);

// Set normalized parameter value by index. Returns 0 on success, -1 on error.
int vst3_gui_set_parameter(Vst3GuiHandle* handle, int index, double value);

#ifdef __cplusplus
}
#endif

#endif // VST3_GUI_H
