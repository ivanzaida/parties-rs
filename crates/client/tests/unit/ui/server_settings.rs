use std::collections::HashMap;

use super::{
  active_members, can_assign_member_role, display_server_name, member_handle, member_initials, parse_max_users,
  role_has_default_permission, text_channel_icon, voice_channel_icon,
};
use crate::{
  network::protocol::{ChannelId, Permission, Role, UserId},
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, LobbyUser},
};

fn channel(id: ChannelId, name: &str) -> LobbyChannel {
  LobbyChannel {
    id,
    name: name.to_owned(),
    max_users: 0,
    sort_order: id,
    user_count: 0,
  }
}

fn user(user_id: UserId, username: &str, role: Role) -> LobbyUser {
  LobbyUser {
    user_id,
    username: username.to_owned(),
    role,
    muted: false,
    deafened: false,
    speaking: false,
  }
}

fn connected_info(server_name: &str) -> ConnectedServerInfo {
  ConnectedServerInfo {
    address: "example.com:7800".to_owned(),
    server_name: server_name.to_owned(),
    display_name: "local".to_owned(),
    user_id: 1,
    role: Role::Admin,
    certificate_fingerprint: "aa:bb".to_owned(),
  }
}

#[test]
fn parse_max_users_accepts_empty_as_unlimited_and_whole_numbers() {
  assert_eq!(parse_max_users(""), Ok(0));
  assert_eq!(parse_max_users("  "), Ok(0));
  assert_eq!(parse_max_users("12"), Ok(12));
  assert_eq!(parse_max_users(" 42 "), Ok(42));
}

#[test]
fn parse_max_users_rejects_non_whole_numbers() {
  assert!(parse_max_users("-1").is_err());
  assert!(parse_max_users("1.5").is_err());
  assert!(parse_max_users("eight").is_err());
}

#[test]
fn active_members_merges_voice_membership_with_known_users_sorted_by_user_id() {
  let mut remote = user(4, "remote", Role::User);
  remote.speaking = true;
  let lobby = LobbyState {
    channels: vec![channel(10, "General")],
    users_by_channel: HashMap::from([
      (10, vec![remote.clone()]),
      (99, vec![user(2, "unknown", Role::Moderator)]),
    ]),
    users: vec![user(3, "idle", Role::Admin), user(4, "stale-name", Role::User)],
    ..LobbyState::default()
  };

  let members = active_members(&lobby);

  assert_eq!(
    members.iter().map(|member| member.user.user_id).collect::<Vec<_>>(),
    vec![2, 3, 4]
  );
  assert_eq!(members[0].channels, vec!["#99"]);
  assert!(members[1].channels.is_empty());
  assert_eq!(members[2].user.username, "remote");
  assert_eq!(members[2].channels, vec!["General"]);
}

#[test]
fn can_assign_member_role_follows_role_hierarchy() {
  assert!(can_assign_member_role(Role::Owner, Role::Owner));
  assert!(can_assign_member_role(Role::Admin, Role::Moderator));
  assert!(can_assign_member_role(Role::Admin, Role::User));
  assert!(!can_assign_member_role(Role::Admin, Role::Admin));
  assert!(!can_assign_member_role(Role::Admin, Role::Owner));
  assert!(can_assign_member_role(Role::Moderator, Role::User));
  assert!(!can_assign_member_role(Role::User, Role::Moderator));
}

#[test]
fn role_has_default_permission_matches_permission_matrix_contract() {
  assert!(role_has_default_permission(Role::Owner, Permission::None));
  assert!(role_has_default_permission(Role::Admin, Permission::ManageServer));
  assert!(!role_has_default_permission(Role::Admin, Permission::SendText));
  assert!(role_has_default_permission(Role::Moderator, Permission::MuteOthers));
  assert!(!role_has_default_permission(
    Role::Moderator,
    Permission::KickFromServer
  ));
  assert!(role_has_default_permission(Role::User, Permission::JoinChannel));
  assert!(role_has_default_permission(Role::User, Permission::Speak));
  assert!(!role_has_default_permission(Role::User, Permission::MuteOthers));
}

#[test]
fn member_handle_keeps_chat_safe_ascii_handle_characters() {
  assert_eq!(member_handle("Lurq Master"), "lurqmaster");
  assert_eq!(member_handle("dev_user-7"), "dev_user-7");
  assert_eq!(member_handle("!!!"), "user");
}

#[test]
fn member_initials_prefers_word_initials_then_character_fallback() {
  assert_eq!(member_initials("lurq master"), "LM");
  assert_eq!(member_initials("lurq"), "L");
  assert_eq!(member_initials("9 lives"), "9L");
  assert_eq!(member_initials("!!!"), "?");
}

#[test]
fn channel_icons_use_specialized_icons_for_known_names() {
  assert_eq!(voice_channel_icon("Stage Room"), "radio");
  assert_eq!(voice_channel_icon("General"), "volume-2");
  assert_eq!(text_channel_icon("announcements"), "megaphone");
  assert_eq!(text_channel_icon("general"), "hash");
}

#[test]
fn display_server_name_uses_generic_fallback_for_blank_names() {
  assert_eq!(
    display_server_name(&connected_info("Millionaries Hub")),
    "Millionaries Hub"
  );
  assert_eq!(display_server_name(&connected_info("   ")), "Server");
}
