#import <Cocoa/Cocoa.h>

#include "pluginterfaces/base/funknown.h"
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstmessage.h"
#include "pluginterfaces/vst/ivstevents.h"
#include "pluginterfaces/gui/iplugview.h"
#include "pluginterfaces/base/ustring.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/plugprovider.h"
#include "public.sdk/source/vst/hosting/connectionproxy.h"
#include "public.sdk/source/common/memorystream.h"

#include "vst3_gui.h"

#include <string>
#include <cstring>
#include <vector>
#include <mutex>

using namespace Steinberg;
using namespace Steinberg::Vst;

// --- Minimal IComponentHandler so plugin can notify host of param changes ---

class ComponentHandlerImpl : public IComponentHandler {
public:
    ComponentHandlerImpl() : refCount(1) {}

    tresult PLUGIN_API beginEdit(ParamID /*id*/) override { return kResultOk; }
    tresult PLUGIN_API performEdit(ParamID /*id*/, ParamValue /*valueNormalized*/) override { return kResultOk; }
    tresult PLUGIN_API endEdit(ParamID /*id*/) override { return kResultOk; }
    tresult PLUGIN_API restartComponent(int32 /*flags*/) override { return kResultOk; }

    tresult PLUGIN_API queryInterface(const TUID _iid, void** obj) override {
        if (FUnknownPrivate::iidEqual(_iid, IComponentHandler::iid) ||
            FUnknownPrivate::iidEqual(_iid, FUnknown::iid)) {
            *obj = this;
            addRef();
            return kResultOk;
        }
        *obj = nullptr;
        return kNoInterface;
    }
    uint32 PLUGIN_API addRef() override { return ++refCount; }
    uint32 PLUGIN_API release() override {
        if (--refCount == 0) { delete this; return 0; }
        return refCount;
    }
private:
    std::atomic<uint32> refCount;
};

// --- PlugFrame implementation for resize support ---

class PlugFrameImpl : public IPlugFrame {
public:
    PlugFrameImpl(NSWindow* window, NSView* container)
        : refCount(1), window_(window), container_(container) {}

    tresult PLUGIN_API resizeView(IPlugView* view, ViewRect* newSize) override {
        if (!newSize || !window_ || !container_) return kResultFalse;

        float w = static_cast<float>(newSize->right - newSize->left);
        float h = static_cast<float>(newSize->bottom - newSize->top);

        NSRect frame = [window_ frame];
        NSRect contentRect = [window_ contentRectForFrameRect:frame];

        float deltaW = w - contentRect.size.width;
        float deltaH = h - contentRect.size.height;

        frame.size.width += deltaW;
        frame.size.height += deltaH;
        frame.origin.y -= deltaH;

        [window_ setFrame:frame display:YES animate:NO];
        [container_ setFrameSize:NSMakeSize(w, h)];

        return kResultOk;
    }

    tresult PLUGIN_API queryInterface(const TUID _iid, void** obj) override {
        if (FUnknownPrivate::iidEqual(_iid, IPlugFrame::iid) ||
            FUnknownPrivate::iidEqual(_iid, FUnknown::iid)) {
            *obj = this;
            addRef();
            return kResultOk;
        }
        *obj = nullptr;
        return kNoInterface;
    }

    uint32 PLUGIN_API addRef() override { return ++refCount; }
    uint32 PLUGIN_API release() override {
        if (--refCount == 0) { delete this; return 0; }
        return refCount;
    }

private:
    std::atomic<uint32> refCount;
    NSWindow* window_;
    NSView* container_;
};

// --- Minimal IEventList for MIDI events ---

class EventListImpl : public IEventList {
public:
    EventListImpl() : refCount(1) {}

    int32 PLUGIN_API getEventCount() override { return (int32)events.size(); }

    tresult PLUGIN_API getEvent(int32 index, Event& e) override {
        if (index < 0 || index >= (int32)events.size()) return kResultFalse;
        e = events[index];
        return kResultOk;
    }

    tresult PLUGIN_API addEvent(Event& e) override {
        events.push_back(e);
        return kResultOk;
    }

    void clear() { events.clear(); }

