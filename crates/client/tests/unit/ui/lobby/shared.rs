use super::user_display_name;

#[test]
fn user_display_name_preserves_plain_name_when_debug_ids_are_hidden() {
  assert_eq!(user_display_name(29, "lurkm", false), "lurkm");
}

#[test]
fn user_display_name_prefixes_id_when_debug_ids_are_visible() {
  assert_eq!(user_display_name(29, "lurkm", true), "[id:29] lurkm");
}

#[test]
fn user_display_name_keeps_empty_names_visible_with_debug_id() {
  assert_eq!(user_display_name(29, "", true), "[id:29] ");
}
