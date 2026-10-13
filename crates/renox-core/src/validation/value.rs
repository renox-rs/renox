use crate::db::{DbValue, ToDbValue};

/// What a rule sees of a field's value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Inspected {
    /// `None`, or text that is empty after trimming.
    Missing,
    /// Non-empty text.
    Text(String),
    /// A number.
    Number(f64),
    /// A boolean, e.g. a checkbox.
    Bool(bool),
    /// A list (`Vec`) with this many items.
    Items(usize),
    /// An uploaded file: size in kilobytes, the extension its content (or
    /// else its name) indicates, whether it really is an image, and its
    /// size in pixels.
    #[non_exhaustive]
    File {
        /// Size in kilobytes.
        kilobytes: f64,
        /// The extension its content indicates, else its name's.
        extension: String,
        /// Whether the content really is an image.
        image: bool,
        /// Width and height in pixels, for images.
        dimensions: Option<(u32, u32)>,
    },
    /// A date or a date and time (a date alone is its midnight).
    Date(chrono::NaiveDateTime),
}

/// A value that can be validated. Implemented for strings, numbers, `bool`,
/// `Option<T>` and `Vec<T>`.
pub trait FieldValue {
    /// What rules see of the value.
    fn inspect(&self) -> Inspected;
    /// The value as a query parameter, for `unique` and `exists`.
    fn db_value(&self) -> DbValue;
}

impl FieldValue for str {
    fn inspect(&self) -> Inspected {
        if self.trim().is_empty() {
            Inspected::Missing
        } else {
            Inspected::Text(self.to_owned())
        }
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

impl FieldValue for String {
    fn inspect(&self) -> Inspected {
        self.as_str().inspect()
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

macro_rules! number {
    ($($t:ty),*) => {$(
        impl FieldValue for $t {
            fn inspect(&self) -> Inspected {
                Inspected::Number(*self as f64)
            }

            fn db_value(&self) -> DbValue {
                self.to_db_value()
            }
        }
    )*};
}

number!(i8, i16, i32, i64, u8, u16, u32, f32, f64);

impl FieldValue for bool {
    fn inspect(&self) -> Inspected {
        Inspected::Bool(*self)
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

impl FieldValue for chrono::NaiveDate {
    fn inspect(&self) -> Inspected {
        Inspected::Date(self.and_time(chrono::NaiveTime::MIN))
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

impl FieldValue for chrono::NaiveDateTime {
    fn inspect(&self) -> Inspected {
        Inspected::Date(*self)
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

impl FieldValue for chrono::DateTime<chrono::Utc> {
    fn inspect(&self) -> Inspected {
        Inspected::Date(self.naive_utc())
    }

    fn db_value(&self) -> DbValue {
        self.to_db_value()
    }
}

impl<T: FieldValue> FieldValue for Option<T> {
    fn inspect(&self) -> Inspected {
        self.as_ref()
            .map_or(Inspected::Missing, FieldValue::inspect)
    }

    fn db_value(&self) -> DbValue {
        self.as_ref().map_or(DbValue::Null, FieldValue::db_value)
    }
}

impl<T> FieldValue for Vec<T> {
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

impl<T: FieldValue + ?Sized> FieldValue for &T {
    fn inspect(&self) -> Inspected {
        (**self).inspect()
    }

    fn db_value(&self) -> DbValue {
        (**self).db_value()
    }
}

impl FieldValue for crate::upload::Upload {
    fn inspect(&self) -> Inspected {
        // An empty file that was chosen is a file with no type: `image()`
        // and `mimes()` refuse it. (No file chosen never gets here.)
        if self.bytes().is_empty() {
            return Inspected::File {
                kilobytes: 0.0,
                extension: String::new(),
                image: false,
                dimensions: None,
            };
        }
        let extension = match self.sniffed_type() {
            Some("image/png") => "png".to_owned(),
            Some("image/jpeg") => "jpg".to_owned(),
            Some("image/gif") => "gif".to_owned(),
            Some("image/webp") => "webp".to_owned(),
            Some("application/pdf") => "pdf".to_owned(),
            _ => self.extension().unwrap_or_default(),
        };
        Inspected::File {
            kilobytes: self.size() as f64 / 1024.0,
            extension,
            image: self.is_image(),
            dimensions: self.dimensions(),
        }
    }

    fn db_value(&self) -> DbValue {
        DbValue::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload::Upload;
    use crate::validation::Validator;

    #[test]
    fn a_text_file_named_png_is_not_an_image() {
        let fake = Some(Upload::new(
            "fake.png",
            "image/png",
            axum::body::Bytes::from_static(b"not an image\n"),
        ));
        assert!(matches!(
            FieldValue::inspect(&fake),
            Inspected::File { image: false, .. }
        ));
        let mut v = Validator::new();
        v.field("photo", &fake).label("picture").image().max(2048);
        assert_eq!(
            v.errors.first("photo"),
            Some("The picture must be an image.")
        );
    }
}
