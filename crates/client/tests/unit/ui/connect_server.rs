use super::{certificate_fingerprint_changed, with_default_port};

#[test]
fn certificate_fingerprint_change_requires_existing_and_received_values() {
  assert!(!certificate_fingerprint_changed("", "aa:bb"));
  assert!(!certificate_fingerprint_changed("aa:bb", ""));
  assert!(!certificate_fingerprint_changed("aa:bb", "AA:BB"));
  assert!(!certificate_fingerprint_changed(" aa:bb ", "AA:BB"));
  assert!(certificate_fingerprint_changed("aa:bb", "cc:dd"));
}

#[test]
fn default_port_is_added_only_when_address_has_no_port() {
  assert_eq!(with_default_port("example.com"), "example.com:7800");
  assert_eq!(with_default_port(" example.com "), "example.com:7800");
  assert_eq!(with_default_port("example.com:1234"), "example.com:1234");
  assert_eq!(with_default_port("127.0.0.1"), "127.0.0.1:7800");
  assert_eq!(with_default_port("127.0.0.1:9000"), "127.0.0.1:9000");
}

#[test]
fn default_port_handles_bracketed_ipv6_addresses() {
  assert_eq!(with_default_port("[::1]"), "[::1]:7800");
  assert_eq!(with_default_port("[::1]:9000"), "[::1]:9000");
}
