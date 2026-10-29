use std::fmt::Write as _;

fn main() {
    tauri_build::build();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=native-panel/panel_backdrop.cpp");
        cc::Build::new()
            .cpp(true)
            .file("native-panel/panel_backdrop.cpp")
            .define("UNICODE", None)
            .define("_UNICODE", None)
            .flag_if_supported("/std:c++20")
            .flag_if_supported("/EHsc")
            .flag_if_supported("/utf-8")
            .compile("codex_flow_panel_backdrop");
        println!("cargo:rerun-if-changed=native-lyrics/des.c");
        println!("cargo:rerun-if-changed=native-lyrics/des.h");
        println!("cargo:rerun-if-changed=native-lyrics/qrc_cipher.c");
        cc::Build::new()
            .file("native-lyrics/des.c")
            .file("native-lyrics/qrc_cipher.c")
            .compile("codex_flow_qrc_cipher");
        println!("cargo:rerun-if-changed=native-lyrics/media.cpp");
        cc::Build::new()
            .cpp(true)
            .file("native-lyrics/media.cpp")
            .define("UNICODE", None)
            .define("_UNICODE", None)
            .flag_if_supported("/std:c++20")
            .flag_if_supported("/EHsc")
            .flag_if_supported("/utf-8")
            .compile("codex_flow_qq_media");
        for library in [
            "windowsapp",
            "CoreMessaging",
            "user32",
            "gdi32",
            "comctl32",
            "runtimeobject",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
    }

    if std::env::var_os("CARGO_FEATURE_NATIVE_ORB_REFRACTION").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    {
        rerun_native_orb_changes();
        let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let shader = std::fs::read_to_string("native-orb/native_orb_shader.hlsl")
            .expect("read native Orb shader");
        std::fs::write(
            out_dir.join("native_orb_shader.inc"),
            format!("R\"CF_ORB({shader})CF_ORB\""),
        )
        .expect("generate embedded native Orb shader");

        let shell = std::fs::read("native-orb/orb-shell-80@4x.png")
            .expect("read native Orb glass shell asset");
        let mut shell_include = String::with_capacity(shell.len() * 6);
        for (index, byte) in shell.iter().enumerate() {
            if index % 24 == 0 {
                shell_include.push('\n');
            }
            write!(&mut shell_include, "0x{byte:02x},").unwrap();
        }
        std::fs::write(out_dir.join("native_orb_shell.inc"), shell_include)
            .expect("generate embedded native Orb glass shell asset");

        cc::Build::new()
            .cpp(true)
            .file("native-orb/native_orb.cpp")
            .include("native-orb")
            .include(&out_dir)
            .define("UNICODE", None)
            .define("_UNICODE", None)
            .flag_if_supported("/std:c++20")
            .flag_if_supported("/utf-8")
            .compile("codex_flow_native_orb");

        for library in [
            "d3d11",
            "dxgi",
            "d3dcompiler",
            "dcomp",
            "dwrite",
            "user32",
            "gdi32",
            "dwmapi",
            "ole32",
            "windowscodecs",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
    }
}

fn rerun_native_orb_changes() {
    println!("cargo:rerun-if-changed=native-orb/edge_dock.h");
    println!("cargo:rerun-if-changed=native-orb/native_orb_text.h");
    println!("cargo:rerun-if-changed=native-orb/native_orb.h");
    println!("cargo:rerun-if-changed=native-orb/native_orb.cpp");
    println!("cargo:rerun-if-changed=native-orb/native_orb_shader.hlsl");
    println!("cargo:rerun-if-changed=native-orb/orb-shell-80@4x.png");
}
