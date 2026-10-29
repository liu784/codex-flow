// Expanded panel background only. Windows SDK C++/WinRT; no pixel capture,
// input handling, quota state, or Orb dependencies.
#define NOMINMAX
#include <windows.h>
#include <commctrl.h>
#include <DispatcherQueue.h>
#include <windows.ui.composition.interop.h>
#include <winrt/Windows.Foundation.Numerics.h>
#include <winrt/Windows.System.h>
#include <winrt/Windows.UI.h>
#include <winrt/Windows.UI.Composition.h>
#include <winrt/Windows.UI.Composition.Desktop.h>
#include <memory>

namespace {
using namespace winrt;
using namespace Windows::UI::Composition;
using Windows::UI::Composition::Desktop::DesktopWindowTarget;
using Windows::System::DispatcherQueue;
using Windows::System::DispatcherQueueController;
constexpr wchar_t kPanelProperty[] = L"CodexFlow.PanelBackdrop.v1";
constexpr UINT_PTR kPanelSubclass = 0x43465042;
thread_local DispatcherQueueController ownedQueue{nullptr};
thread_local Windows::Foundation::IAsyncAction queueShutdown{nullptr};
thread_local bool backdropStopping = false;

HRESULT SetHostBackdrop(HWND window, bool enabled) noexcept {
    struct AccentPolicy { DWORD state, flags, color, animation; };
    struct AttributeData { DWORD attribute; void* data; SIZE_T size; };
    using SetAttribute = BOOL(WINAPI*)(HWND, AttributeData*);
    const auto set = reinterpret_cast<SetAttribute>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "SetWindowCompositionAttribute"));
    if (!set) return HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND);
    // Windows 10 host-backdrop support, NOT whole-window Acrylic (state 4).
    // The actual visible glass is confined to the composition visual below.
    AccentPolicy policy{enabled ? 5u : 0u, 0, 0, 0};
    AttributeData data{19, &policy, sizeof(policy)};
    SetLastError(ERROR_SUCCESS);
    if (set(window, &data)) return S_OK;
    const DWORD error = GetLastError();
    return error ? HRESULT_FROM_WIN32(error) : E_FAIL;
}

void EnsureDispatcherQueue() {
    // One queue per owning UI thread, reused across expansion/collapse. Do not
    // replace an existing Tauri/Windows queue or create a render/polling thread.
    if (backdropStopping) throw_hresult(RO_E_CLOSED);
    if (DispatcherQueue::GetForCurrentThread()) return;
    DispatcherQueueOptions options{sizeof(options), DQTYPE_THREAD_CURRENT, DQTAT_COM_NONE};
    check_hresult(CreateDispatcherQueueController(options,
        reinterpret_cast<ABI::Windows::System::IDispatcherQueueController**>(put_abi(ownedQueue))));
}

struct PanelBackdrop {
    Compositor compositor{nullptr};
    DesktopWindowTarget target{nullptr};
    ContainerVisual root{nullptr};
    CompositionRoundedRectangleGeometry geometry{nullptr};
    SpriteVisual glass{nullptr};
    SpriteVisual tint{nullptr};

    void Initialize(HWND window, const int* bounds) {
        EnsureDispatcherQueue();
        compositor = Compositor();
        const auto interop = compositor.as<ABI::Windows::UI::Composition::Desktop::ICompositorDesktopInterop>();
        // Below child HWNDs: WebView continues to own all panel text, character,
        // controls, accessibility, focus and pointer routing.
        check_hresult(interop->CreateDesktopWindowTarget(window, false,
            reinterpret_cast<ABI::Windows::UI::Composition::Desktop::IDesktopWindowTarget**>(put_abi(target))));
        root = compositor.CreateContainerVisual();
        geometry = compositor.CreateRoundedRectangleGeometry();
        root.Clip(compositor.CreateGeometricClip(geometry));
        glass = compositor.CreateSpriteVisual();
        glass.Brush(compositor.CreateHostBackdropBrush());
        tint = compositor.CreateSpriteVisual();
        // Preserve the previous cool-white native tint; CSS surface optics stay
        // untouched. Both sprites are inside ONE antialiased rounded clip.
        tint.Brush(compositor.CreateColorBrush({42, 241, 246, 250}));
        root.Children().InsertAtBottom(glass);
        root.Children().InsertAtTop(tint);
        Update(bounds);
        target.Root(root);
    }