    tresult PLUGIN_API queryInterface(const TUID _iid, void** obj) override {
        if (FUnknownPrivate::iidEqual(_iid, IEventList::iid) ||
            FUnknownPrivate::iidEqual(_iid, FUnknown::iid)) {
            *obj = this;
            addRef();
            return kResultOk;
        }
        *obj = nullptr;
        return kNoInterface;
    }
    uint32 PLUGIN_API addRef() override { return ++refCount; }
    uint32 PLUGIN_API release() override {
        if (--refCount == 0) { delete this; return 0; }
        return refCount;
    }

    std::vector<Event> events;
private:
    std::atomic<uint32> refCount;
};

// --- Handle struct ---

struct Vst3GuiHandle {
    VST3::Hosting::Module::Ptr module;
    IPtr<IComponent> component;
    IPtr<IEditController> controller;
    IPtr<IPlugView> view;
    NSWindow* window;
    NSView* containerView;
    PlugFrameImpl* plugFrame;
    ComponentHandlerImpl* componentHandler;
    bool isSingleComponent;
    // Connection proxies for component↔controller messaging
    IPtr<ConnectionProxy> componentCP;
    IPtr<ConnectionProxy> controllerCP;
    // Audio processing
    IAudioProcessor* processor = nullptr;
    int inputChannels = 0;
    int outputChannels = 0;
    bool processingSetUp = false;
    std::vector<Event> pendingMidiEvents;
    std::mutex midiMutex;
};

static Steinberg::Vst::HostApplication& getHostApp() {
    static Steinberg::Vst::HostApplication sHostApp;
    return sHostApp;
}

