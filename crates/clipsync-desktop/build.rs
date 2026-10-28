fn main() {
    println!("cargo:rerun-if-changed=native/record.m");
    println!("cargo:rerun-if-changed=native/windows.m");
    println!("cargo:rerun-if-changed=native/meminfo.m");
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("native/record.m")
            .file("native/windows.m")
            .file("native/meminfo.m")
            .flag("-fobjc-arc")
            .compile("clipsyncrecord");
        println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
