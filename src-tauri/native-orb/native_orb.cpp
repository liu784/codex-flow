#define NOMINMAX
#include "native_orb.h"
#include "edge_dock.h"

#include <windows.h>
#include <windowsx.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <dcomp.h>
#include <dxgi1_2.h>
#include <wincodec.h>
#include <wrl/client.h>
#include "native_orb_text.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

using Microsoft::WRL::ComPtr;
using Clock = std::chrono::steady_clock;

namespace {

constexpr int kLogicalWindowSize = 104;
constexpr int kLogicalOrbRadius = 40;
constexpr int kWcaExcludedFromDda = 24;
constexpr UINT kApplyStateMessage = WM_APP + 41;
constexpr UINT kShutdownMessage = WM_APP + 42;
constexpr UINT kConfigureDockMessage = WM_APP + 43;
constexpr UINT kOptionsMessage = WM_APP + 45;
constexpr UINT kPositionMessage = WM_APP + 44;
constexpr std::uint32_t kEventOpen = 1;
constexpr std::uint32_t kEventMoved = 2;
constexpr std::uint32_t kEventFailure = 3;
constexpr std::uint32_t kStageWindow = 1;
constexpr std::uint32_t kStageCaptureExclusion = 2;
constexpr std::uint32_t kStageDxgiCapture = 3;
constexpr std::uint32_t kStageD3d11 = 4;
constexpr std::uint32_t kStageShader = 5;
constexpr std::uint32_t kStageDirectComposition = 6;
constexpr auto kFrameInterval = std::chrono::microseconds(16667);

struct WindowCompositionAttributeData {
    int attribute;
    void* data;
    SIZE_T size;
};
using SetWindowCompositionAttributeFn = BOOL(WINAPI*)(HWND, WindowCompositionAttributeData*);

bool SetDdaCaptureExclusion(HWND window, DWORD* errorCode) {
    const HMODULE user32 = GetModuleHandleW(L"user32.dll");
    const auto setAttribute = reinterpret_cast<SetWindowCompositionAttributeFn>(
        GetProcAddress(user32, "SetWindowCompositionAttribute"));
    if (!setAttribute) {
        *errorCode = ERROR_PROC_NOT_FOUND;
        return false;
    }
    BOOL excluded = TRUE;
    WindowCompositionAttributeData data{kWcaExcludedFromDda, &excluded, sizeof(excluded)};
    SetLastError(ERROR_SUCCESS);
    const BOOL result = setAttribute(window, &data);
    *errorCode = result ? ERROR_SUCCESS : GetLastError();
    return result == TRUE;
}

constexpr const char* kShaderSource =
#include "native_orb_shader.inc"
;

constexpr BYTE kOrbShellPng[] = {
#include "native_orb_shell.inc"
};

struct FrameConstants {
    float captureSize[2];
    float windowOrigin[2];
    float monitorOrigin[2];
    float outputSize[2];
    float percentage;
    float health;
    float hasPercentage;
    float dpiScale;
    float elapsedSeconds;
    float padding[3];
};

static_assert(sizeof(FrameConstants) % 16 == 0);

struct VisualState {
    int percentage = -1;
    std::uint32_t health = 0;
    bool visible = false;
    bool collapsed = true;
    bool marquee = true;
};

class ComApartment {
public:
    ComApartment() : result_(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) {}
    ~ComApartment() {
        if (SUCCEEDED(result_)) CoUninitialize();
    }

