//! Expanded-panel-only, rounded Windows Composition host backdrop.
//! No desktop capture or Orb changes. Native lifetime stays on the UI thread.
use tauri::WebviewWindow;

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    extern "C" {
        fn cf_panel_backdrop_set(window: *mut c_void, enabled: i32, bounds: *const i32) -> i32;
        fn cf_panel_backdrop_shutdown(window: *mut c_void) -> i32;
    }

    pub(super) fn shutdown(hwnd: usize) -> Result<bool, String> {
        // UI-thread-only native queue lifecycle; true means shutdown completed.
        let result = unsafe { cf_panel_backdrop_shutdown(hwnd as *mut c_void) };
        match result {
            0 => Ok(true),
            1 => Ok(false),
            error => Err(format!(
                "PANEL_QUEUE_SHUTDOWN_FAILED:0x{:08X}",
                error as u32
            )),
        }
    }
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(version: *mut Version) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn CreateRoundRectRgn(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowRgn(window: *mut c_void, region: *mut c_void, redraw: i32) -> i32;
        fn GetWindowLongW(window: *mut c_void, index: i32) -> i32;
        fn SetWindowLongW(window: *mut c_void, index: i32, value: i32) -> i32;
        fn SetWindowPos(
            window: *mut c_void,
            after: *mut c_void,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            flags: u32,
        ) -> i32;
    }

    type SubclassProc =
        unsafe extern "system" fn(*mut c_void, u32, usize, isize, usize, usize) -> isize;
    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(
            window: *mut c_void,
            proc: SubclassProc,
            id: usize,
            data: usize,
        ) -> i32;
        fn GetWindowSubclass(
            window: *mut c_void,
            proc: SubclassProc,
            id: usize,
            data: *mut usize,
        ) -> i32;
        fn RemoveWindowSubclass(window: *mut c_void, proc: SubclassProc, id: usize) -> i32;
        fn DefSubclassProc(
            window: *mut c_void,
            message: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize;
    }
    const PANEL_FRAME_ID: usize = 0x43465046;
    const WS_CAPTION: i32 = 0x00c00000;
    const GWL_STYLE: i32 = -16;

    #[repr(C)]
    struct StyleChange {
        old: u32,
        new: u32,
    }

    unsafe extern "system" fn panel_frame_proc(
        window: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        id: usize,
        _data: usize,
    ) -> isize {
        // Panel owns the complete client surface. Tao handles only the TRUE
        // WM_NCCALCSIZE form; the FALSE form otherwise reaches DefWindowProc.
        // Suppress non-client paint/activation as well, so transparent WebView
        // surfaces cannot expose a ghost caption on focus changes (tauri #14764).
        // No input, sizing, visibility, or focus messages are intercepted.
        match message {
            0x0083 | 0x0085 => return 0, // WM_NCCALCSIZE / WM_NCPAINT
            0x0086 => return 1,          // WM_NCACTIVATE: no native frame to paint
            _ => {}
        }
        // Tao keeps WS_CAPTION even for undecorated top-level windows and can
        // reapply it on show/focus. Filter only that bit while Panel is expanded.
        let result = DefSubclassProc(window, message, wparam, lparam);
        if message == 0x007c && wparam as i32 == GWL_STYLE && lparam != 0 {
            (*(lparam as *mut StyleChange)).new &= !(WS_CAPTION as u32);
        }
        if message == 0x0082 {
            RemoveWindowSubclass(window, panel_frame_proc, id);
        }
        result
    }

    unsafe fn panel_frame(window: *mut c_void, enabled: bool) -> Result<(), String> {
        let current = GetWindowLongW(window, GWL_STYLE);
        let mut original_caption = 0;
        let installed = GetWindowSubclass(
            window,
            panel_frame_proc,
            PANEL_FRAME_ID,
            &mut original_caption,
        ) != 0;
        if enabled && !installed {
            original_caption = (current & WS_CAPTION) as usize;
            if SetWindowSubclass(window, panel_frame_proc, PANEL_FRAME_ID, original_caption) == 0 {
                return Err("PANEL_FRAME_INSTALL_FAILED".into());
            }
        } else if !enabled {
            if !installed {
                return Ok(());
            }
            if RemoveWindowSubclass(window, panel_frame_proc, PANEL_FRAME_ID) == 0 {
                return Err("PANEL_FRAME_CLEAR_FAILED".into());
            }
        }
        let original_caption = original_caption as i32 & WS_CAPTION;
        let desired = (current & !WS_CAPTION) | if enabled { 0 } else { original_caption };
        SetWindowLongW(window, GWL_STYLE, desired);
        // Recalculate the frame without moving, resizing, activating or reordering.
        let refreshed = SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0, 0x0037);
        if GetWindowLongW(window, GWL_STYLE) != desired || refreshed == 0 {
            if enabled {
                RemoveWindowSubclass(window, panel_frame_proc, PANEL_FRAME_ID);
                SetWindowLongW(
                    window,
                    GWL_STYLE,
                    (current & !WS_CAPTION) | original_caption,
                );
                SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0, 0x0037);
            }
            return Err("PANEL_FRAME_UPDATE_FAILED".into());
        }
        Ok(())
    }

    // Match the existing 8 DIP shell inset and 32 DIP panel radius.
    pub(super) fn clip_bounds(width: u32, height: u32, scale: f64) -> [i32; 5] {
        let inset = (8.0 * scale).round() as i32;
        [
            inset,
            inset,
            width as i32 - inset,
            height as i32 - inset,
            (64.0 * scale).round() as i32,
        ]
    }

    pub(super) fn configure(hwnd: usize, enabled: bool, bounds: [i32; 5]) -> Result<bool, String> {
        // SAFETY: ABI-compatible Win32 structs, live main-window HWND, and
        // synchronous calls on its owning thread. Successful SetWindowRgn
        // transfers region ownership to Windows; otherwise we free it here.
        unsafe {
            let mut version: Version = std::mem::zeroed();
            version.size = std::mem::size_of::<Version>() as u32;
            if RtlGetVersion(&mut version) != 0 || version.major < 10 || version.build < 17763 {
                return Ok(false);
            }
            let hwnd = hwnd as *mut c_void;
            if !enabled {
                // Release the composition target before the caller resizes to Orb.
                let cleared = cf_panel_backdrop_set(hwnd, 0, std::ptr::null());
                let frame = panel_frame(hwnd, false);
                let region = SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
                frame?;
                if cleared < 0 {
                    return Err(format!(
                        "PANEL_BACKDROP_CLEAR_FAILED:0x{:08X}",
                        cleared as u32
                    ));
                }
                if region == 0 {
                    return Err("PANEL_REGION_CLEAR_FAILED".into());
                }
                return Ok(false);
            }
            panel_frame(hwnd, true)?;
            let [l, t, r, b, diameter] = bounds;
            let backdrop_bounds = [
                l + 1,
                t + 1,
                (r - 1).max(l + 2),
                (b - 1).max(t + 2),
                (diameter - 2).max(2),
            ];
            let attached = cf_panel_backdrop_set(hwnd, 1, backdrop_bounds.as_ptr());
            if attached < 0 {
                panel_frame(hwnd, false)?;
                return Err(format!(
                    "PANEL_BACKDROP_ATTACH_FAILED:0x{:08X}",
                    attached as u32
                ));
            }
            // Keep the GDI hit-test region 6px outside the visible rounded border
            // so GDI's 1-bit non-antialiased staircase never clips the smooth CSS/Composition edge.
            let region = CreateRoundRectRgn(
                (l - 6).max(0),
                (t - 6).max(0),
                r + 6,
                b + 6,
                diameter + 12,
                diameter + 12,
            );
            if !region.is_null() && SetWindowRgn(hwnd, region, 1) != 0 {
                return Ok(true);
            }
            if !region.is_null() {
                DeleteObject(region);
            }
            let cleanup = configure(hwnd as usize, false, bounds);
            cleanup?;
            Err("PANEL_REGION_INSTALL_FAILED".into())
        }
    }

    #[test]
    fn clip_tracks_panel_inset_and_radius_at_multiple_dpi_scales() {
        assert_eq!(clip_bounds(760, 470, 1.0), [8, 8, 752, 462, 64]);
        assert_eq!(clip_bounds(1140, 705, 1.5), [12, 12, 1128, 693, 96]);
        assert_eq!(clip_bounds(1520, 940, 2.0), [16, 16, 1504, 924, 128]);
    }

    #[test]
    #[ignore = "Requires Windows 10 1809+ with a desktop compositor"]
    fn native_clipped_backdrop_apply_and_clear_on_owned_hidden_window() {
        #[link(name = "runtimeobject")]
        extern "system" {
            fn RoInitialize(mode: u32) -> i32;
            fn RoUninitialize();
        }
        const GWL_EXSTYLE: i32 = -20;
        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowExW(
                ex: u32,
                class: *const u16,
                title: *const u16,
                style: u32,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                parent: *mut c_void,
                menu: *mut c_void,
                instance: *mut c_void,
                parameter: *mut c_void,
            ) -> *mut c_void;
            fn DestroyWindow(hwnd: *mut c_void) -> i32;
            fn GetWindowRgn(hwnd: *mut c_void, region: *mut c_void) -> i32;
            fn SendMessageW(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize)
                -> isize;
        }
        // Only a test-owned, invisible window is touched; cleanup precedes assertions.
        unsafe {
            assert!(RoInitialize(0) >= 0, "test-owned STA initialization");
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(
                0x80, // Keep Tao's default redirection-bitmap path.
                class.as_ptr(),
                [0u16].as_ptr(),
                0x80000000 | WS_CAPTION as u32,
                0,
                0,
                760,
                470,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(!hwnd.is_null(), "test window creation");
            let original_style = GetWindowLongW(hwnd, GWL_STYLE);
            let original_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let region = CreateRoundRectRgn(0, 0, 1, 1, 1, 1);
            let active = configure(hwnd as usize, true, clip_bounds(760, 470, 1.0));
            let active_style = GetWindowLongW(hwnd, GWL_STYLE);
            let active_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            // Simulate Tao reapplying caption flags during show/focus transitions.
            SetWindowLongW(hwnd, GWL_STYLE, original_style);
            let reapplied_style = GetWindowLongW(hwnd, GWL_STYLE);
            let clipped = GetWindowRgn(hwnd, region);
            let mut proposed_client = [0i32, 0, 760, 470];
            let calc = SendMessageW(hwnd, 0x0083, 0, proposed_client.as_mut_ptr() as isize);
            let nc_active = SendMessageW(hwnd, 0x0086, 1, 0);
            let cleared = configure(hwnd as usize, false, clip_bounds(760, 470, 1.0));
            let restored_style = GetWindowLongW(hwnd, GWL_STYLE);
            let restored_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let mut data = 0;
            let frame_removed =
                GetWindowSubclass(hwnd, panel_frame_proc, PANEL_FRAME_ID, &mut data) == 0;
            let unclipped = GetWindowRgn(hwnd, region);
            DeleteObject(region);
            let destroyed = DestroyWindow(hwnd);
            RoUninitialize();
            assert_eq!(active, Ok(true));
            assert_eq!(calc, 0);
            assert_eq!(proposed_client, [0, 0, 760, 470]);
            assert_eq!(nc_active, 1);
            assert_ne!(original_style & WS_CAPTION, 0);
            assert_eq!(active_style, original_style & !WS_CAPTION);
            // Windows removes WS_EX_WINDOWEDGE (0x100) with WS_CAPTION;
            // the backdrop must not add WS_EX_LAYERED or change other bits.
            assert_eq!(active_ex & !0x100, original_ex & !0x100);
            assert_eq!(restored_ex, original_ex);
            assert_eq!(reapplied_style, active_style);
            assert_eq!(restored_style, original_style);
            assert!(
                frame_removed,
                "collapsed window must not retain Panel subclass"
            );
            assert_eq!(clipped, 3, "rounded region must be installed");
            assert_eq!(cleared, Ok(false));
            assert_eq!(unclipped, 0, "collapsed window must not retain clipping");
            assert_ne!(destroyed, 0);
        }
    }
}

