use super::*;

#[test]
fn i420_planes_pack_to_nv12() {
  let y = [1, 2, 3, 4];
  let u = [5];
  let v = [6];
  let decoded = i420_planes_to_nv12(&y, &u, &v, 2, 2, (2, 1, 1), None).unwrap();

  assert_eq!(decoded.format, DecodedVideoPixelFormat::Nv12);
  assert_eq!(decoded.pixels, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn i420_planes_reuse_output_buffer() {
  let y = [1, 2, 3, 4];
  let u = [5];
  let v = [6];
  let mut reusable = Vec::with_capacity(16);
  reusable.extend_from_slice(&[9, 9, 9, 9, 9, 9]);
  let original_ptr = reusable.as_ptr();
  let decoded = i420_planes_to_nv12(&y, &u, &v, 2, 2, (2, 1, 1), Some(reusable)).unwrap();

  assert_eq!(decoded.pixels.as_ptr(), original_ptr);
  assert_eq!(decoded.pixels, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn h264_annex_b_input_clamps_high_sps_level_for_openh264() {
  let input = [0, 0, 0, 1, 0x67, 100, 0, 60, 1, 0, 0, 1, 0x68, 2, 0, 0, 1, 0x65, 3];
  let parsed = h264_annex_b_decode_input(&input).unwrap();

  assert!(matches!(parsed.data, Cow::Owned(_)));
  assert_eq!(parsed.data[7], 52);
  assert_eq!(parsed.summary.nals, 3);
  assert_eq!(parsed.summary.sps, 1);
  assert_eq!(parsed.summary.pps, 1);
  assert_eq!(parsed.summary.idr, 1);
  assert_eq!(parsed.summary.sps_profile, Some(100));
  assert_eq!(parsed.summary.sps_level, Some(52));
  assert_eq!(parsed.summary.sps_level_clamped_from, Some(60));
  assert!(!parsed.summary.length_prefixed);
  assert_eq!(parsed.ranges, [4..9, 12..14, 17..19]);
}

#[test]
fn h264_length_prefixed_input_converts_to_annex_b() {
  let input = [0, 0, 0, 4, 0x67, 100, 0, 60, 0, 0, 0, 1, 0x68];
  let parsed = h264_annex_b_decode_input(&input).unwrap();

  assert_eq!(parsed.data.as_ref(), [0, 0, 0, 1, 0x67, 100, 0, 52, 0, 0, 0, 1, 0x68]);
  assert_eq!(parsed.summary.nals, 2);
  assert_eq!(parsed.summary.sps, 1);
  assert_eq!(parsed.summary.pps, 1);
  assert_eq!(parsed.summary.sps_profile, Some(100));
  assert_eq!(parsed.summary.sps_level, Some(52));
  assert_eq!(parsed.summary.sps_level_clamped_from, Some(60));
  assert!(parsed.summary.length_prefixed);
  assert_eq!(parsed.ranges, [4..8, 12..13]);
}

#[test]
fn h264_invalid_nal_range_returns_error_instead_of_panicking() {
  let data = [0; 12];
  let invalid_range = std::ops::Range { start: 13, end: 12 };

  let error = h264_nal_range(&data, invalid_range, "bad range").unwrap_err();

  assert_eq!(error.to_string(), "bad range");
}