extern "C" {

Vst3GuiHandle* vst3_gui_open(const char* vst3_path, const char* uid_str, const char* title) {
    if (!vst3_path || !uid_str) return nullptr;

    try {
    std::string errMsg;

    // 1. Load VST3 module
    auto module = VST3::Hosting::Module::create(vst3_path, errMsg);
    if (!module) {
        fprintf(stderr, "vst3_gui: failed to load module '%s': %s\n", vst3_path, errMsg.c_str());
        return nullptr;
    }

    auto factory = module->getFactory();
    if (!factory.get()) {
        fprintf(stderr, "vst3_gui: no factory in module\n");
        return nullptr;
    }

    // 2. Find and create IComponent + IEditController
    IPtr<IComponent> component;
    IPtr<IEditController> controller;
    bool isSingleComponent = false;

    // Normalize UID for comparison: extract only hex digits, uppercase
    auto normalize = [](const std::string& s) -> std::string {
        std::string result;
        for (char c : s) {
            if (isxdigit(c)) result += toupper(c);
        }
        return result;
    };

    std::string targetNorm = normalize(std::string(uid_str));

    for (auto& classInfo : factory.classInfos()) {
        if (classInfo.category() != kVstAudioEffectClass) continue;

        std::string classNorm = normalize(classInfo.ID().toString());
        if (targetNorm != classNorm) continue;

        fprintf(stderr, "vst3_gui: found matching class '%s'\n", classInfo.name().c_str());

        // Create component
        component = factory.createInstance<IComponent>(classInfo.ID());
        if (!component) {
            fprintf(stderr, "vst3_gui: failed to create IComponent\n");
            continue;
        }

        // Initialize component
        if (component->initialize(&getHostApp()) != kResultOk) {
            fprintf(stderr, "vst3_gui: failed to initialize IComponent\n");
            component = nullptr;
            continue;
        }

        // Get IEditController - try combined first
        if (component->queryInterface(IEditController::iid, (void**)&controller) == kResultTrue) {
            isSingleComponent = true;
            fprintf(stderr, "vst3_gui: using single-component controller\n");
        } else {
            // Create separate controller
            TUID controllerCID;
            if (component->getControllerClassId(controllerCID) == kResultTrue) {
                controller = factory.createInstance<IEditController>(VST3::UID(controllerCID));
                if (controller) {
                    if (controller->initialize(&getHostApp()) != kResultOk) {
                        fprintf(stderr, "vst3_gui: failed to initialize controller\n");
                        controller = nullptr;
                    } else {
                        fprintf(stderr, "vst3_gui: created separate controller\n");
                    }
                }
            }
        }

        if (controller) break;
        component->terminate();
        component = nullptr;
    }

    if (!controller) {
        fprintf(stderr, "vst3_gui: no IEditController found for uid '%s'\n", uid_str);
        if (component) component->terminate();
        return nullptr;
    }

    // 3. Set component handler on controller (required by many plugins)
    auto* componentHandler = new ComponentHandlerImpl();
    controller->setComponentHandler(componentHandler);

    // 4. Connect component ↔ controller (for separate component/controller)
    IPtr<ConnectionProxy> componentCP;
    IPtr<ConnectionProxy> controllerCP;

    if (!isSingleComponent) {
        auto compICP = U::cast<IConnectionPoint>(component);
        auto ctrlICP = U::cast<IConnectionPoint>(controller);
        if (compICP && ctrlICP) {
            componentCP = owned(new ConnectionProxy(compICP));
            controllerCP = owned(new ConnectionProxy(ctrlICP));
            componentCP->connect(ctrlICP);
            controllerCP->connect(compICP);
            fprintf(stderr, "vst3_gui: connected component ↔ controller\n");
        }
    }

    // 5. Synchronize component state → controller
    {
        MemoryStream stream;
        if (component->getState(&stream) == kResultOk) {
            stream.seek(0, IBStream::kIBSeekSet, nullptr);
            controller->setComponentState(&stream);
            fprintf(stderr, "vst3_gui: synced component state to controller\n");
        }
    }

    // 6. Create view
    IPtr<IPlugView> view = owned(controller->createView("editor"));
    if (!view) {
        fprintf(stderr, "vst3_gui: plugin has no editor view\n");
        // Cleanup
        if (componentCP) { componentCP->disconnect(); controllerCP->disconnect(); }
        if (!isSingleComponent && controller) controller->terminate();
        if (component) component->terminate();
        componentHandler->release();
        return nullptr;
    }

    // Check platform support
    if (view->isPlatformTypeSupported(kPlatformTypeNSView) != kResultOk) {
        fprintf(stderr, "vst3_gui: plugin view does not support NSView\n");
        if (componentCP) { componentCP->disconnect(); controllerCP->disconnect(); }
        if (!isSingleComponent && controller) controller->terminate();
        if (component) component->terminate();
        componentHandler->release();
        return nullptr;
    }

    // 7. Get initial size
    ViewRect rect = {};
    if (view->getSize(&rect) != kResultOk) {
        rect.left = 0; rect.top = 0;
        rect.right = 800; rect.bottom = 600;
    }

    float viewW = static_cast<float>(rect.right - rect.left);
    float viewH = static_cast<float>(rect.bottom - rect.top);
    if (viewW < 100) viewW = 800;
    if (viewH < 100) viewH = 600;

    // 8. Create NSWindow
    NSString* nsTitle = title ? [NSString stringWithUTF8String:title]
                              : @"VST3 Plugin";

    NSRect contentRect = NSMakeRect(200, 200, viewW, viewH);
    NSUInteger styleMask = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
                         | NSWindowStyleMaskMiniaturizable;

    NSWindow* window = [[NSWindow alloc] initWithContentRect:contentRect
                                                   styleMask:styleMask
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    [window setTitle:nsTitle];
    [window setReleasedWhenClosed:NO];

    // 9. Create container view
    NSView* containerView = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, viewW, viewH)];
    [window setContentView:containerView];

    // 10. Set up plug frame for resize
    auto* plugFrame = new PlugFrameImpl(window, containerView);
    view->setFrame(plugFrame);

    // 11. Attach view to NSView
    if (view->attached((__bridge void*)containerView, kPlatformTypeNSView) != kResultOk) {
        fprintf(stderr, "vst3_gui: failed to attach view\n");
        view->setFrame(nullptr);
        plugFrame->release();
        if (componentCP) { componentCP->disconnect(); controllerCP->disconnect(); }
        if (!isSingleComponent && controller) controller->terminate();
        if (component) component->terminate();
        componentHandler->release();
        [window close];
        return nullptr;
    }

    // 12. Show window
    [window center];
    [window makeKeyAndOrderFront:nil];

    // 13. Build handle
    auto* handle = new Vst3GuiHandle();
    handle->module = module;
    handle->component = component;
    handle->controller = controller;
    handle->view = view;
    handle->window = window;
    handle->containerView = containerView;
    handle->plugFrame = plugFrame;
    handle->componentHandler = componentHandler;
    handle->isSingleComponent = isSingleComponent;
    handle->componentCP = componentCP;
    handle->controllerCP = controllerCP;

    fprintf(stderr, "vst3_gui: opened GUI for '%s' (%gx%g)\n",
            title ? title : "?", viewW, viewH);

    return handle;

    } catch (const std::exception& e) {
        fprintf(stderr, "vst3_gui_open: C++ exception: %s\n", e.what());
        return nullptr;
    } catch (...) {
        fprintf(stderr, "vst3_gui_open: unknown C++ exception\n");
        return nullptr;
    }
}