    HRESULT Status() const {
        return result_ == RPC_E_CHANGED_MODE ? S_OK : result_;
    }

private:
    HRESULT result_;
};

HRESULT CreateOrbShellTexture(
    ID3D11Device* device,
    ID3D11ShaderResourceView** textureView) {
    if (!device || !textureView) return E_INVALIDARG;
    *textureView = nullptr;

    ComPtr<IWICImagingFactory> factory;
    HRESULT result = CoCreateInstance(
        CLSID_WICImagingFactory2, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
    if (FAILED(result)) {
        result = CoCreateInstance(
            CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
    }
    if (FAILED(result)) return result;

    ComPtr<IWICStream> stream;
    result = factory->CreateStream(&stream);
    if (FAILED(result)) return result;
    static_assert(sizeof(kOrbShellPng) <= MAXDWORD);
    result = stream->InitializeFromMemory(
        const_cast<BYTE*>(kOrbShellPng), static_cast<DWORD>(sizeof(kOrbShellPng)));
    if (FAILED(result)) return result;

    ComPtr<IWICBitmapDecoder> decoder;
    result = factory->CreateDecoderFromStream(
        stream.Get(), nullptr, WICDecodeMetadataCacheOnLoad, &decoder);
    if (FAILED(result)) return result;
    ComPtr<IWICBitmapFrameDecode> frame;
    result = decoder->GetFrame(0, &frame);
    if (FAILED(result)) return result;

    UINT width = 0;
    UINT height = 0;
    result = frame->GetSize(&width, &height);
    if (FAILED(result) || width == 0 || height == 0) return FAILED(result) ? result : E_FAIL;
    const std::uint64_t stride64 = static_cast<std::uint64_t>(width) * 4;
    const std::uint64_t size64 = stride64 * height;
    if (stride64 > MAXUINT || size64 > MAXUINT) return E_OUTOFMEMORY;

    ComPtr<IWICFormatConverter> converter;
    result = factory->CreateFormatConverter(&converter);
    if (FAILED(result)) return result;
    result = converter->Initialize(
        frame.Get(), GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone,
        nullptr, 0.0, WICBitmapPaletteTypeCustom);
    if (FAILED(result)) return result;

    std::vector<BYTE> pixels(static_cast<std::size_t>(size64));
    result = converter->CopyPixels(
        nullptr, static_cast<UINT>(stride64), static_cast<UINT>(size64), pixels.data());
    if (FAILED(result)) return result;

    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_IMMUTABLE;
    description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const D3D11_SUBRESOURCE_DATA initialData{
        pixels.data(), static_cast<UINT>(stride64), static_cast<UINT>(size64)};
    ComPtr<ID3D11Texture2D> texture;
    result = device->CreateTexture2D(&description, &initialData, &texture);
    if (FAILED(result)) return result;
    return device->CreateShaderResourceView(texture.Get(), nullptr, textureView);
}

class DesktopCapture {
public:
    HRESULT Initialize(HWND window, std::uint32_t* failureStage, HMONITOR dockMonitor = nullptr) {
        *failureStage = kStageDxgiCapture;
        monitor_ = dockMonitor ? dockMonitor : MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        ComPtr<IDXGIFactory1> factory;
        HRESULT result = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
        if (FAILED(result)) return result;

        ComPtr<IDXGIAdapter1> selectedAdapter;
        ComPtr<IDXGIOutput> selectedOutput;
        for (UINT adapterIndex = 0; !selectedOutput; ++adapterIndex) {
            ComPtr<IDXGIAdapter1> adapter;
            if (factory->EnumAdapters1(adapterIndex, &adapter) == DXGI_ERROR_NOT_FOUND) break;
            for (UINT outputIndex = 0;; ++outputIndex) {
                ComPtr<IDXGIOutput> output;
                if (adapter->EnumOutputs(outputIndex, &output) == DXGI_ERROR_NOT_FOUND) break;
                DXGI_OUTPUT_DESC description{};
                if (SUCCEEDED(output->GetDesc(&description)) && description.Monitor == monitor_) {
                    selectedAdapter = adapter;
                    selectedOutput = output;
                    outputDescription_ = description;
                    break;
                }
            }
        }
        if (!selectedAdapter || !selectedOutput) return DXGI_ERROR_NOT_FOUND;

        constexpr D3D_FEATURE_LEVEL levels[] = {
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
        };
        *failureStage = kStageD3d11;
        D3D_FEATURE_LEVEL selectedLevel{};
        result = D3D11CreateDevice(
            selectedAdapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
            &device_, &selectedLevel, &context_);
        if (FAILED(result)) return result;

        *failureStage = kStageDxgiCapture;
        ComPtr<IDXGIOutput1> output1;
        result = selectedOutput.As(&output1);
        if (FAILED(result)) return result;
        result = output1->DuplicateOutput(device_.Get(), &duplication_);
        if (FAILED(result)) return result;
        duplication_->GetDesc(&duplicationDescription_);

        *failureStage = kStageD3d11;
        D3D11_TEXTURE2D_DESC description{};
        description.Width = duplicationDescription_.ModeDesc.Width;
        description.Height = duplicationDescription_.ModeDesc.Height;
        description.MipLevels = 1;
        description.ArraySize = 1;
        description.Format = duplicationDescription_.ModeDesc.Format;
        description.SampleDesc.Count = 1;
        description.Usage = D3D11_USAGE_DEFAULT;
        description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        result = device_->CreateTexture2D(&description, nullptr, &texture_);
        if (FAILED(result)) return result;
        return device_->CreateShaderResourceView(texture_.Get(), nullptr, &view_);
    }

    HRESULT Acquire(UINT timeoutMilliseconds) {
        DXGI_OUTDUPL_FRAME_INFO frameInfo{};
        ComPtr<IDXGIResource> resource;
        const HRESULT result = duplication_->AcquireNextFrame(timeoutMilliseconds, &frameInfo, &resource);
        if (result == DXGI_ERROR_WAIT_TIMEOUT) return S_FALSE;
        if (FAILED(result)) return result;
        ComPtr<ID3D11Texture2D> frame;
        const HRESULT textureResult = resource.As(&frame);
        if (SUCCEEDED(textureResult)) {
            context_->CopyResource(texture_.Get(), frame.Get());
            ++frames_;
        }
        duplication_->ReleaseFrame();
        return textureResult;
    }

    bool IsCurrentMonitor(HWND window, HMONITOR dockMonitor = nullptr) const {
        return (dockMonitor ? dockMonitor : MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST)) == monitor_;
    }

    ID3D11Device* Device() const { return device_.Get(); }
    ID3D11DeviceContext* Context() const { return context_.Get(); }
    ID3D11ShaderResourceView* View() const { return view_.Get(); }
#ifdef CF_NATIVE_ORB_EVIDENCE
    ID3D11Texture2D* EvidenceTexture() const { return texture_.Get(); }
#endif
    const DXGI_OUTDUPL_DESC& Description() const { return duplicationDescription_; }
    const DXGI_OUTPUT_DESC& OutputDescription() const { return outputDescription_; }
    std::uint64_t Frames() const { return frames_; }

private:
    HMONITOR monitor_ = nullptr;
    ComPtr<ID3D11Device> device_;
    ComPtr<ID3D11DeviceContext> context_;
    ComPtr<IDXGIOutputDuplication> duplication_;
    ComPtr<ID3D11Texture2D> texture_;
    ComPtr<ID3D11ShaderResourceView> view_;
    DXGI_OUTDUPL_DESC duplicationDescription_{};
    DXGI_OUTPUT_DESC outputDescription_{};
    std::uint64_t frames_ = 0;
};

class OrbRenderer {
public:
    OrbRenderer() = default;
#ifdef CF_NATIVE_ORB_EVIDENCE
    // Finite native evidence may compare alternate visual models while retaining
    // the production capture, composition, and render path. Production always
    // uses kShaderSource because this constructor is not compiled there.
    explicit OrbRenderer(const char* evidenceShaderSource)
        : shaderSource_(evidenceShaderSource ? evidenceShaderSource : kShaderSource) {}
#endif

    HRESULT Initialize(HWND window, std::uint32_t* failureStage, HMONITOR dockMonitor = nullptr) {
        HRESULT result = apartment_.Status();
        if (FAILED(result)) return result;
        window_ = window;
        result = capture_.Initialize(window, failureStage, dockMonitor);
        if (FAILED(result)) return result;

        *failureStage = kStageD3d11;
        ComPtr<IDXGIDevice> dxgiDevice;
        result = capture_.Device()->QueryInterface(IID_PPV_ARGS(&dxgiDevice));
        if (FAILED(result)) return result;
        ComPtr<IDXGIAdapter> adapter;
        result = dxgiDevice->GetAdapter(&adapter);
        if (FAILED(result)) return result;
        ComPtr<IDXGIFactory2> factory;
        result = adapter->GetParent(IID_PPV_ARGS(&factory));
        if (FAILED(result)) return result;

        DXGI_SWAP_CHAIN_DESC1 swapDescription{};
        swapDescription.Width = kLogicalWindowSize;
        swapDescription.Height = kLogicalWindowSize;
        swapDescription.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        swapDescription.SampleDesc.Count = 1;
        swapDescription.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        swapDescription.BufferCount = 2;
        swapDescription.Scaling = DXGI_SCALING_STRETCH;
        swapDescription.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
        swapDescription.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
        result = factory->CreateSwapChainForComposition(
            capture_.Device(), &swapDescription, nullptr, &swapChain_);
        if (FAILED(result)) return result;

        D3D11_TEXTURE2D_DESC renderDescription{};
        renderDescription.Width = kLogicalWindowSize;
        renderDescription.Height = kLogicalWindowSize;
        renderDescription.MipLevels = 1;
        renderDescription.ArraySize = 1;
        renderDescription.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        renderDescription.SampleDesc.Count = 1;
        renderDescription.Usage = D3D11_USAGE_DEFAULT;
        renderDescription.BindFlags = D3D11_BIND_RENDER_TARGET;
        result = capture_.Device()->CreateTexture2D(&renderDescription, nullptr, &renderTexture_);
        if (FAILED(result)) return result;
        result = capture_.Device()->CreateRenderTargetView(renderTexture_.Get(), nullptr, &renderTarget_);
        if (FAILED(result)) return result;

        *failureStage = kStageShader;
        ComPtr<ID3DBlob> vertexBlob;
        ComPtr<ID3DBlob> pixelBlob;
        ComPtr<ID3DBlob> errors;
        result = D3DCompile(
            shaderSource_, std::strlen(shaderSource_), "native_orb_shader.hlsl", nullptr, nullptr,
            "VSMain", "vs_5_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &vertexBlob, &errors);
        if (FAILED(result)) return result;
        errors.Reset();
        result = D3DCompile(
            shaderSource_, std::strlen(shaderSource_), "native_orb_shader.hlsl", nullptr, nullptr,
            "PSMain", "ps_5_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &pixelBlob, &errors);
        if (FAILED(result)) return result;
        result = capture_.Device()->CreateVertexShader(
            vertexBlob->GetBufferPointer(), vertexBlob->GetBufferSize(), nullptr, &vertexShader_);
        if (FAILED(result)) return result;
        result = capture_.Device()->CreatePixelShader(
            pixelBlob->GetBufferPointer(), pixelBlob->GetBufferSize(), nullptr, &pixelShader_);
        if (FAILED(result)) return result;
        result = CreateOrbTextAtlas(capture_.Device(), &textAtlas_);
        if (FAILED(result)) return result;
        result = CreateOrbShellTexture(capture_.Device(), &shellTexture_);
        if (FAILED(result)) return result;

        *failureStage = kStageD3d11;
        D3D11_BUFFER_DESC constantDescription{};
        constantDescription.ByteWidth = sizeof(FrameConstants);
        constantDescription.Usage = D3D11_USAGE_DYNAMIC;
        constantDescription.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        constantDescription.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
        result = capture_.Device()->CreateBuffer(&constantDescription, nullptr, &constantBuffer_);
        if (FAILED(result)) return result;

        D3D11_SAMPLER_DESC samplerDescription{};
        samplerDescription.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
        samplerDescription.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
        samplerDescription.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
        samplerDescription.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
        samplerDescription.MaxLOD = D3D11_FLOAT32_MAX;
        result = capture_.Device()->CreateSamplerState(&samplerDescription, &sampler_);
        if (FAILED(result)) return result;

        *failureStage = kStageDirectComposition;
        result = DCompositionCreateDevice(dxgiDevice.Get(), IID_PPV_ARGS(&compositionDevice_));
        if (FAILED(result)) return result;
        result = compositionDevice_->CreateTargetForHwnd(window, TRUE, &compositionTarget_);
        if (FAILED(result)) return result;
        result = compositionDevice_->CreateVisual(&compositionVisual_);
        if (FAILED(result)) return result;
        result = compositionVisual_->SetContent(swapChain_.Get());
        if (FAILED(result)) return result;
        result = compositionTarget_->SetRoot(compositionVisual_.Get());
        if (FAILED(result)) return result;
        result = compositionDevice_->Commit();
        started_ = Clock::now();
        return result;
    }

    HRESULT Tick(const VisualState& visual, bool forceRender, HMONITOR dockMonitor = nullptr) {
        if (!capture_.IsCurrentMonitor(window_, dockMonitor)) return DXGI_ERROR_ACCESS_LOST;
        const HRESULT captureResult = capture_.Acquire(8);
        if (FAILED(captureResult)) return captureResult;
        if (captureResult == S_FALSE && !forceRender) return S_FALSE;
        return Render(visual);
    }

    std::uint64_t CaptureFrames() const { return capture_.Frames(); }
    std::uint64_t RenderFrames() const { return renderFrames_; }
#ifdef CF_NATIVE_ORB_EVIDENCE
    DesktopCapture& EvidenceCapture() { return capture_; }
    ID3D11Texture2D* EvidenceOutput() const { return renderTexture_.Get(); }
#endif
    double ElapsedSeconds() const {
        return std::chrono::duration<double>(Clock::now() - started_).count();
    }

private:
    const char* shaderSource_ = kShaderSource;

    HRESULT Render(const VisualState& visual) {
        RECT rectangle{};
        if (!GetWindowRect(window_, &rectangle)) return HRESULT_FROM_WIN32(GetLastError());
        const float dpiScale = static_cast<float>(GetDpiForWindow(window_)) / 96.0f;
        D3D11_MAPPED_SUBRESOURCE mapped{};
        HRESULT result = capture_.Context()->Map(
            constantBuffer_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped);
        if (FAILED(result)) return result;
        auto* constants = static_cast<FrameConstants*>(mapped.pData);
        constants->captureSize[0] = static_cast<float>(capture_.Description().ModeDesc.Width);
        constants->captureSize[1] = static_cast<float>(capture_.Description().ModeDesc.Height);
        constants->windowOrigin[0] = static_cast<float>(rectangle.left);
        constants->windowOrigin[1] = static_cast<float>(rectangle.top);
        constants->monitorOrigin[0] = static_cast<float>(capture_.OutputDescription().DesktopCoordinates.left);
        constants->monitorOrigin[1] = static_cast<float>(capture_.OutputDescription().DesktopCoordinates.top);
        constants->outputSize[0] = static_cast<float>(kLogicalWindowSize);
        constants->outputSize[1] = static_cast<float>(kLogicalWindowSize);
        constants->percentage = visual.percentage < 0 ? 0.0f : static_cast<float>(visual.percentage);
        constants->health = static_cast<float>(visual.health);
        constants->hasPercentage = visual.percentage < 0 ? 0.0f : 1.0f;
        constants->dpiScale = dpiScale;
        constants->elapsedSeconds = visual.marquee ? static_cast<float>(ElapsedSeconds()) : 0.0f;
        constants->padding[0] = 0.0f;
        constants->padding[1] = 0.0f;
        constants->padding[2] = 0.0f;
        capture_.Context()->Unmap(constantBuffer_.Get(), 0);

        constexpr float clear[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        capture_.Context()->ClearRenderTargetView(renderTarget_.Get(), clear);
        const D3D11_VIEWPORT viewport = {
            0.0f, 0.0f, static_cast<float>(kLogicalWindowSize),
            static_cast<float>(kLogicalWindowSize), 0.0f, 1.0f,
        };
        capture_.Context()->RSSetViewports(1, &viewport);
        ID3D11RenderTargetView* target = renderTarget_.Get();
        capture_.Context()->OMSetRenderTargets(1, &target, nullptr);
        capture_.Context()->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        capture_.Context()->VSSetShader(vertexShader_.Get(), nullptr, 0);
        capture_.Context()->PSSetShader(pixelShader_.Get(), nullptr, 0);
        ID3D11Buffer* constant = constantBuffer_.Get();
        capture_.Context()->PSSetConstantBuffers(0, 1, &constant);
        ID3D11ShaderResourceView* views[] = {
            capture_.View(), textAtlas_.Get(), shellTexture_.Get()};
        capture_.Context()->PSSetShaderResources(0, 3, views);
        ID3D11SamplerState* sampler = sampler_.Get();
        capture_.Context()->PSSetSamplers(0, 1, &sampler);
        capture_.Context()->Draw(3, 0);
        ID3D11ShaderResourceView* nullViews[] = {nullptr, nullptr, nullptr};
        capture_.Context()->PSSetShaderResources(0, 3, nullViews);

        ComPtr<ID3D11Texture2D> backBuffer;
        result = swapChain_->GetBuffer(0, IID_PPV_ARGS(&backBuffer));
        if (FAILED(result)) return result;
        capture_.Context()->CopyResource(backBuffer.Get(), renderTexture_.Get());
        // DirectComposition owns presentation timing; the outer loop enforces the 60 FPS ceiling.
        result = swapChain_->Present(0, 0);
        if (SUCCEEDED(result)) ++renderFrames_;
        return result;
    }

    HWND window_ = nullptr;
    ComApartment apartment_;
    DesktopCapture capture_;
    ComPtr<ID3D11Texture2D> renderTexture_;
    ComPtr<ID3D11RenderTargetView> renderTarget_;
    ComPtr<IDXGISwapChain1> swapChain_;
    ComPtr<ID3D11VertexShader> vertexShader_;
    ComPtr<ID3D11PixelShader> pixelShader_;
    ComPtr<ID3D11ShaderResourceView> textAtlas_;
    ComPtr<ID3D11ShaderResourceView> shellTexture_;
    ComPtr<ID3D11Buffer> constantBuffer_;
    ComPtr<ID3D11SamplerState> sampler_;
    ComPtr<IDCompositionDevice> compositionDevice_;
    ComPtr<IDCompositionTarget> compositionTarget_;
    ComPtr<IDCompositionVisual> compositionVisual_;
    Clock::time_point started_{};
    std::uint64_t renderFrames_ = 0;
};

struct Runtime {
    std::thread thread;
    std::mutex readyMutex;
    std::condition_variable readyCondition;
    bool ready = false;
    bool initialized = false;
    std::uint32_t initializationError = 0;
    std::uint32_t initializationStage = 0;
    HWND window = nullptr;
    std::mutex visualMutex;
    VisualState visual;
    std::uint32_t options = 15; // topmost, marquee, hover reveal, half-hide; optional reduced motion.
    std::atomic<bool> requestedVisible{true};
    std::atomic<bool> dirty{true};
    std::atomic<bool> stopping{false};
    std::atomic<std::int32_t> x{0};
    std::atomic<std::int32_t> y{0};
    std::mutex eventMutex;
    std::deque<CfNativeOrbEvent> events;
    std::atomic<std::uint64_t> captureFrames{0};
    std::atomic<std::uint64_t> renderFrames{0};
    std::atomic<double> elapsedSeconds{0.0};
    std::atomic<std::uint32_t> dpi{96};
    POINT dragStart{};
    POINT dragWindowStart{};
    bool pointerDown = false;
    bool dragging = false;
    edge_dock::Controller dock;
    HMONITOR dockMonitor = nullptr;
    bool wasVisible = false;
    std::mutex dockMutex;
    CfNativeOrbDockPosition dockPosition{};
};

void PushEvent(Runtime* runtime, std::uint32_t kind, std::int32_t x, std::int32_t y, std::int32_t code) {
    std::lock_guard lock(runtime->eventMutex);
    runtime->events.push_back(CfNativeOrbEvent{kind, x, y, code});
}

int PhysicalWindowSize(HWND window) {
    return MulDiv(kLogicalWindowSize, static_cast<int>(GetDpiForWindow(window)), 96);
}

double DockNow() {
    return std::chrono::duration<double, std::milli>(Clock::now().time_since_epoch()).count();
}

bool ReadDockMonitor(Runtime* runtime, bool retain, edge_dock::Geometry& geometry) {
    HMONITOR monitor = retain && runtime->dockMonitor ? runtime->dockMonitor
        : MonitorFromWindow(runtime->window, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    if (!monitor || !GetMonitorInfoW(monitor, &info)) return false;
    const double scale = GetDpiForWindow(runtime->window) / 96.0;
    geometry = {{static_cast<double>(info.rcMonitor.left), static_cast<double>(info.rcMonitor.top),
        static_cast<double>(info.rcMonitor.right), static_cast<double>(info.rcMonitor.bottom),
        static_cast<double>(info.rcWork.top), static_cast<double>(info.rcWork.bottom), scale}};
    if (!geometry.monitor.valid()) return false;
    runtime->dockMonitor = monitor;
    return true;
}

void PublishDockPosition(Runtime* runtime, bool notify) {
    const auto p = runtime->dock.saved();
    const auto edge = static_cast<std::uint32_t>(runtime->dock.edge);
    {
        std::lock_guard lock(runtime->dockMutex);
        runtime->dockPosition = {static_cast<int>(std::lround(p.x)), static_cast<int>(std::lround(p.y)), edge};
    }
    if (notify) PushEvent(runtime, kEventMoved, static_cast<int>(std::lround(p.x)),
        static_cast<int>(std::lround(p.y)), static_cast<int>(edge));
}

void PlaceDock(Runtime* runtime) {
    const int x = static_cast<int>(std::lround(runtime->dock.position.x));
    const int y = static_cast<int>(std::lround(runtime->dock.position.y));
    runtime->x.store(x); runtime->y.store(y);
    SetWindowPos(runtime->window, runtime->options & 1 ? HWND_TOPMOST : HWND_NOTOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    if (runtime->dock.edge != edge_dock::Edge::None) {
        // Clip against the selected monitor even when another display adjoins it.
        const auto& m = runtime->dock.geometry.monitor;
        const int size = PhysicalWindowSize(runtime->window);
        HRGN region = CreateRectRgn(std::max(0, static_cast<int>(m.left) - x), 0,
            std::min(size, static_cast<int>(m.right) - x), size);
        if (region && !SetWindowRgn(runtime->window, region, FALSE)) DeleteObject(region);
    } else SetWindowRgn(runtime->window, nullptr, FALSE);
    runtime->dirty.store(true);
    PublishDockPosition(runtime, false);
}

void RecoverOrbPosition(Runtime* runtime) {
    // Monitor removal/failure must never strand a hidden ball off-screen.
    runtime->dockMonitor = MonitorFromPoint(POINT{0, 0}, MONITOR_DEFAULTTOPRIMARY);
    edge_dock::Geometry geometry;
    if (!ReadDockMonitor(runtime, true, geometry)) {
        const double scale = std::max(1.0, GetDpiForWindow(runtime->window) / 96.0);
        geometry = {{0, 0, static_cast<double>(std::max(GetSystemMetrics(SM_CXSCREEN), 320)),
            static_cast<double>(std::max(GetSystemMetrics(SM_CYSCREEN), 320)), 0,
            static_cast<double>(std::max(GetSystemMetrics(SM_CYSCREEN), 320)), scale}};
    }
    runtime->dock.recover(geometry);
    PlaceDock(runtime);
    PublishDockPosition(runtime, true);
}

bool DockHovered(Runtime* runtime) {
    POINT cursor{};
    if (!GetCursorPos(&cursor)) return false;
    const auto& dock = runtime->dock;
    const auto inside = [&](edge_dock::Point p) {
        const double radius = kLogicalOrbRadius * GetDpiForWindow(runtime->window) / 96.0;
        const double center = PhysicalWindowSize(runtime->window) / 2.0;
        return std::hypot(cursor.x - p.x - center, cursor.y - p.y - center) <= radius;
    };
    if (inside(dock.position)) return true;
    // Preserve hover through the reveal movement instead of oscillating under a stationary pointer.
    return dock.edge != edge_dock::Edge::None
        && (dock.phase == edge_dock::Phase::Revealing || dock.phase == edge_dock::Phase::Full)
        && inside(dock.geometry.half(dock.edge, dock.position.y));
}

void ApplyRequestedState(Runtime* runtime) {
    VisualState visual;
    {
        std::lock_guard lock(runtime->visualMutex);
        visual = runtime->visual;
    }
    const bool visible = runtime->requestedVisible.load() && visual.visible && visual.collapsed;
    if (!visible && runtime->wasVisible) runtime->dock.suspend();
    if (visible && !runtime->wasVisible) {
        if (runtime->dock.edge != edge_dock::Edge::None) {
            edge_dock::Geometry geometry;
            if (ReadDockMonitor(runtime, true, geometry)) runtime->dock.geometry = geometry;
            else RecoverOrbPosition(runtime);
        }
        runtime->dock.resume(DockNow());
        PlaceDock(runtime);
    }
    runtime->wasVisible = visible;
    if (visible) {
        const int size = PhysicalWindowSize(runtime->window);
        SetWindowPos(runtime->window, HWND_TOPMOST, runtime->x.load(), runtime->y.load(), size, size,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
    } else {
        ShowWindow(runtime->window, SW_HIDE);
    }
    runtime->dirty.store(true);
}

LRESULT CALLBACK OrbWindowProcedure(HWND window, UINT message, WPARAM wParam, LPARAM lParam) {
    auto* runtime = reinterpret_cast<Runtime*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        auto* create = reinterpret_cast<CREATESTRUCTW*>(lParam);
        runtime = static_cast<Runtime*>(create->lpCreateParams);
        runtime->window = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(runtime));
    }
    if (!runtime) return DefWindowProcW(window, message, wParam, lParam);

    switch (message) {
        case WM_NCHITTEST: {
            POINT point{GET_X_LPARAM(lParam), GET_Y_LPARAM(lParam)};
            ScreenToClient(window, &point);
            RECT client{};
            GetClientRect(window, &client);
            const double dx = point.x - client.right / 2.0;
            const double dy = point.y - client.bottom / 2.0;
            const double radius = kLogicalOrbRadius * (static_cast<double>(GetDpiForWindow(window)) / 96.0);
            return dx * dx + dy * dy <= radius * radius ? HTCLIENT : HTTRANSPARENT;
        }
        case WM_LBUTTONDOWN: {
            runtime->dock.press();
            runtime->pointerDown = true;
            runtime->dragging = false;
            GetCursorPos(&runtime->dragStart);
            RECT rectangle{};
            GetWindowRect(window, &rectangle);
            runtime->dragWindowStart = POINT{rectangle.left, rectangle.top};
            SetCapture(window);
            return 0;
        }
        case WM_MOUSEMOVE: {
            if (!runtime->pointerDown || (wParam & MK_LBUTTON) == 0) return 0;
            POINT point{};
            GetCursorPos(&point);
            const int dx = point.x - runtime->dragStart.x;
            const int dy = point.y - runtime->dragStart.y;
            if (!runtime->dragging && std::abs(dx) + std::abs(dy) >= 5) {
                runtime->dragging = true;
                edge_dock::Geometry geometry;
                if (ReadDockMonitor(runtime, false, geometry)) {
                    runtime->dock.undock(geometry);
                    runtime->dragWindowStart = {static_cast<LONG>(std::lround(runtime->dock.position.x)),
                        static_cast<LONG>(std::lround(runtime->dock.position.y))};
                    SetWindowRgn(window, nullptr, FALSE);
                }
            }
            if (runtime->dragging) {
                const int nextX = runtime->dragWindowStart.x + dx;
                const int nextY = runtime->dragWindowStart.y + dy;
                runtime->x.store(nextX);
                runtime->y.store(nextY);
                runtime->dock.position = {static_cast<double>(nextX), static_cast<double>(nextY)};
                PublishDockPosition(runtime, false);
            SetWindowPos(window, runtime->options & 1 ? HWND_TOPMOST : HWND_NOTOPMOST, nextX, nextY, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                runtime->dirty.store(true);
            }
            return 0;
        }
        case WM_LBUTTONUP: {
            if (!runtime->pointerDown) return 0;
            runtime->pointerDown = false;
            ReleaseCapture();
            if (runtime->dragging) {
                runtime->dragging = false;
                edge_dock::Geometry geometry;
                if (ReadDockMonitor(runtime, false, geometry)) runtime->dock.release(geometry, DockNow());
                else RecoverOrbPosition(runtime);
                PublishDockPosition(runtime, true);
            } else {
                PushEvent(runtime, kEventOpen, runtime->x.load(), runtime->y.load(), 0);
            }
            return 0;
        }
        case WM_DPICHANGED: {
            const RECT* suggested = reinterpret_cast<const RECT*>(lParam);
            SetWindowPos(window, runtime->options & 1 ? HWND_TOPMOST : HWND_NOTOPMOST, suggested->left, suggested->top,
                         suggested->right - suggested->left, suggested->bottom - suggested->top,
                         SWP_NOACTIVATE);
            runtime->x.store(suggested->left);
            runtime->y.store(suggested->top);
            runtime->dpi.store(HIWORD(wParam));
            runtime->dock.position = {static_cast<double>(suggested->left), static_cast<double>(suggested->top)};
            if (!runtime->dragging && runtime->dock.edge != edge_dock::Edge::None) {
                edge_dock::Geometry geometry;
                if (ReadDockMonitor(runtime, true, geometry)) {
                    runtime->dock.restore(runtime->dock.edge, geometry, DockNow());
                    PlaceDock(runtime);
                } else RecoverOrbPosition(runtime);
            }
            runtime->dirty.store(true);
            return 0;
        }
        case WM_DISPLAYCHANGE:
            RecoverOrbPosition(runtime);
            return 0;
        case WM_SETTINGCHANGE: {
            BOOL animations = TRUE;
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &animations, 0);
            runtime->dock.reduced = animations == FALSE || (runtime->options & 16) != 0;
            if (runtime->dock.edge != edge_dock::Edge::None) {
                edge_dock::Geometry geometry;
                if (ReadDockMonitor(runtime, true, geometry)) {
                    runtime->dock.restore(runtime->dock.edge, geometry, DockNow());
                    PlaceDock(runtime);
                } else RecoverOrbPosition(runtime);
            }
            return 0;
        }
        case WM_CAPTURECHANGED:
        case WM_CANCELMODE:
            if (runtime->pointerDown) {
                runtime->pointerDown = false;
                runtime->dragging = false;
                RecoverOrbPosition(runtime);
            }
            return 0;
        case kPositionMessage:
            if (runtime->dock.edge == edge_dock::Edge::None) {
                runtime->dock.position = {static_cast<double>(static_cast<std::int32_t>(wParam)),
                    static_cast<double>(static_cast<std::int32_t>(lParam))};
                PlaceDock(runtime);
            }
            return 0;
        case kConfigureDockMessage: {
            const auto previousEdge = runtime->dock.edge;
            runtime->dock.configure(wParam != 0, DockNow());
            BOOL animations = TRUE;
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &animations, 0);
            runtime->dock.reduced = animations == FALSE || (runtime->options & 16) != 0;
            const int restore = static_cast<int>(lParam);
            if (wParam && restore > 0 && restore <= 2) {
                edge_dock::Geometry geometry;
                if (ReadDockMonitor(runtime, false, geometry))
                    runtime->dock.restore(static_cast<edge_dock::Edge>(restore), geometry, DockNow());
                else RecoverOrbPosition(runtime);
            }
            PlaceDock(runtime);
            PublishDockPosition(runtime, previousEdge != runtime->dock.edge || restore > 0);
            return 0;
        }
        case kApplyStateMessage:
            ApplyRequestedState(runtime);
            return 0;
        case kOptionsMessage: {
            runtime->options = static_cast<std::uint32_t>(wParam);
            runtime->dock.halfHide = (runtime->options & 8) != 0;
            runtime->dock.hoverReveal = (runtime->options & 4) != 0;
            BOOL animations = TRUE;
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &animations, 0);
            runtime->dock.reduced = animations == FALSE || (runtime->options & 16) != 0;
            runtime->dock.tick(DockNow(), DockHovered(runtime));
            PlaceDock(runtime);
            return 0;
        }
        case kShutdownMessage:
        case WM_CLOSE:
            DestroyWindow(window);
            return 0;
        case WM_DESTROY:
            PostQuitMessage(0);
            return 0;
        default:
            return DefWindowProcW(window, message, wParam, lParam);
    }
}

void SignalReady(Runtime* runtime) {
    std::lock_guard lock(runtime->readyMutex);
    runtime->ready = true;
    runtime->readyCondition.notify_one();
}

void RunRuntime(Runtime* runtime) {
    SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSEXW windowClass{sizeof(WNDCLASSEXW)};
    windowClass.lpfnWndProc = OrbWindowProcedure;
    windowClass.hInstance = instance;
    windowClass.hCursor = LoadCursorW(nullptr, IDC_SIZEALL);
    windowClass.lpszClassName = L"CodexFlowNativeOrbWindow";
    if (!RegisterClassExW(&windowClass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        runtime->initializationStage = kStageWindow;
        runtime->initializationError = GetLastError();
        SignalReady(runtime);
        return;
    }

    runtime->window = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
        windowClass.lpszClassName, L"Codex Flow Native Orb", WS_POPUP,
        runtime->x.load(), runtime->y.load(), kLogicalWindowSize, kLogicalWindowSize,
        nullptr, nullptr, instance, runtime);
    if (!runtime->window) {
        runtime->initializationStage = kStageWindow;
        runtime->initializationError = GetLastError();
        SignalReady(runtime);
        return;
    }

    // Keep normal screenshot APIs able to see the Orb. Global display-affinity
    // protection blacked it out in third-party captures. Only exclude this HWND
    // from Desktop Duplication, which is our refraction input, to prevent
    // sampling our own rendered Orb. Failure remains fail-closed below.
    DWORD ddaExclusionError = ERROR_SUCCESS;
    if (!SetDdaCaptureExclusion(runtime->window, &ddaExclusionError)) {
        runtime->initializationStage = kStageCaptureExclusion;
        runtime->initializationError = ddaExclusionError;
        DestroyWindow(runtime->window);
        SignalReady(runtime);
        return;
    }

    auto renderer = std::make_unique<OrbRenderer>();
    HRESULT result = renderer->Initialize(runtime->window, &runtime->initializationStage);
    if (FAILED(result)) {
        runtime->initializationError = static_cast<std::uint32_t>(result);
        DestroyWindow(runtime->window);
        SignalReady(runtime);
        return;
    }

    runtime->dpi.store(GetDpiForWindow(runtime->window));
    runtime->dock.position = {static_cast<double>(runtime->x.load()), static_cast<double>(runtime->y.load())};
    PublishDockPosition(runtime, false);
    runtime->initialized = true;
    SignalReady(runtime);

    MSG message{};
    auto nextFrame = Clock::now();
    while (!runtime->stopping.load()) {
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
            if (message.message == WM_QUIT) runtime->stopping.store(true);
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if (runtime->stopping.load()) break;

        VisualState visual;
        {
            std::lock_guard lock(runtime->visualMutex);
            visual = runtime->visual;
        }
        visual.marquee = (runtime->options & 2) != 0;
        const bool visible = runtime->requestedVisible.load() && visual.visible && visual.collapsed;
        if (!visible) {
            nextFrame = Clock::now();
            MsgWaitForMultipleObjectsEx(0, nullptr, 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            continue;
        }

        const auto now = Clock::now();
        if (now < nextFrame) {
            const auto delay = std::chrono::duration_cast<std::chrono::milliseconds>(nextFrame - now);
            const DWORD waitMilliseconds = static_cast<DWORD>(std::max<std::int64_t>(1, delay.count()));
            MsgWaitForMultipleObjectsEx(0, nullptr, waitMilliseconds, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            continue;
        }
        nextFrame = now + kFrameInterval;

        const auto beforeDock = runtime->dock.position;
        runtime->dock.tick(DockNow(), DockHovered(runtime));
        if (runtime->dock.position.x != beforeDock.x || runtime->dock.position.y != beforeDock.y)
            PlaceDock(runtime);

        const bool animate = visual.percentage > 0 && visual.marquee;
        const bool force = runtime->dirty.exchange(false) || animate;
        const auto captureMonitor = runtime->dock.edge == edge_dock::Edge::None ? nullptr : runtime->dockMonitor;
        result = renderer->Tick(visual, force, captureMonitor);
        if (result == DXGI_ERROR_ACCESS_LOST || result == DXGI_ERROR_DEVICE_REMOVED
            || result == DXGI_ERROR_DEVICE_RESET || result == DXGI_ERROR_NOT_CURRENTLY_AVAILABLE) {
            renderer = std::make_unique<OrbRenderer>();
            result = renderer->Initialize(runtime->window, &runtime->initializationStage, captureMonitor);
            if (FAILED(result)) {
                PushEvent(runtime, kEventFailure, runtime->x.load(), runtime->y.load(), static_cast<std::int32_t>(result));
                ShowWindow(runtime->window, SW_HIDE);
                runtime->requestedVisible.store(false);
                break;
            }
            runtime->dirty.store(true);
            continue;
        }
        if (FAILED(result)) {
            PushEvent(runtime, kEventFailure, runtime->x.load(), runtime->y.load(), static_cast<std::int32_t>(result));
            ShowWindow(runtime->window, SW_HIDE);
            runtime->requestedVisible.store(false);
            break;
        }
        runtime->captureFrames.store(renderer->CaptureFrames());
        runtime->renderFrames.store(renderer->RenderFrames());
        runtime->elapsedSeconds.store(renderer->ElapsedSeconds());
    }

    if (runtime->window && IsWindow(runtime->window)) DestroyWindow(runtime->window);
    runtime->window = nullptr;
}

} // namespace

extern "C" void* cf_native_orb_start(
    std::int32_t x,
    std::int32_t y,
    std::uint32_t* errorStage,
    std::uint32_t* errorCode) {
    auto runtime = std::make_unique<Runtime>();
    runtime->x.store(x);
    runtime->y.store(y);
    Runtime* raw = runtime.get();
    runtime->thread = std::thread([raw] { RunRuntime(raw); });
    {
        std::unique_lock lock(runtime->readyMutex);
        runtime->readyCondition.wait(lock, [raw] { return raw->ready; });
    }
    if (!runtime->initialized) {
        if (errorStage) *errorStage = runtime->initializationStage;
        if (errorCode) *errorCode = runtime->initializationError;
        if (runtime->thread.joinable()) runtime->thread.join();
        return nullptr;
    }
    if (errorStage) *errorStage = 0;
    if (errorCode) *errorCode = 0;
    return runtime.release();
}

extern "C" void cf_native_orb_update(void* handle, CfNativeOrbVisualState state) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime) return;
    {
        std::lock_guard lock(runtime->visualMutex);
        runtime->visual.percentage = std::clamp(state.percentage, -1, 100);
        runtime->visual.health = std::min<std::uint32_t>(state.health, 3);
        runtime->visual.visible = state.visible != 0;
        runtime->visual.collapsed = state.collapsed != 0;
    }
    runtime->dirty.store(true);
    if (runtime->window) PostMessageW(runtime->window, kApplyStateMessage, 0, 0);
}

extern "C" void cf_native_orb_set_position(void* handle, std::int32_t x, std::int32_t y) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime) return;
    if (runtime->window) PostMessageW(runtime->window, kPositionMessage, static_cast<WPARAM>(x), static_cast<LPARAM>(y));
}

extern "C" void cf_native_orb_configure_dock(void* handle, std::uint32_t enabled, std::int32_t restore_edge) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (runtime && runtime->window) PostMessageW(runtime->window, kConfigureDockMessage, enabled, restore_edge);
}

