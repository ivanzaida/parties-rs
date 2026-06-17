use super::*;

#[test]
fn macos_backend_order_matches_original_parties() {
  assert_eq!(BACKEND_ORDER, [NativeVideoBackend::AppleVideoToolbox]);
}

#[test]
fn splits_annex_b_nals() {
  let nals = split_annex_b(&[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3]);
  assert_eq!(nals, vec![vec![0x67, 1, 2], vec![0x68, 3]]);
}

#[test]
fn splits_length_prefixed_nals() {
  let nals = split_length_prefixed(&[0, 0, 0, 2, 0x67, 1, 0, 0, 0, 1, 0x68]).unwrap();
  assert_eq!(nals, vec![vec![0x67, 1], vec![0x68]]);
}
