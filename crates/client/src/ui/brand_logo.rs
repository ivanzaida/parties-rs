use lurq::{
  components::Row,
  images::ImageData,
  node::{Element, dimension::Dimension},
};

pub const LOGO_BYTES: &[u8] = include_bytes!("../../assets/icons/parties_logo.png");

pub(crate) fn logo_mark(size: impl Into<Dimension>, radius: f32) -> impl Into<Element> {
  let size = size.into();

  Row::new()
    .width(size)
    .height(size)
    .rounded(radius)
    .clip()
    .background_image(ImageData::from_bytes(LOGO_BYTES).unwrap())
    .background_cover()
}