// Open plugin without GUI window (no NSWindow/NSView, works from any thread).
// For audio processing, state, and parameters only.
Vst3GuiHandle* vst3_gui_open_headless(const char* vst3_path, const char* uid_str) {
    if (!vst3_path || !uid_str) return nullptr;

    try {
    std::string errMsg;
    auto module = VST3::Hosting::Module::create(vst3_path, errMsg);
    if (!module) return nullptr;

    auto factory = module->getFactory();
    if (!factory.get()) return nullptr;

    IPtr<IComponent> component;
    IPtr<IEditController> controller;
    bool isSingleComponent = false;

    auto normalize = [](const std::string& s) -> std::string {
        std::string result;
        for (char c : s) { if (isxdigit(c)) result += toupper(c); }
        return result;
    };
    std::string targetNorm = normalize(std::string(uid_str));

    for (auto& classInfo : factory.classInfos()) {
        if (classInfo.category() != kVstAudioEffectClass) continue;
        if (normalize(classInfo.ID().toString()) != targetNorm) continue;

        component = factory.createInstance<IComponent>(classInfo.ID());
        if (!component) continue;
        if (component->initialize(&getHostApp()) != kResultOk) { component = nullptr; continue; }

        if (component->queryInterface(IEditController::iid, (void**)&controller) == kResultTrue) {
            isSingleComponent = true;
        } else {
            TUID cid;
            if (component->getControllerClassId(cid) == kResultTrue) {
                controller = factory.createInstance<IEditController>(VST3::UID(cid));
                if (controller && controller->initialize(&getHostApp()) != kResultOk) controller = nullptr;
            }
        }
        if (controller) break;
        component->terminate();
        component = nullptr;
    }

    if (!controller) { if (component) component->terminate(); return nullptr; }

    auto* ch = new ComponentHandlerImpl();
    controller->setComponentHandler(ch);

    IPtr<ConnectionProxy> componentCP, controllerCP;
    if (!isSingleComponent) {
        auto compICP = U::cast<IConnectionPoint>(component);
        auto ctrlICP = U::cast<IConnectionPoint>(controller);
        if (compICP && ctrlICP) {
            componentCP = owned(new ConnectionProxy(compICP));
            controllerCP = owned(new ConnectionProxy(ctrlICP));
            componentCP->connect(ctrlICP);
            controllerCP->connect(compICP);
        }
    }

    { MemoryStream s; if (component->getState(&s) == kResultOk) { s.seek(0, IBStream::kIBSeekSet, nullptr); controller->setComponentState(&s); } }

    auto* handle = new Vst3GuiHandle();
    handle->module = module;
    handle->component = component;
    handle->controller = controller;
    handle->view = nullptr;
    handle->window = nil;
    handle->containerView = nil;
    handle->plugFrame = nullptr;
    handle->componentHandler = ch;
    handle->isSingleComponent = isSingleComponent;
    handle->componentCP = componentCP;
    handle->controllerCP = controllerCP;
    return handle;

    } catch (...) {
        fprintf(stderr, "vst3_gui_open_headless: exception\n");
        return nullptr;
    }
}

void vst3_gui_close(Vst3GuiHandle* handle) {
    if (!handle) return;

    // Just hide the window — do NOT terminate/release plugin objects.
    if (handle->window && [handle->window isVisible]) {
        [handle->window orderOut:nil];
    }

    fprintf(stderr, "vst3_gui_close: window hidden\n");
}

