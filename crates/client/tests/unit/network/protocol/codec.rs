use super::*;

#[test]
fn strings_are_u16_len_prefixed_utf8() {
  let mut w = BinaryWriter::new();
  w.write_string("test").unwrap();
  assert_eq!(w.as_slice(), &[4, 0, b't', b'e', b's', b't']);

  let mut r = BinaryReader::new(w.as_slice());
  assert_eq!(r.read_string().unwrap(), "test");
  r.finish().unwrap();
}
