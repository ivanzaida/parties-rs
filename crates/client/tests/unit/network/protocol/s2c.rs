use super::*;

#[test]
fn keepalive_pong_decodes() {
  let frame = ControlFrame {
    ty: ControlMessageType::KeepalivePong,
    payload: Vec::new(),
  };
  assert_eq!(S2C::decode(&frame).unwrap(), S2C::KeepalivePong);
}

#[test]
fn server_error_decodes() {
  let mut w = super::super::BinaryWriter::new();
  w.write_u16(ServerErrorCode::BadAuth.as_u16());
  w.write_string("bad request").unwrap();
  let frame = ControlFrame {
    ty: ControlMessageType::ServerError,
    payload: w.into_bytes(),
  };
  match S2C::decode(&frame).unwrap() {
    S2C::ServerError { code, message } => {
      assert_eq!(code, ServerErrorCode::BadAuth);
      assert_eq!(message, "bad request");
    }
    other => panic!("expected ServerError, got {other:?}"),
  }
}

#[test]
fn chat_file_ready_decodes_attachment_id() {
  let mut w = super::super::BinaryWriter::new();
  w.write_u64(42);
  w.write_u64(7);
  let frame = ControlFrame {
    ty: ControlMessageType::ChatFileReady,
    payload: w.into_bytes(),
  };
  assert_eq!(
    S2C::decode(&frame).unwrap(),
    S2C::ChatFileReady {
      message_id: 42,
      attachment_id: 7,
    }
  );
}

#[test]
fn chat_command_list_decodes() {
  let mut w = super::super::BinaryWriter::new();
  w.write_u16(1);
  w.write_string("botping").unwrap();
  w.write_string("Ping the bot").unwrap();
  w.write_string("/botping [text]").unwrap();
  let frame = ControlFrame {
    ty: ControlMessageType::ChatCommandList,
    payload: w.into_bytes(),
  };

  assert_eq!(
    S2C::decode(&frame).unwrap(),
    S2C::ChatCommandList(ChatCommandList {
      commands: vec![crate::network::protocol::control::ChatCommandInfo {
        name: "botping".to_owned(),
        description: "Ping the bot".to_owned(),
        usage: "/botping [text]".to_owned(),
      }]
    })
  );
}

#[test]
fn c2s_type_returns_error() {
  let frame = ControlFrame {
    ty: ControlMessageType::AuthIdentity,
    payload: Vec::new(),
  };
  assert!(S2C::decode(&frame).is_err());
}
