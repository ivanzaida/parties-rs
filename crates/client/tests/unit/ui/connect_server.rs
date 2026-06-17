use super::certificate_fingerprint_changed;

#[test]
fn certificate_fingerprint_change_requires_existing_and_received_values() {
  assert!(!certificate_fingerprint_changed("", "aa:bb"));
  assert!(!certificate_fingerprint_changed("aa:bb", ""));
  assert!(!certificate_fingerprint_changed("aa:bb", "AA:BB"));
  assert!(certificate_fingerprint_changed("aa:bb", "cc:dd"));
}
