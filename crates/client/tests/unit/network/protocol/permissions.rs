use super::*;

#[test]
fn default_permission_masks_match_upstream() {
  assert_eq!(default_permissions(Role::Owner), 0xFFFF_FFFF);
  assert_eq!(default_permissions(Role::Admin), 0x0000_07FF);
  assert_eq!(default_permissions(Role::Moderator), 0x0000_001F);
  assert_eq!(default_permissions(Role::User), 0x0000_0003);
}

#[test]
fn permissions_match_default_role_masks() {
  assert!(Role::User.has_permission(Permission::JoinChannel));
  assert!(Role::User.has_permission(Permission::Speak));
  assert!(!Role::User.has_permission(Permission::KickFromChannel));

  assert!(Role::Moderator.has_permission(Permission::KickFromChannel));
  assert!(!Role::Moderator.has_permission(Permission::KickFromServer));

  assert!(Role::Admin.has_permission(Permission::ManageServer));
  assert!(!Role::Admin.has_permission(Permission::SendText));
}

#[test]
fn owner_has_all_permissions_even_with_empty_override() {
  assert!(has_permission(Role::Owner, Permission::ManageRoles, Some(0)));
}

#[test]
fn channel_override_replaces_default_permissions() {
  assert!(has_permission(
    Role::User,
    Permission::SendText,
    Some(Permission::SendText as u32)
  ));
  assert!(!has_permission(Role::Admin, Permission::JoinChannel, Some(0)));
}

#[test]
fn moderation_requires_strictly_lower_rank_target() {
  assert!(Role::Owner.can_moderate(Role::Admin));
  assert!(Role::Admin.can_moderate(Role::Moderator));
  assert!(Role::Moderator.can_moderate(Role::User));

  assert!(!Role::User.can_moderate(Role::Moderator));
  assert!(!Role::Admin.can_moderate(Role::Admin));
  assert!(!Role::Moderator.can_moderate(Role::Owner));
}

#[test]
fn server_settings_are_only_visible_to_roles_with_edit_permissions() {
  assert!(Role::Owner.can_edit_server_settings());
  assert!(Role::Admin.can_edit_server_settings());
  assert!(!Role::Moderator.can_edit_server_settings());
  assert!(!Role::User.can_edit_server_settings());
}

#[test]
fn channel_settings_require_channel_management_permissions() {
  assert!(Role::Owner.can_manage_channels());
  assert!(Role::Admin.can_manage_channels());
  assert!(!Role::Moderator.can_manage_channels());
  assert!(!Role::User.can_manage_channels());
}