extern "C" void cf_native_orb_configure_options(void* handle, std::uint32_t options) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (runtime && runtime->window) PostMessageW(runtime->window, kOptionsMessage, options, 0);
}

extern "C" std::uint32_t cf_native_orb_get_dock_position(void* handle, CfNativeOrbDockPosition* position) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime || !position) return 0;
    std::lock_guard lock(runtime->dockMutex);
    *position = runtime->dockPosition;
    return 1;
}

extern "C" void cf_native_orb_set_visible(void* handle, std::uint32_t visible) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime) return;
    runtime->requestedVisible.store(visible != 0);
    if (runtime->window) PostMessageW(runtime->window, kApplyStateMessage, 0, 0);
}

extern "C" std::uint32_t cf_native_orb_get_position(void* handle, std::int32_t* x, std::int32_t* y) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime || !x || !y) return 0;
    *x = runtime->x.load();
    *y = runtime->y.load();
    return 1;
}

extern "C" std::uint32_t cf_native_orb_poll_event(void* handle, CfNativeOrbEvent* event) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime || !event) return 0;
    std::lock_guard lock(runtime->eventMutex);
    if (runtime->events.empty()) return 0;
    *event = runtime->events.front();
    runtime->events.pop_front();
    return 1;
}

extern "C" void cf_native_orb_get_stats(void* handle, CfNativeOrbStats* stats) {
    auto* runtime = static_cast<Runtime*>(handle);
    if (!runtime || !stats) return;
    const double elapsed = runtime->elapsedSeconds.load();
    stats->capture_frames = runtime->captureFrames.load();
    stats->render_frames = runtime->renderFrames.load();
    stats->capture_fps = elapsed > 0.0 ? stats->capture_frames / elapsed : 0.0;
    stats->render_fps = elapsed > 0.0 ? stats->render_frames / elapsed : 0.0;
    stats->dpi = runtime->dpi.load();
}

extern "C" void cf_native_orb_shutdown(void* handle) {
    std::unique_ptr<Runtime> runtime(static_cast<Runtime*>(handle));
    if (!runtime) return;
    runtime->stopping.store(true);
    if (runtime->window) PostMessageW(runtime->window, kShutdownMessage, 0, 0);
    if (runtime->thread.joinable()) runtime->thread.join();
}
