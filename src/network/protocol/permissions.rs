use super::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Permission {
  None = 0,
  JoinChannel = 1 << 0,
  Speak = 1 << 1,
  MuteOthers = 1 << 2,
  DeafenOthers = 1 << 3,
  KickFromChannel = 1 << 4,
  KickFromServer = 1 << 5,
  CreateChannel = 1 << 6,
  DeleteChannel = 1 << 7,
  ManagePermissions = 1 << 8,
  ManageRoles = 1 << 9,
  ManageServer = 1 << 10,
  SendText = 1 << 11,
  UploadFiles = 1 << 12,
  ShareScreen = 1 << 13,
  ShareWebcam = 1 << 14,
}

pub const DEFAULT_OWNER_PERMS: u32 = 0xFFFF_FFFF;
pub const DEFAULT_ADMIN_PERMS: u32 = 0x0000_07FF;
pub const DEFAULT_MODERATOR_PERMS: u32 = 0x0000_001F;
pub const DEFAULT_USER_PERMS: u32 = 0x0000_0003;

pub fn default_permissions(role: Role) -> u32 {
  match role {
    Role::Owner => DEFAULT_OWNER_PERMS,
    Role::Admin => DEFAULT_ADMIN_PERMS,
    Role::Moderator => DEFAULT_MODERATOR_PERMS,
    Role::User => DEFAULT_USER_PERMS,
  }
}

pub fn has_permission(role: Role, permission: Permission, channel_override: Option<u32>) -> bool {
  if role == Role::Owner {
    return true;
  }

  let permissions = channel_override.unwrap_or_else(|| default_permissions(role));
  permissions & permission as u32 != 0
}

pub fn can_moderate(actor: Role, target: Role) -> bool {
  (actor as u8) < target as u8
}

pub fn can_edit_server_settings(role: Role) -> bool {
  role.has_permission(Permission::ManageServer)
    || role.has_permission(Permission::ManagePermissions)
    || role.has_permission(Permission::ManageRoles)
    || role.has_permission(Permission::CreateChannel)
    || role.has_permission(Permission::DeleteChannel)
}

pub fn can_manage_channels(role: Role) -> bool {
  role.has_permission(Permission::ManageServer)
    || role.has_permission(Permission::CreateChannel)
    || role.has_permission(Permission::DeleteChannel)
}

impl Role {
  pub fn default_permissions(self) -> u32 {
    default_permissions(self)
  }

  pub fn has_permission(self, permission: Permission) -> bool {
    has_permission(self, permission, None)
  }

  pub fn has_channel_permission(self, permission: Permission, channel_override: u32) -> bool {
    has_permission(self, permission, Some(channel_override))
  }

  pub fn can_moderate(self, target: Role) -> bool {
    can_moderate(self, target)
  }

  pub fn can_edit_server_settings(self) -> bool {
    can_edit_server_settings(self)
  }

  pub fn can_manage_channels(self) -> bool {
    can_manage_channels(self)
  }
}

#[cfg(test)]
mod tests {
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
}
