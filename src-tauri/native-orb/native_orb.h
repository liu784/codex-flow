#pragma once

#include <cstdint>

extern "C" {

struct CfNativeOrbVisualState {
    std::int32_t percentage;
    std::uint32_t health;
    std::uint32_t visible;
    std::uint32_t collapsed;
};

struct CfNativeOrbEvent {
    std::uint32_t kind;
    std::int32_t x;
    std::int32_t y;
    std::int32_t code;
};

struct CfNativeOrbStats {
    double capture_fps;
    double render_fps;
    std::uint64_t capture_frames;
    std::uint64_t render_frames;
    std::uint32_t dpi;
};

struct CfNativeOrbDockPosition {
    std::int32_t x;
    std::int32_t y;
    std::uint32_t edge; // 0 free, 1 left, 2 right; x/y are the fully revealed anchor
};

void* cf_native_orb_start(
    std::int32_t x,
    std::int32_t y,
    std::uint32_t* error_stage,
    std::uint32_t* error_code);
void cf_native_orb_update(void* handle, CfNativeOrbVisualState state);
void cf_native_orb_set_position(void* handle, std::int32_t x, std::int32_t y);
void cf_native_orb_set_visible(void* handle, std::uint32_t visible);
void cf_native_orb_configure_dock(void* handle, std::uint32_t enabled, std::int32_t restore_edge);
void cf_native_orb_configure_options(void* handle, std::uint32_t options);
std::uint32_t cf_native_orb_get_dock_position(void* handle, CfNativeOrbDockPosition* position);
std::uint32_t cf_native_orb_get_position(void* handle, std::int32_t* x, std::int32_t* y);
std::uint32_t cf_native_orb_poll_event(void* handle, CfNativeOrbEvent* event);
void cf_native_orb_get_stats(void* handle, CfNativeOrbStats* stats);
void cf_native_orb_shutdown(void* handle);

}
