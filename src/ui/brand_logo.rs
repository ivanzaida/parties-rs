use lurq::{
  components::Row,
  node::{Element, dimension::Dimension},
};

pub(crate) const LOGO_RESOURCE: &str = "icons/parties_logo.png";
pub(crate) const LOGO_BYTES: &[u8] = include_bytes!("../../assets/icons/parties_logo.png");

pub(crate) fn logo_mark(size: impl Into<Dimension>, radius: f32) -> impl Into<Element> {
  let size = size.into();

  Row::new()
    .width(size)
    .height(size)
    .rounded(radius)
    .clip()
    .background_image(LOGO_RESOURCE)
    .background_cover()
}
