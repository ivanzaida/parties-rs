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

pub const PERMISSION_MATRIX_ROLES: [Role; 4] = [Role::Owner, Role::Admin, Role::Moderator, Role::User];
pub const PERMISSION_MATRIX_PERMISSIONS: [Permission; 15] = [
  Permission::JoinChannel,
  Permission::Speak,
  Permission::MuteOthers,
  Permission::DeafenOthers,
  Permission::KickFromChannel,
  Permission::KickFromServer,
  Permission::CreateChannel,
  Permission::DeleteChannel,
  Permission::ManagePermissions,
  Permission::ManageRoles,
  Permission::ManageServer,
  Permission::SendText,
  Permission::UploadFiles,
  Permission::ShareScreen,
  Permission::ShareWebcam,
];

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
#[path = "../../../tests/unit/network/protocol/permissions.rs"]
mod tests;