void vst3_gui_destroy(Vst3GuiHandle* handle) {
    if (!handle) return;

    try {
        // 1. Hide window first to stop any rendering/interaction
        if (handle->window) {
            [handle->window orderOut:nil];
        }

        // 2. Detach the view from the container
        if (handle->view) {
            handle->view->setFrame(nullptr);
            handle->view->removed();
        }

        // 3. Close and release window (before terminating plugin objects)
        if (handle->window) {
            [handle->window setContentView:nil];
            [handle->window close];
            handle->window = nil;
        }
        handle->containerView = nil;

        // 4. Release the view (after window is gone)
        handle->view = nullptr;

        // 5. Disconnect connection proxies
        if (handle->componentCP) {
            handle->componentCP->disconnect();
            handle->componentCP = nullptr;
        }
        if (handle->controllerCP) {
            handle->controllerCP->disconnect();
            handle->controllerCP = nullptr;
        }

        // 6. Release plug frame and component handler before terminating
        if (handle->plugFrame) {
            handle->plugFrame->release();
            handle->plugFrame = nullptr;
        }
        if (handle->componentHandler) {
            handle->componentHandler->release();
            handle->componentHandler = nullptr;
        }

        // 7. Terminate controller (only if separate from component)
        if (handle->controller) {
            if (!handle->isSingleComponent) {
                handle->controller->terminate();
            }
            handle->controller = nullptr;
        }

        // 8. Terminate component
        if (handle->component) {
            handle->component->terminate();
            handle->component = nullptr;
        }

        // 9. Release the module last
        handle->module = nullptr;

        delete handle;
        fprintf(stderr, "vst3_gui_destroy: handle destroyed\n");
    } catch (const std::exception& e) {
        fprintf(stderr, "vst3_gui_destroy: C++ exception: %s\n", e.what());
        delete handle;
    } catch (...) {
        fprintf(stderr, "vst3_gui_destroy: unknown C++ exception\n");
        delete handle;
    }
}

void vst3_gui_show(Vst3GuiHandle* handle) {
    if (!handle || !handle->window) return;
    [handle->window makeKeyAndOrderFront:nil];
    fprintf(stderr, "vst3_gui_show: window shown\n");
}

int vst3_gui_is_open(Vst3GuiHandle* handle) {
    if (!handle || !handle->window) return 0;
    return [handle->window isVisible] ? 1 : 0;
}

int vst3_gui_get_size(Vst3GuiHandle* handle, float* width, float* height) {
    if (!handle || !handle->view) return -1;
    ViewRect rect = {};
    if (handle->view->getSize(&rect) != kResultOk) return -1;
    if (width) *width = static_cast<float>(rect.right - rect.left);
    if (height) *height = static_cast<float>(rect.bottom - rect.top);
    return 0;
}

int vst3_gui_get_state(Vst3GuiHandle* handle, unsigned char* data, int capacity) {
    if (!handle || !handle->component) return -1;

    try {
        // Use the same format as rack: [uint32 component_size][component state][controller state]
        MemoryStream stream;

        // Reserve space for component state size marker
        uint32_t size_placeholder = 0;
        stream.write(&size_placeholder, sizeof(size_placeholder), nullptr);

        int64 comp_start = 0;
        stream.tell(&comp_start);

        if (handle->component->getState(&stream) != kResultOk) {
            return -1;
        }

        int64 comp_end = 0;
        stream.tell(&comp_end);
        uint32_t comp_size = static_cast<uint32_t>(comp_end - comp_start);

        // Write component size at the beginning
        stream.seek(0, IBStream::kIBSeekSet, nullptr);
        stream.write(&comp_size, sizeof(comp_size), nullptr);
        stream.seek(comp_end, IBStream::kIBSeekSet, nullptr);

        // Get controller state if separate
        if (handle->controller && !handle->isSingleComponent) {
            handle->controller->getState(&stream);
        }

        int64 total_size = 0;
        stream.seek(0, IBStream::kIBSeekEnd, &total_size);
        stream.seek(0, IBStream::kIBSeekSet, nullptr);

        if (!data || capacity <= 0) {
            return (int)total_size;
        }

        int bytesToCopy = (int)total_size < capacity ? (int)total_size : capacity;
        int32 bytesRead = 0;
        stream.read(data, bytesToCopy, &bytesRead);
        return bytesRead;
    } catch (...) {
        fprintf(stderr, "vst3_gui_get_state: C++ exception\n");
        return -1;
    }
}

