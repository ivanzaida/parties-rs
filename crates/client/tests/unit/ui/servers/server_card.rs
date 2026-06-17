use super::server_letter;

#[test]
fn server_letter_uses_first_alphanumeric_character() {
  assert_eq!(server_letter("parties"), "P");
  assert_eq!(server_letter("  - millionaries"), "M");
  assert_eq!(server_letter("9 lives"), "9");
}

#[test]
fn server_letter_falls_back_when_name_has_no_alphanumeric_character() {
  assert_eq!(server_letter(""), "?");
  assert_eq!(server_letter(" --- "), "?");
}
