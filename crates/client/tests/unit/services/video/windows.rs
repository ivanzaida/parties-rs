use super::*;

#[test]
fn windows_broadcast_backend_order_prefers_native_hardware() {
  assert_eq!(
    BACKEND_ORDER,
    [
      NativeVideoBackend::NvidiaNvenc,
      NativeVideoBackend::AmdAmf,
      NativeVideoBackend::OpenH264,
    ]
  );
  assert_eq!(backend_order_label(), "NVENC -> AMF -> OpenH264");
}

#[test]
fn rgba_to_nv12_converts_black_frame_to_video_range_neutral_chroma() {
  let rgba = [0, 0, 0, 255].repeat(4);
  let nv12 = rgba_to_nv12(&rgba, 2, 2).unwrap();

  assert_eq!(&nv12[..4], &[16, 16, 16, 16]);
  assert_eq!(&nv12[4..], &[128, 128]);
}

#[test]
fn rgba_to_nv12_converts_white_frame_to_video_range_neutral_chroma() {
  let rgba = [255, 255, 255, 255].repeat(4);
  let nv12 = rgba_to_nv12(&rgba, 2, 2).unwrap();

  assert_eq!(&nv12[..4], &[235, 235, 235, 235]);
  assert_eq!(&nv12[4..], &[128, 128]);
}

#[test]
fn rgba_to_nv12_rejects_odd_dimensions() {
  let rgba = vec![0; 3 * 2 * 4];
  let error = rgba_to_nv12(&rgba, 3, 2).unwrap_err();

  assert_eq!(error.to_string(), "NV12 conversion requires non-zero even dimensions.");
}

#[test]
fn rgba_to_bgra_swaps_red_and_blue() {
  let rgba = vec![10, 20, 30, 255, 40, 50, 60, 128];
  let bgra = rgba_to_bgra(&rgba).unwrap();

  assert_eq!(bgra, vec![30, 20, 10, 255, 60, 50, 40, 128]);
}

#[test]
fn normalize_rgba_frame_resizes_to_output_dimensions() {
  let rgba = vec![
    1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255, 19, 20, 21, 255, 22,
    23, 24, 255,
  ];

  let resized = normalize_rgba_frame(rgba, 4, 2, 2, 1).unwrap();

  assert_eq!(resized, vec![1, 2, 3, 255, 7, 8, 9, 255]);
}

#[test]
fn nv12_to_rgba_converts_black_frame() {
  let nv12 = [16, 16, 16, 16, 128, 128].to_vec();
  let rgba = nv12_to_rgba(&nv12, 2, 2).unwrap();

  assert_eq!(rgba, [0, 0, 0, 255].repeat(4));
}

#[test]
fn nv12_to_rgba_converts_white_frame() {
  let nv12 = [235, 235, 235, 235, 128, 128].to_vec();
  let rgba = nv12_to_rgba(&nv12, 2, 2).unwrap();

  assert_eq!(rgba, [255, 255, 255, 255].repeat(4));
}
