use lurq::app::i18n::I18n;

const EN: &str = include_str!("../assets/i18n/en.json");
const UK: &str = include_str!("../assets/i18n/uk.json");
const BE: &str = include_str!("../assets/i18n/be.json");
const JSON_VALUE_KEY: &str = "$value";

pub fn setup(i18n: &I18n) {
  i18n.set_locale("en");
  i18n.set_fallback_locale("en");
  add_embedded_resources(i18n, "en", EN).expect("failed to load English i18n resources");
  add_embedded_resources(i18n, "uk", UK).expect("failed to load Ukrainian i18n resources");
  add_embedded_resources(i18n, "be", BE).expect("failed to load Belarusian i18n resources");
}

fn add_embedded_resources(i18n: &I18n, locale: &str, json: &str) -> Result<(), serde_json::Error> {
  let value: serde_json::Value = serde_json::from_str(json)?;
  let mut entries = Vec::new();
  flatten_json(&value, &mut String::new(), &mut entries);
  i18n.add_resources(locale, "translation", entries);
  Ok(())
}

fn flatten_json(value: &serde_json::Value, prefix: &mut String, out: &mut Vec<(String, String)>) {
  match value {
    serde_json::Value::Object(map) => {
      for (key, child) in map {
        if key == JSON_VALUE_KEY {
          push_json_value(child, prefix, out);
          continue;
        }

        let len = prefix.len();
        if !prefix.is_empty() {
          prefix.push('.');
        }
        prefix.push_str(key);
        flatten_json(child, prefix, out);
        prefix.truncate(len);
      }
    }
    other => push_json_value(other, prefix, out),
  }
}

fn push_json_value(value: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String)>) {
  match value {
    serde_json::Value::String(s) => out.push((prefix.to_owned(), s.clone())),
    other => out.push((prefix.to_owned(), other.to_string())),
  }
}