int vst3_gui_set_state(Vst3GuiHandle* handle, const unsigned char* data, int size) {
    if (!handle || !handle->component || !data || size <= (int)sizeof(uint32_t)) return -1;

    try {
        // Parse rack format: [uint32 component_size][component state][controller state]
        uint32_t comp_size = 0;
        memcpy(&comp_size, data, sizeof(comp_size));

        const unsigned char* comp_data = data + sizeof(uint32_t);
        int comp_data_len = (int)comp_size;
        if ((int)sizeof(uint32_t) + comp_data_len > size) {
            comp_data_len = size - (int)sizeof(uint32_t);
        }

        // Set component state
        MemoryStream compStream;
        compStream.write((void*)comp_data, comp_data_len, nullptr);
        compStream.seek(0, IBStream::kIBSeekSet, nullptr);
        tresult result = handle->component->setState(&compStream);
        if (result != kResultOk) {
            fprintf(stderr, "vst3_gui_set_state: component setState failed (%d)\n", result);
            return -1;
        }

        // Sync component state to controller (setComponentState, not setState)
        if (handle->controller) {
            compStream.seek(0, IBStream::kIBSeekSet, nullptr);
            handle->controller->setComponentState(&compStream);
        }

        // Set controller state if separate and data is available
        if (handle->controller && !handle->isSingleComponent) {
            int ctrl_offset = (int)sizeof(uint32_t) + (int)comp_size;
            if (ctrl_offset < size) {
                const unsigned char* ctrl_data = data + ctrl_offset;
                int ctrl_len = size - ctrl_offset;
                MemoryStream ctrlStream;
                ctrlStream.write((void*)ctrl_data, ctrl_len, nullptr);
                ctrlStream.seek(0, IBStream::kIBSeekSet, nullptr);
                handle->controller->setState(&ctrlStream);
            }
        }

        fprintf(stderr, "vst3_gui_set_state: restored %d bytes (comp=%u)\n", size, comp_size);
        return 0;
    } catch (...) {
        fprintf(stderr, "vst3_gui_set_state: C++ exception\n");
        return -1;
    }
}

int vst3_gui_get_parameter_count(Vst3GuiHandle* handle) {
    if (!handle || !handle->controller) return -1;
    return handle->controller->getParameterCount();
}

int vst3_gui_get_parameter(Vst3GuiHandle* handle, int index, double* value) {
    if (!handle || !handle->controller || !value) return -1;
    int count = handle->controller->getParameterCount();
    if (index < 0 || index >= count) return -1;

    ParameterInfo info;
    if (handle->controller->getParameterInfo(index, info) != kResultOk) return -1;
    *value = handle->controller->getParamNormalized(info.id);
    return 0;
}

int vst3_gui_set_parameter(Vst3GuiHandle* handle, int index, double value) {
    if (!handle || !handle->controller) return -1;
    int count = handle->controller->getParameterCount();
    if (index < 0 || index >= count) return -1;

    ParameterInfo info;
    if (handle->controller->getParameterInfo(index, info) != kResultOk) return -1;
    if (handle->controller->setParamNormalized(info.id, value) != kResultOk) return -1;

    // Also notify the component if separate (through connection proxy)
    // The controller→component sync happens via the connection if present
    return 0;
}

int vst3_gui_setup_processing(Vst3GuiHandle* handle, double sample_rate, int block_size) {
    if (!handle || !handle->component) return -1;

    try {
        // Query IAudioProcessor from component
        IAudioProcessor* processor = nullptr;
        if (handle->component->queryInterface(IAudioProcessor::iid, (void**)&processor) != kResultOk || !processor) {
            fprintf(stderr, "vst3_gui_setup_processing: no IAudioProcessor\n");
            return -1;
        }
        handle->processor = processor;

        // Setup processing
        ProcessSetup setup = {};
        setup.processMode = kRealtime;
        setup.symbolicSampleSize = kSample32;
        setup.sampleRate = sample_rate;
        setup.maxSamplesPerBlock = block_size;

        if (processor->setupProcessing(setup) != kResultOk) {
            fprintf(stderr, "vst3_gui_setup_processing: setupProcessing failed\n");
            handle->processor = nullptr;
            processor->release();
            return -1;
        }

        // Query bus arrangements to find channel counts
        SpeakerArrangement inputArr = 0;
        SpeakerArrangement outputArr = 0;

        // Count input channels (instruments may have 0)
        int32 numInputBuses = handle->component->getBusCount(kAudio, kInput);
        if (numInputBuses > 0) {
            processor->getBusArrangement(kInput, 0, inputArr);
            handle->component->activateBus(kAudio, kInput, 0, true);
        }
        handle->inputChannels = SpeakerArr::getChannelCount(inputArr);

        // Count output channels
        int32 numOutputBuses = handle->component->getBusCount(kAudio, kOutput);
        if (numOutputBuses > 0) {
            processor->getBusArrangement(kOutput, 0, outputArr);
            handle->component->activateBus(kAudio, kOutput, 0, true);
        }
        handle->outputChannels = SpeakerArr::getChannelCount(outputArr);

        // Activate the component and start processing
        handle->component->setActive(true);
        processor->setProcessing(true);
        handle->processingSetUp = true;

        fprintf(stderr, "vst3_gui_setup_processing: OK (in=%d, out=%d, sr=%.0f, bs=%d)\n",
                handle->inputChannels, handle->outputChannels, sample_rate, block_size);
        return 0;
    } catch (...) {
        fprintf(stderr, "vst3_gui_setup_processing: C++ exception\n");
        return -1;
    }
}

