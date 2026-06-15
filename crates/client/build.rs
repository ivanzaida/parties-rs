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
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=VideoToolbox");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rerun-if-changed=src/native/macos_video/bridge/macos_stream_bridge.mm");
  }

  #[cfg(target_os = "windows")]
  {
    compile_windows_libhevc();

    cc::Build::new()
      .cpp(true)
      .std("c++17")
      .define("_SILENCE_EXPERIMENTAL_COROUTINE_DEPRECATION_WARNINGS", None)
      .flag_if_supported("/EHsc")
      .file("src/native/windows_video/common/native_log.cpp")
      .file("src/native/windows_video/amd/amf/public/common/AMFFactory.cpp")
      .file("src/native/windows_video/amd/amf/public/common/Windows/ThreadWindows.cpp")
      .file("src/native/windows_video/amd/amf_decoder.cpp")
      .file("src/native/windows_video/amd/amf_encoder.cpp")
      .file("src/native/windows_video/bridge/amf_bridge.cpp")
      .file("src/native/windows_video/bridge/gpu_stream_bridge.cpp")
      .file("src/native/windows_video/bridge/nvdec_bridge.cpp")
      .file("src/native/windows_video/bridge/nvenc_bridge.cpp")
      .file("src/native/windows_video/capture/windows_screen_capture.cpp")
      .file("src/native/windows_video/nvidia/nvidia_loader.cpp")
      .file("src/native/windows_video/nvidia/nvdec_decoder.cpp")
      .file("src/native/windows_video/nvidia/nvenc_encoder.cpp")
      .file("src/native/windows_video/software/libhevc_decoder_bridge.cpp")
      .include("src/native/windows_video")
      .include("src/native/windows_video/amd")
      .include("src/native/windows_video/amd/amf/public")
      .include("src/native/windows_video/amd/amf/public/common")
      .include("src/native/windows_video/amd/amf/public/include")
      .include("src/native/windows_video/common")
      .include("src/native/windows_video/capture")
      .include("src/native/windows_video/nvidia")
      .include("../../third_party/libhevc/common")
      .include("../../third_party/libhevc/decoder")
      .warnings(false)
      .compile("parties_nvdec_bridge");

    println!("cargo:rustc-link-lib=d3d11");
    println!("cargo:rustc-link-lib=d3dcompiler");
    println!("cargo:rustc-link-lib=dxgi");
    println!("cargo:rustc-link-lib=dwmapi");
    println!("cargo:rustc-link-lib=winmm");
    println!("cargo:rustc-link-lib=windowsapp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/gpu_stream_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/amf_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/nvdec_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/bridge/nvenc_bridge.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf_decoder.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf_decoder.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf_encoder.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf_encoder.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf/public/common/AMFFactory.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf/public/common/AMFFactory.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf/public/common/Thread.h");
    println!("cargo:rerun-if-changed=src/native/windows_video/amd/amf/public/common/Windows/ThreadWindows.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/common/native_log.cpp");
    println!("cargo:rerun-if-changed=src/native/windows_video/common/native_profile.h");
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
    println!("cargo:rerun-if-changed=src/native/windows_video/software/libhevc_decoder_bridge.cpp");
    println!("cargo:rerun-if-changed=assets/icons/parties_icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/parties_icon.ico");
    resource.compile().expect("failed to compile Windows app resources");
  }
}

