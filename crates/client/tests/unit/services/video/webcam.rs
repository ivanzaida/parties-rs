use super::{WebcamFrameFormat, bgra_to_resized_rgba, nv12_to_resized_rgba, webcam_media_score, yuy2_to_resized_rgba};

#[test]
fn bgra_to_resized_rgba_swaps_blue_and_red() {
  let bgra = vec![30, 20, 10, 255, 60, 50, 40, 255];
  let rgba = bgra_to_resized_rgba(&bgra, 2, 1, 2, 1).unwrap();
  assert_eq!(rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);
}

#[test]
fn nv12_to_resized_rgba_converts_black_frame() {
  let nv12 = vec![16, 16, 16, 16, 128, 128];
  let rgba = nv12_to_resized_rgba(&nv12, 2, 2, 2, 2).unwrap();
  assert_eq!(rgba, vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
}

#[test]
fn yuy2_to_resized_rgba_converts_black_pair() {
  let yuy2 = vec![16, 128, 16, 128];
  let rgba = yuy2_to_resized_rgba(&yuy2, 2, 1, 2, 1).unwrap();
  assert_eq!(rgba, vec![0, 0, 0, 255, 0, 0, 0, 255]);
}

#[test]
fn webcam_media_score_prefers_requested_fps_over_resolution() {
  let fast_lower_resolution = webcam_media_score(WebcamFrameFormat::Nv12, 640, 480, 120, 1280, 720, 120);
  let slow_exact_resolution = webcam_media_score(WebcamFrameFormat::Nv12, 1280, 720, 30, 1280, 720, 120);

  assert!(fast_lower_resolution < slow_exact_resolution);
}

#[test]
fn webcam_media_score_penalizes_unknown_fps_for_high_fps_request() {
  let known_fps = webcam_media_score(WebcamFrameFormat::Nv12, 640, 480, 120, 1280, 720, 120);
  let unknown_fps = webcam_media_score(WebcamFrameFormat::Nv12, 1280, 720, 0, 1280, 720, 120);

  assert!(known_fps < unknown_fps);
}
