//! `KeyValues`: the pairs of the UI kit's `key_value` field.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::value::{FieldValue, Inspected};
use crate::db::DbValue;

/// Pairs of text in the order they were entered, like headers or settings:
/// what the UI kit's `key_value` field sends (`meta[0][key]`,
/// `meta[0][value]`, …). Rows without a key are skipped, and a key entered
/// twice keeps its last value.
///
/// It is stored (`db::Json<KeyValues>`) and serialized as a list of pairs,
/// `[["Origin", "Aceh"], ["Roast", "Medium"]]`, so the order holds (a JSON
/// object would lose it in PostgreSQL's `JSONB`), and reads back from that,
/// or from an object; `key_value("meta", "Meta", value=item.meta)` shows
/// what was saved.
///
/// ```
/// # use renox::prelude::*;
/// # use renox::KeyValues;
/// #[derive(serde::Deserialize)]
/// struct Settings {
///     #[serde(default)]
///     headers: KeyValues,
/// }
///
/// impl Validate for Settings {
///     fn rules(&self, v: &mut Validator) {
///         v.field("headers", &self.headers).max(20);
///     }
/// }
/// # fn demo(form: Settings) {
/// for (name, value) in form.headers.iter() {
///     println!("{name}: {value}");
/// }
/// assert_eq!(form.headers.get("Accept"), None);
/// # }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyValues(Vec<(String, String)>);

impl KeyValues {
    /// No pairs.
    pub fn new() -> Self {
        Self::default()
    }

    /// The value of `key`, if there is one.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets `key` to `value`, keeping its place if it was there.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let (key, value) = (key.into(), value.into());
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(pair) => pair.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// The pairs, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// How many pairs there are.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The pairs as a `Vec`.
    pub fn into_vec(self) -> Vec<(String, String)> {
        self.0
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for KeyValues {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
        let mut all = KeyValues::new();
        for (key, value) in pairs {
            all.insert(key, value);
        }
        all
    }
}

impl Serialize for KeyValues {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for pair in &self.0 {
            seq.serialize_element(pair)?;
        }
        seq.end()
    }
}

/// A row of the form, `{key, value}` (either may be missing or empty), or a
/// stored pair, `[key, value]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum Row {
    Form {
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        value: Option<String>,
    },
    Pair(String, String),
}

impl<'de> Deserialize<'de> for KeyValues {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Pairs;

        impl<'de> Visitor<'de> for Pairs {
            type Value = KeyValues;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("rows of key and value, or an object")
            }

            // The form's `{key, value}` rows, or the stored `[key, value]` pairs.
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<KeyValues, A::Error> {
                let mut all = KeyValues::new();
                while let Some(row) = seq.next_element::<Row>()? {
                    let (key, value) = match row {
                        Row::Form { key, value } => {
                            (key.unwrap_or_default(), value.unwrap_or_default())
                        }
                        Row::Pair(key, value) => (key, value),
                    };
                    let key = key.trim();
                    if !key.is_empty() {
                        all.insert(key, value);
                    }
                }
                Ok(all)
            }

            // An object (written by hand, or another app).
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<KeyValues, A::Error> {
                let mut all = KeyValues::new();
                while let Some((key, value)) = map.next_entry::<String, String>()? {
                    all.insert(key, value);
                }
                Ok(all)
            }

            // An empty field.
            fn visit_str<E: de::Error>(self, value: &str) -> Result<KeyValues, E> {
                if value.trim().is_empty() {
                    Ok(KeyValues::new())
                } else {
                    Err(E::invalid_type(de::Unexpected::Str(value), &self))
                }
            }

            fn visit_unit<E: de::Error>(self) -> Result<KeyValues, E> {
                Ok(KeyValues::new())
            }
        }

        deserializer.deserialize_any(Pairs)
    }
}

impl FieldValue for KeyValues {
    fn inspect(&self) -> Inspected {
        if self.is_empty() {
            Inspected::Missing
        } else {
            Inspected::Items(self.len())
        }
    }

    fn db_value(&self) -> DbValue {
        DbValue::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::nested::{Node, deserialize};

    #[test]
    fn reads_form_rows_and_saved_objects() {
        #[derive(Deserialize)]
        struct Form {
            meta: KeyValues,
        }
        let pairs: Vec<(String, String)> = [
            ("meta[0][key]", "Color"),
            ("meta[0][value]", "Red"),
            ("meta[1][key]", ""),
            ("meta[1][value]", "ignored"),
            ("meta[2][key]", " Size "),
            ("meta[2][value]", "L"),
            ("meta[3][key]", "Color"),
            ("meta[3][value]", "Blue"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        let form: Form = deserialize(Node::build(&pairs)).unwrap();
        assert_eq!(
            form.meta.iter().collect::<Vec<_>>(),
            [("Color", "Blue"), ("Size", "L")]
        );
        let json = serde_json::to_string(&form.meta).unwrap();
        assert_eq!(json, r#"[["Color","Blue"],["Size","L"]]"#);
        let back: KeyValues = serde_json::from_str(&json).unwrap();
        assert_eq!(back, form.meta);
        assert_eq!(back.get("Size"), Some("L"));
        let object: KeyValues = serde_json::from_str(r#"{"Size":"L","Color":"Blue"}"#).unwrap();
        assert_eq!(
            object.iter().collect::<Vec<_>>(),
            [("Size", "L"), ("Color", "Blue")]
        );
        let empty: KeyValues = serde_json::from_str(r#""""#).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn text_that_isnt_empty_and_null_are_handled() {
        // Some text where rows were expected is refused, naming what's wanted.
        let err = serde_json::from_str::<KeyValues>(r#""colour=red""#).unwrap_err();
        assert!(
            err.to_string()
                .contains("expected rows of key and value, or an object"),
            "{err}"
        );
        // An empty text box and `null` are no pairs.
        assert!(
            serde_json::from_str::<KeyValues>(r#""  ""#)
                .unwrap()
                .is_empty()
        );
        assert!(
            serde_json::from_str::<KeyValues>("null")
                .unwrap()
                .is_empty()
        );
        let pairs: KeyValues =
            serde_json::from_str(r#"[["a", "1"], {"key": " b ", "value": "2"}, {"key": ""}]"#)
                .unwrap();
        assert_eq!(
            pairs.into_vec(),
            [
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );
    }
}