int vst3_gui_process(Vst3GuiHandle* handle,
                     const float* const* inputs, int num_input_channels,
                     float** outputs, int num_output_channels,
                     int num_frames) {
    if (!handle || !handle->processor || !handle->processingSetUp) return -1;

    // Build input AudioBusBuffers
    AudioBusBuffers inputBus = {};
    inputBus.numChannels = num_input_channels;
    inputBus.channelBuffers32 = const_cast<float**>(inputs);

    // Build output AudioBusBuffers
    AudioBusBuffers outputBus = {};
    outputBus.numChannels = num_output_channels;
    outputBus.channelBuffers32 = outputs;

    // Build event list from pending MIDI events
    EventListImpl eventList;
    {
        std::lock_guard<std::mutex> lock(handle->midiMutex);
        for (auto& ev : handle->pendingMidiEvents) {
            eventList.addEvent(ev);
        }
        handle->pendingMidiEvents.clear();
    }

    // Build ProcessData
    ProcessData data = {};
    data.processMode = kRealtime;
    data.symbolicSampleSize = kSample32;
    data.numSamples = num_frames;
    data.numInputs = (num_input_channels > 0) ? 1 : 0;
    data.numOutputs = (num_output_channels > 0) ? 1 : 0;
    data.inputs = (num_input_channels > 0) ? &inputBus : nullptr;
    data.outputs = (num_output_channels > 0) ? &outputBus : nullptr;
    data.inputEvents = &eventList;

    tresult result;
    try {
        result = handle->processor->process(data);
    } catch (...) {
        fprintf(stderr, "vst3_gui_process: C++ exception\n");
        return -1;
    }
    return (result == kResultOk) ? 0 : -1;
}

int vst3_gui_send_midi(Vst3GuiHandle* handle,
                       const unsigned char* midi_data, int num_events,
                       const int* sample_offsets) {
    if (!handle || !midi_data || num_events <= 0) return -1;

    std::lock_guard<std::mutex> lock(handle->midiMutex);

    for (int i = 0; i < num_events; i++) {
        const unsigned char* ev = midi_data + i * 3;
        unsigned char status = ev[0] & 0xF0;
        unsigned char channel = ev[0] & 0x0F;
        unsigned char data1 = ev[1];
        unsigned char data2 = ev[2];

        Event event = {};
        event.busIndex = 0;
        event.sampleOffset = sample_offsets ? sample_offsets[i] : 0;
        event.ppqPosition = 0;
        event.flags = Event::kIsLive;

        if (status == 0x90 && data2 > 0) {
            // Note On
            event.type = Event::kNoteOnEvent;
            event.noteOn.channel = (int16)channel;
            event.noteOn.pitch = (int16)data1;
            event.noteOn.velocity = (float)data2 / 127.0f;
            event.noteOn.length = 0;
            event.noteOn.tuning = 0.0f;
            event.noteOn.noteId = -1;
        } else if (status == 0x80 || (status == 0x90 && data2 == 0)) {
            // Note Off
            event.type = Event::kNoteOffEvent;
            event.noteOff.channel = (int16)channel;
            event.noteOff.pitch = (int16)data1;
            event.noteOff.velocity = (float)data2 / 127.0f;
            event.noteOff.tuning = 0.0f;
            event.noteOff.noteId = -1;
        } else {
            continue; // Skip unsupported MIDI messages for now
        }

        handle->pendingMidiEvents.push_back(event);
    }

    return 0;
}

int vst3_gui_get_audio_input_channels(Vst3GuiHandle* handle) {
    if (!handle) return 0;
    return handle->inputChannels;
}

int vst3_gui_get_audio_output_channels(Vst3GuiHandle* handle) {
    if (!handle) return 0;
    return handle->outputChannels;
}

} // extern "C"