pub async fn configure(window: &WebviewWindow, enabled: bool) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|_| "PANEL_HWND_UNAVAILABLE")?.0 as usize;
        let size = window.inner_size().map_err(|_| "PANEL_SIZE_UNAVAILABLE")?;
        let scale = window
            .scale_factor()
            .map_err(|_| "PANEL_SCALE_UNAVAILABLE")?;
        let bounds = windows::clip_bounds(size.width, size.height, scale);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                let _ = sender.send(windows::configure(hwnd, enabled, bounds));
            })
            .map_err(|_| "PANEL_BACKDROP_DISPATCH_FAILED")?;
        receiver
            .await
            .map_err(|_| "PANEL_BACKDROP_DISPATCH_FAILED")?
    }
    #[cfg(not(windows))]
    {
        let _ = (window, enabled);
        Ok(false)
    }
}

pub async fn shutdown(window: &WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|_| "PANEL_HWND_UNAVAILABLE")?.0 as usize;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            window
                .run_on_main_thread(move || {
                    let _ = sender.send(windows::shutdown(hwnd));
                })
                .map_err(|_| "PANEL_SHUTDOWN_DISPATCH_FAILED")?;
            let complete = tokio::time::timeout_at(deadline, receiver)
                .await
                .map_err(|_| "PANEL_SHUTDOWN_TIMEOUT")?
                .map_err(|_| "PANEL_SHUTDOWN_DISPATCH_FAILED")??;
            if complete {
                return Ok(());
            }
            // Await off-thread; the owning UI loop must keep dispatching shutdown work.
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(())
    }
}

/// Settings/tray use the same proven native clip, with their own 24-DIP radius.
/// Main-panel bounds and material remain unchanged.
pub async fn configure_utility(window: &WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|_| "UTILITY_HWND_UNAVAILABLE")?.0 as usize;
        let size = window
            .inner_size()
            .map_err(|_| "UTILITY_SIZE_UNAVAILABLE")?;
        let scale = window
            .scale_factor()
            .map_err(|_| "UTILITY_DPI_UNAVAILABLE")?;
        let mut bounds = windows::clip_bounds(size.width, size.height, scale);
        bounds[4] = (48.0 * scale).round() as i32;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                let _ = sender.send(windows::configure(hwnd, true, bounds));
            })
            .map_err(|_| "UTILITY_DISPATCH_FAILED")?;
        if !receiver.await.map_err(|_| "UTILITY_DISPATCH_FAILED")?? {
            return Err("UTILITY_GLASS_UNSUPPORTED".into());
        }
    }
    Ok(())
}