#[cfg(target_os = "windows")]
fn compile_windows_libhevc() {
  let root = "../../third_party/libhevc";
  let mut build = cc::Build::new();
  build
    .include(format!("{root}/common"))
    .include(format!("{root}/common/x86"))
    .include(format!("{root}/decoder"))
    .include(format!("{root}/decoder/x86"))
    .define("X86", None)
    .define("DISABLE_AVX2", None)
    .define("DEFAULT_ARCH", Some("D_ARCH_X86_SSE42"))
    .define("MSVC", None)
    .define("MEM_ALIGN8", Some("__declspec(align(8))"))
    .define("MEM_ALIGN16", Some("__declspec(align(16))"))
    .define("MEM_ALIGN32", Some("__declspec(align(32))"))
    .warnings(false);

  for file in [
    "common/ithread_win32.c",
    "common/ihevc_quant_tables.c",
    "common/ihevc_inter_pred_filters.c",
    "common/ihevc_weighted_pred.c",
    "common/ihevc_padding.c",
    "common/ihevc_deblk_edge_filter.c",
    "common/ihevc_deblk_tables.c",
    "common/ihevc_cabac_tables.c",
    "common/ihevc_common_tables.c",
    "common/ihevc_intra_pred_filters.c",
    "common/ihevc_chroma_intra_pred_filters.c",
    "common/ihevc_mem_fns.c",
    "common/ihevc_sao.c",
    "common/ihevc_trans_tables.c",
    "common/ihevc_recon.c",
    "common/ihevc_itrans.c",
    "common/ihevc_itrans_recon.c",
    "common/ihevc_iquant_recon.c",
    "common/ihevc_iquant_itrans_recon.c",
    "common/ihevc_itrans_recon_32x32.c",
    "common/ihevc_itrans_recon_16x16.c",
    "common/ihevc_itrans_recon_8x8.c",
    "common/ihevc_chroma_itrans_recon.c",
    "common/ihevc_chroma_iquant_recon.c",
    "common/ihevc_chroma_iquant_itrans_recon.c",
    "common/ihevc_chroma_recon.c",
    "common/ihevc_chroma_itrans_recon_16x16.c",
    "common/ihevc_chroma_itrans_recon_8x8.c",
    "common/ihevc_buf_mgr.c",
    "common/ihevc_disp_mgr.c",
    "common/ihevc_dpb_mgr.c",
    "common/ihevc_hbd_deblk_edge_filter.c",
    "common/ihevc_quant_iquant_ssd.c",
    "common/ihevc_resi_trans.c",
    "common/x86/ihevc_inter_pred_filters_ssse3_intr.c",
    "common/x86/ihevc_weighted_pred_ssse3_intr.c",
    "common/x86/ihevc_intra_pred_filters_ssse3_intr.c",
    "common/x86/ihevc_chroma_intra_pred_filters_ssse3_intr.c",
    "common/x86/ihevc_itrans_recon_ssse3_intr.c",
    "common/x86/ihevc_itrans_recon_16x16_ssse3_intr.c",
    "common/x86/ihevc_itrans_recon_32x32_ssse3_intr.c",
    "common/x86/ihevc_sao_ssse3_intr.c",
    "common/x86/ihevc_deblk_ssse3_intr.c",
    "common/x86/ihevc_padding_ssse3_intr.c",
    "common/x86/ihevc_mem_fns_ssse3_intr.c",
    "common/x86/ihevc_inter_pred_filters_sse42_intr.c",
    "common/x86/ihevc_weighted_pred_sse42_intr.c",
    "common/x86/ihevc_intra_pred_filters_sse42_intr.c",
    "common/x86/ihevc_chroma_intra_pred_filters_sse42_intr.c",
    "common/x86/ihevc_itrans_recon_sse42_intr.c",
    "common/x86/ihevc_16x16_itrans_recon_sse42_intr.c",
    "common/x86/ihevc_32x32_itrans_recon_sse42_intr.c",
    "common/x86/ihevc_tables_x86_intr.c",
    "decoder/ihevcd_version.c",
    "decoder/ihevcd_api.c",
    "decoder/ihevcd_decode.c",
    "decoder/ihevcd_nal.c",
    "decoder/ihevcd_bitstream.c",
    "decoder/ihevcd_parse_headers.c",
    "decoder/ihevcd_parse_slice_header.c",
    "decoder/ihevcd_parse_slice.c",
    "decoder/ihevcd_parse_residual.c",
    "decoder/ihevcd_cabac.c",
    "decoder/ihevcd_intra_pred_mode_prediction.c",
    "decoder/ihevcd_process_slice.c",
    "decoder/ihevcd_utils.c",
    "decoder/ihevcd_job_queue.c",
    "decoder/ihevcd_ref_list.c",
    "decoder/ihevcd_get_mv.c",
    "decoder/ihevcd_mv_pred.c",
    "decoder/ihevcd_mv_merge.c",
    "decoder/ihevcd_iquant_itrans_recon_ctb.c",
    "decoder/ihevcd_itrans_recon_dc.c",
    "decoder/ihevcd_common_tables.c",
    "decoder/ihevcd_boundary_strength.c",
    "decoder/ihevcd_deblk.c",
    "decoder/ihevcd_inter_pred.c",
    "decoder/ihevcd_sao.c",
    "decoder/ihevcd_ilf_padding.c",
    "decoder/ihevcd_fmt_conv.c",
    "decoder/x86/ihevcd_function_selector.c",
    "decoder/x86/ihevcd_function_selector_generic.c",
    "decoder/x86/ihevcd_function_selector_ssse3.c",
    "decoder/x86/ihevcd_function_selector_sse42.c",
    "decoder/x86/ihevcd_fmt_conv_ssse3_intr.c",
    "decoder/x86/ihevcd_it_rec_dc_ssse3_intr.c",
    "decoder/x86/ihevcd_it_rec_dc_sse42_intr.c",
  ] {
    build.file(format!("{root}/{file}"));
    println!("cargo:rerun-if-changed={root}/{file}");
  }

  build.compile("parties_libhevc");
  println!("cargo:rerun-if-changed={root}/common/x86/ihevc_platform_macros.h");
}
