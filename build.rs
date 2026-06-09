fn main() {
  #[cfg(target_os = "macos")]
  {
    cc::Build::new()
      .cpp(true)
      .std("c++17")
      .flag_if_supported("-fobjc-arc")
      .flag_if_supported("-fblocks")
      .file("src/native/macos_video/bridge/macos_stream_bridge.mm")
      .warnings(false)
      .compile("parties_macos_stream_bridge");

    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=VideoToolbox");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rerun-if-changed=src/native/macos_video/bridge/macos_stream_bridge.mm");
  }

  #[cfg(target_os = "windows")]
  {
    cc::Build::new()
      .cpp(true)
      .std("c++17")
      .flag_if_supported("/EHsc")
      .file("src/native/windows_video/bridge/gpu_stream_bridge.cpp")
      .file("src/native/windows_video/bridge/nvdec_bridge.cpp")
      .file("src/native/windows_video/bridge/nvenc_bridge.cpp")
      .file("src/native/windows_video/capture/windows_screen_capture.cpp")
      .file("src/native/windows_video/nvidia/nvidia_loader.cpp")
      .file("src/native/windows_video/nvidia/nvdec_decoder.cpp")
      .file("src/native/windows_video/nvidia/nvenc_encoder.cpp")
      .include("src/native/windows_video")
      .include("src/native/windows_video/common")
      .include("src/native/windows_video/capture")
      .include("src/native/windows_video/nvidia")
      .warnings(false)
      .compile("parties_nvdec_bridge");

    println!("cargo:rustc-link-lib=d3d11");
    println!("cargo:rustc-link-lib=dxgi");
    println!("cargo:rustc-link-lib=dwmapi");
    println!("cargo:rustc-link-lib=windowsapp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/gpu_stream_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/nvdec_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/nvenc_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/common/video_types.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/capture/windows_screen_capture.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/capture/windows_screen_capture.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvidia_loader.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvidia_loader.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvdec_decoder.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvdec_decoder.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvenc_encoder.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvenc_encoder.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/cuda_drvapi.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/cuviddec.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvcuvid.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/nvidia/nvEncodeAPI.h");
    println!("cargo:rerun-if-changed=assets/icons/parties_icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/parties_icon.ico");
    resource.compile().expect("failed to compile Windows app resources");
  }
}