    void Update(const int* bounds) {
        const Windows::Foundation::Numerics::float2 size{
            static_cast<float>(bounds[2] - bounds[0]),
            static_cast<float>(bounds[3] - bounds[1])};
        const float radius = static_cast<float>(bounds[4]) / 2.0f;
        root.Offset({static_cast<float>(bounds[0]), static_cast<float>(bounds[1]), 0});
        root.Size(size);
        geometry.Size(size);
        geometry.CornerRadius({radius, radius});
        glass.Size(size);
        tint.Size(size);
    }

    ~PanelBackdrop() noexcept {
        try {
            if (target) { target.Root(nullptr); target.Close(); }
        } catch (...) { /* Window teardown must not throw across Win32/Rust. */ }
    }
};

LRESULT CALLBACK PanelLifetime(HWND window, UINT message, WPARAM wparam,
    LPARAM lparam, UINT_PTR id, DWORD_PTR data);

void Detach(HWND window) noexcept {
    // Remove ownership first: Close can synchronously pump window messages.
    auto* state = reinterpret_cast<PanelBackdrop*>(RemovePropW(window, kPanelProperty));
    RemoveWindowSubclass(window, PanelLifetime, kPanelSubclass);
    delete state;
}

LRESULT CALLBACK PanelLifetime(HWND window, UINT message, WPARAM wparam,
    LPARAM lparam, UINT_PTR, DWORD_PTR) {
    if (message == WM_NCDESTROY) {
        Detach(window);
        SetHostBackdrop(window, false);
    }
    return DefSubclassProc(window, message, wparam, lparam);
}
}

extern "C" int cf_panel_backdrop_set(void* handle, int enabled, const int* bounds) noexcept {
    const auto window = static_cast<HWND>(handle);
    if (!IsWindow(window)) return E_INVALIDARG;
    if (GetWindowThreadProcessId(window, nullptr) != GetCurrentThreadId()) return RPC_E_WRONG_THREAD;
    if (!enabled) {
        Detach(window);
        return SetHostBackdrop(window, false);
    }
    if (!bounds || bounds[0] < 0 || bounds[1] < 0 || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1] || bounds[4] <= 0) return E_INVALIDARG;
    try {
        if (auto* state = reinterpret_cast<PanelBackdrop*>(GetPropW(window, kPanelProperty))) {
            state->Update(bounds);
            return S_OK;
        }
        check_hresult(SetHostBackdrop(window, true));
        auto state = std::make_unique<PanelBackdrop>();
        state->Initialize(window, bounds);
        if (!SetPropW(window, kPanelProperty, state.get())) throw_hresult(E_FAIL);
        if (!SetWindowSubclass(window, PanelLifetime, kPanelSubclass, 0)) {
            RemovePropW(window, kPanelProperty);
            throw_hresult(E_FAIL);
        }
        state.release();
        return S_OK;
    } catch (...) {
        const HRESULT error = to_hresult();
        Detach(window);
        SetHostBackdrop(window, false);
        return error;
    }
}

// Called on the owning UI thread before Tauri ends its event loop. S_FALSE means
// pending: the caller must keep pumping normal UI messages, not block this thread.
extern "C" int cf_panel_backdrop_shutdown(void* handle) noexcept {
    const auto window = static_cast<HWND>(handle);
    if (window && (!IsWindow(window)
        || GetWindowThreadProcessId(window, nullptr) != GetCurrentThreadId())) return RPC_E_WRONG_THREAD;
    try {
        if (!backdropStopping) {
            backdropStopping = true;
            if (window) { Detach(window); SetHostBackdrop(window, false); }
        }
        // Never shut down a queue borrowed from Tauri/another component.
        if (!ownedQueue) return S_OK;
        if (!queueShutdown) queueShutdown = ownedQueue.ShutdownQueueAsync();
        if (queueShutdown.Status() == Windows::Foundation::AsyncStatus::Started) return S_FALSE;
        queueShutdown.GetResults();
        queueShutdown = nullptr;
        ownedQueue = nullptr;
        return S_OK;
    } catch (...) { return to_hresult(); }
}
