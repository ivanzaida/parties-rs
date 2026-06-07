use lurq::app::i18n::I18n;

pub fn setup(i18n: &I18n) {
  i18n.set_locale("en");
  i18n.set_fallback_locale("en");
  i18n
    .add_resources_json(
      "en",
      "translation",
      std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/i18n/en.json"),
    )
    .expect("failed to load English i18n resources");
  i18n
    .add_resources_json(
      "uk",
      "translation",
      std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/i18n/uk.json"),
    )
    .expect("failed to load Ukrainian i18n resources");
  i18n
    .add_resources_json(
      "be",
      "translation",
      std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/i18n/be.json"),
    )
    .expect("failed to load Belarusian i18n resources");
}
