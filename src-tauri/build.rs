fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/macos_selection.m");
        cc::Build::new()
            .file("native/macos_selection.m")
            .flag("-fobjc-arc")
            .flag("-Wno-deprecated-declarations")
            .compile("macos_selection");

        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }

    tauri_build::build()
}
