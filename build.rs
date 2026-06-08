fn main() {
  #[cfg(target_os = "macos")]
  {
    println!("cargo:rustc-link-lib=framework=VideoToolbox");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
  }

  #[cfg(target_os = "windows")]
  {
    use std::path::Path;

    let original = Path::new("..").join("parties");
    let encdec = original.join("encdec");
    let common = original.join("common");

    cc::Build::new()
      .cpp(true)
      .std("c++17")
      .file("src/services/video/gpu_stream_bridge.cpp")
      .file("src/services/video/nvdec_bridge.cpp")
      .file("src/services/video/nvenc_bridge.cpp")
      .file(original.join("client/src/windows/screen_capture.cpp"))
      .file(encdec.join("src/nvidia/nvidia_loader.cpp"))
      .file(encdec.join("src/nvidia/nvdec_decoder.cpp"))
      .file(encdec.join("src/nvidia/nvenc_encoder.cpp"))
      .include("src/services/video/nvdec_shim")
      .include(original.join("client/include"))
      .include(encdec.join("src"))
      .include(encdec.join("include"))
      .include(common.join("include"))
      .warnings(false)
      .compile("parties_nvdec_bridge");

    println!("cargo:rustc-link-lib=d3d11");
    println!("cargo:rustc-link-lib=dxgi");
    println!("cargo:rustc-link-lib=dwmapi");
    println!("cargo:rustc-link-lib=windowsapp");
    println!("cargo:rerun-if-changed=src/services/video/gpu_stream_bridge.cpp");
    println!("cargo:rerun-if-changed=src/services/video/nvdec_bridge.cpp");
    println!("cargo:rerun-if-changed=src/services/video/nvenc_bridge.cpp");
    println!("cargo:rerun-if-changed=src/services/video/nvdec_shim/parties/log.h");
    println!("cargo:rerun-if-changed=src/services/video/nvdec_shim/parties/profiler.h");
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvidia_loader.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      original.join("client/src/windows/screen_capture.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      original.join("client/include/client/screen_capture.h").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvdec_decoder.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvenc_encoder.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvdec_decoder.h").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvenc_encoder.h").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvidia_loader.h").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/cuda_drvapi.h").display()
    );
  }
}
