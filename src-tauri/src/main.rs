#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--codex-flow-stop-hook"))
    {
        codex_flow_lib::codex_hook::run_stop_hook();
        return;
    }
    codex_flow_lib::run();
}
