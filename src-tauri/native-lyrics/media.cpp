#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <tlhelp32.h>
#include <roapi.h>
#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Media.Control.h>
#include <winrt/Windows.Storage.Streams.h>
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <string>
#include <vector>

using namespace winrt::Windows::Media::Control;
using MediaCallback = void (*)(void*, const char*, const char*, std::int64_t, int);
using CoverCallback = void (*)(void*, const std::uint8_t*, std::size_t);

namespace {
GlobalSystemMediaTransportControlsSession find_qq_music_session() {
    auto manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().get();
    GlobalSystemMediaTransportControlsSession chosen{nullptr};
    for (auto const& session : manager.GetSessions()) {
        std::wstring id(session.SourceAppUserModelId());
        std::transform(id.begin(), id.end(), id.begin(), [](wchar_t c) {
            return c >= L'A' && c <= L'Z' ? static_cast<wchar_t>(c + (L'a' - L'A')) : c;
        });
        if (id.find(L"qqmusic") == std::wstring::npos) continue;
        chosen = session;
        if (session.GetPlaybackInfo().PlaybackStatus() == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing) break;
    }
    return chosen;
}
}

// Only the executable name and Windows session are checked; process contents are not read.
extern "C" int cf_process_running(const wchar_t* name) noexcept {
    if (!name || !*name) return -1;
    DWORD current_session = 0;
    if (!ProcessIdToSessionId(GetCurrentProcessId(), &current_session)) return -1;
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return -1;
    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    BOOL more = Process32FirstW(snapshot, &entry);
    bool found = false;
    bool unknown_session = false;
    while (more) {
        if (_wcsicmp(entry.szExeFile, name) == 0) {
            DWORD session = 0;
            if (!ProcessIdToSessionId(entry.th32ProcessID, &session)) {
                unknown_session = true;
            } else if (session == current_session) {
                found = true;
                break;
            }
        }
        more = Process32NextW(snapshot, &entry);
    }
    DWORD error = found ? ERROR_SUCCESS : GetLastError();
    CloseHandle(snapshot);
    return found ? 1 : !unknown_session && error == ERROR_NO_MORE_FILES ? 0 : -1;
}

extern "C" int cf_qq_music_snapshot(MediaCallback callback, void* context) noexcept {
    if (!callback || !context) return -1;
    const HRESULT initialized = RoInitialize(RO_INIT_MULTITHREADED);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) return -1;
    int result = 0;
    try {
        auto chosen = find_qq_music_session();
        if (chosen) {
            const auto playback = chosen.GetPlaybackInfo().PlaybackStatus();
            if (playback == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing ||
                playback == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused) {
                const auto properties = chosen.TryGetMediaPropertiesAsync().get();
                const auto timeline = chosen.GetTimelineProperties();
                auto title = winrt::to_string(properties.Title());
                auto artist = winrt::to_string(properties.Artist());
                if (title.size() <= 1024 && artist.size() <= 1024 && !title.empty()) {
                    auto position = std::chrono::duration_cast<std::chrono::milliseconds>(timeline.Position()).count();
                    const bool playing = playback == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
                    if (playing) {
                        const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(winrt::clock::now() - timeline.LastUpdatedTime()).count();
                        position += std::clamp<std::int64_t>(elapsed, 0, 1500);
                    }
                    callback(context, title.c_str(), artist.c_str(), std::max<std::int64_t>(0, position), playing ? 1 : 0);
                    result = 1;
                }
            }
        }
    } catch (...) { result = -1; }
    if (SUCCEEDED(initialized)) RoUninitialize();
    return result;
}

extern "C" int cf_qq_music_cover(CoverCallback callback, void* context) noexcept {
    if (!callback || !context) return -1;
    const HRESULT initialized = RoInitialize(RO_INIT_MULTITHREADED);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) return -1;
    int result = 0;
    try {
        auto chosen = find_qq_music_session();
        if (chosen) {
            const auto playback = chosen.GetPlaybackInfo().PlaybackStatus();
            if (playback == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing ||
                playback == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused) {
                const auto properties = chosen.TryGetMediaPropertiesAsync().get();
                const auto thumb = properties.Thumbnail();
                if (thumb) {
                    const auto stream = thumb.OpenReadAsync().get();
                    const auto size64 = stream.Size();
                    if (size64 >= 16 && size64 <= 2 * 1024 * 1024) {
                        const auto size = static_cast<std::uint32_t>(size64);
                        winrt::Windows::Storage::Streams::DataReader reader(stream.GetInputStreamAt(0));
                        const auto loaded = reader.LoadAsync(size).get();
                        if (loaded == size) {
                            std::vector<std::uint8_t> bytes(size);
                            reader.ReadBytes(bytes);
                            callback(context, bytes.data(), bytes.size());
                            result = 1;
                        }
                        reader.Close();
                    }
                    stream.Close();
                }
            }
        }
    } catch (...) { result = -1; }
    if (SUCCEEDED(initialized)) RoUninitialize();
    return result;
}
