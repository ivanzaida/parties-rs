fn main() {
  #[cfg(target_os = "windows")]
  {
    use std::path::Path;

    let original = Path::new("..").join("parties");
    let encdec = original.join("encdec");
    let common = original.join("common");

    cc::Build::new()
      .cpp(true)
      .std("c++17")
      .file("src/services/video/nvdec_bridge.cpp")
      .file(encdec.join("src/nvidia/nvidia_loader.cpp"))
      .file(encdec.join("src/nvidia/nvdec_decoder.cpp"))
      .include("src/services/video/nvdec_shim")
      .include(encdec.join("src"))
      .include(encdec.join("include"))
      .include(common.join("include"))
      .warnings(false)
      .compile("parties_nvdec_bridge");

    println!("cargo:rerun-if-changed=src/services/video/nvdec_bridge.cpp");
    println!("cargo:rerun-if-changed=src/services/video/nvdec_shim/parties/log.h");
    println!("cargo:rerun-if-changed=src/services/video/nvdec_shim/parties/profiler.h");
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvidia_loader.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvdec_decoder.cpp").display()
    );
    println!(
      "cargo:rerun-if-changed={}",
      encdec.join("src/nvidia/nvdec_decoder.h").display()
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
