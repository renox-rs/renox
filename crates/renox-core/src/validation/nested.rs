//! Forms with nested names: `items[0][name]=Coffee&items[0][qty]=2` read into
//! `items: Vec<Item>`, and `meta[0][key]=…` into a list of pairs. Plain
//! forms keep going through `serde_html_form`; a form switches to this
//! reader when one of its names has a `[`.
//!
//! Every name is turned into a dotted path (`items.0.name`), which is also
//! how its errors are keyed, so `error('items[0][name]')` and the
//! `data-error-for` slots find them. Values stay text until the target type
//! asks for something else: "2" becomes a number for an `i64`, "on" `true`
//! for a `bool`, "" `None` for an `Option`.

use serde::de::value::{MapDeserializer, SeqDeserializer, StringDeserializer};
use serde::de::{self, IntoDeserializer, Visitor};
use serde_json::{Map, Value};

/// How many levels a nested name may have (`a[b][c]` has three).
const MAX_DEPTH: usize = 32;

/// Whether a form uses nested names.
pub(crate) fn is_nested<'a>(mut keys: impl Iterator<Item = &'a str>) -> bool {
    keys.any(|key| key.contains('['))
}

/// `items[0][name]` (or `items.0.name`) as the dotted path `items.0.name`;
/// `tags[]` is `tags` (repeated names make a list anyway).
pub(crate) fn normalize(name: &str) -> String {
    if !name.contains('[') {
        return name.to_owned();
    }
    segments(name).join(".")
}

fn segments(name: &str) -> Vec<String> {
    name.replace(']', "")
        .split(['[', '.'])
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A form read into a tree: text at the leaves, lists where every key of a
/// level is a number.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Leaf(String),
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    /// The tree of `pairs` (names in any form). A name sent twice makes a
    /// list, as with a group of checkboxes.
    pub(crate) fn build(pairs: &[(String, String)]) -> Node {
        let mut root = Node::Map(Vec::new());
        for (name, value) in pairs {
            let path = segments(name);
            // Deeper than any form goes: ignored, so a crafted name can't
            // make the reader recurse without end.
            if !path.is_empty() && path.len() <= MAX_DEPTH {
                // `tags[]` is a list even with one value: never a scalar field.
                root.insert(&path, value.clone(), name.ends_with("[]"));
            }
        }
        root.into_lists()
    }

    fn insert(&mut self, path: &[String], value: String, list: bool) {
        let Node::Map(entries) = self else { return };
        let (first, rest) = (&path[0], &path[1..]);
        let found = entries.iter().position(|(key, _)| key == first);
        if rest.is_empty() {
            match found {
                Some(i) => match &mut entries[i].1 {
                    Node::Seq(items) => items.push(Node::Leaf(value)),
                    leaf @ Node::Leaf(_) => {
                        let first = std::mem::replace(leaf, Node::Seq(Vec::new()));
                        *leaf = Node::Seq(vec![first, Node::Leaf(value)]);
                    }
                    // `a=1` after `a[b]=2`: the deeper name wins.
                    Node::Map(_) => {}
                },
                None if list => entries.push((first.clone(), Node::Seq(vec![Node::Leaf(value)]))),
                None => entries.push((first.clone(), Node::Leaf(value))),
            }
            return;
        }
        let i = match found {
            Some(i) => i,
            None => {
                entries.push((first.clone(), Node::Map(Vec::new())));
                entries.len() - 1
            }
        };
        if !matches!(entries[i].1, Node::Map(_)) {
            entries[i].1 = Node::Map(Vec::new());
        }
        entries[i].1.insert(rest, value, list);
    }

    /// Levels keyed 0, 1, 2… become lists, in the order of their numbers.
    fn into_lists(self) -> Node {
        match self {
            Node::Map(entries) => {
                let numbered = !entries.is_empty()
                    && entries
                        .iter()
                        .all(|(key, _)| key.bytes().all(|b| b.is_ascii_digit()));
                if numbered {
                    let mut items: Vec<(u64, Node)> = entries
                        .into_iter()
                        .map(|(key, node)| (key.parse().unwrap_or(u64::MAX), node.into_lists()))
                        .collect();
                    items.sort_by_key(|(index, _)| *index);
                    Node::Seq(items.into_iter().map(|(_, node)| node).collect())
                } else {
                    Node::Map(
                        entries
                            .into_iter()
                            .map(|(key, node)| (key, node.into_lists()))
                            .collect(),
                    )
                }
            }
            // A name sent several times (`tags`, `tags`): as in a plain form,
            // empty values aren't items (an empty "add a tag" box).
            Node::Seq(items) => Node::Seq(
                items
                    .into_iter()
                    .filter(|item| !matches!(item, Node::Leaf(text) if text.trim().is_empty()))
                    .map(Node::into_lists)
                    .collect(),
            ),
            leaf => leaf,
        }
    }

    /// The tree as JSON, for `old()` after a failed submit.
    pub(crate) fn into_json(self) -> Value {
        match self {
            Node::Leaf(text) => Value::String(text),
            Node::Seq(items) => Value::Array(items.into_iter().map(Node::into_json).collect()),
            Node::Map(entries) => Value::Object(
                entries
                    .into_iter()
                    .map(|(key, node)| (key, node.into_json()))
                    .collect::<Map<String, Value>>(),
            ),
        }
    }
}

/// The value at a dotted path of nested JSON (`items.0.name`).
pub(crate) fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    segments(path)
        .iter()
        .try_fold(value, |value, segment| match value {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        })
}

/// A deserialization error; its message is matched by `extract.rs` like
/// serde_html_form's ("missing field", "invalid digit", "unknown variant").
#[derive(Debug)]
pub(crate) struct DeError(String);

impl std::fmt::Display for DeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        DeError(msg.to_string())
    }
}

impl<'de> IntoDeserializer<'de, DeError> for Node {
    type Deserializer = Node;
    fn into_deserializer(self) -> Node {
        self
    }
}

macro_rules! parse_number {
    ($($method:ident => $visit:ident: $ty:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
            match self {
                Node::Leaf(text) => {
                    let number: $ty = text.trim().parse().map_err(|e| DeError(format!("{e}")))?;
                    visitor.$visit(number)
                }
                other => other.deserialize_any(visitor),
            }
        }
    )*};
}

impl<'de> de::Deserializer<'de> for Node {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Node::Leaf(text) => visitor.visit_string(text),
            Node::Seq(items) => {
                let mut seq = SeqDeserializer::new(items.into_iter());
                let value = visitor.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(value)
            }
            Node::Map(entries) => {
                let mut map = MapDeserializer::new(entries.into_iter());
                let value = visitor.visit_map(&mut map)?;
                map.end()?;
                Ok(value)
            }
        }
    }

    parse_number! {
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_i128 => visit_i128: i128,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
        deserialize_u128 => visit_u128: u128,
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
    }

    /// A checkbox sends "on" when ticked and nothing (put back as "") when not.
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Node::Leaf(text) => match text.trim().to_ascii_lowercase().as_str() {
                "true" | "on" | "1" | "yes" | "checked" => visitor.visit_bool(true),
                "false" | "off" | "0" | "no" | "" => visitor.visit_bool(false),
                _ => Err(DeError("provided string was not `true` or `false`".into())),
            },
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self {
            Node::Leaf(text) if text.trim().is_empty() => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    /// One value where a list is expected: a list of one (a single ticked box).
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Node::Leaf(text) if text.trim().is_empty() => {
                Node::Seq(Vec::new()).deserialize_any(visitor)
            }
            leaf @ Node::Leaf(_) => Node::Seq(vec![leaf]).deserialize_any(visitor),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        match self {
            Node::Leaf(text) => {
                let variant: StringDeserializer<DeError> = text.into_deserializer();
                visitor.visit_enum(variant)
            }
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        visitor.visit_unit()
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf map struct identifier
    }
}

/// Deserializes `T` from a form tree, with the failing field's path kept
/// for the error.
pub(crate) fn deserialize<T: de::DeserializeOwned>(
    node: Node,
) -> Result<T, serde_path_to_error::Error<DeError>> {
    serde_path_to_error::deserialize(node)
}

/// The label of a field in messages: the app's translation of the name
/// (`renox.validation.attributes.<name>`), else, for a nested name, of
/// `items.*.name` and then of its last part (`name`), else that part with
/// `_` as spaces.
pub(crate) fn label(field: &str, translated: impl Fn(&str) -> Option<String>) -> String {
    if let Some(label) = translated(field) {
        return label;
    }
    if !field.contains('.') {
        return field.replace('_', " ");
    }
    let parts = segments(field);
    let starred: Vec<&str> = parts
        .iter()
        .map(|p| {
            if p.bytes().all(|b| b.is_ascii_digit()) {
                "*"
            } else {
                p.as_str()
            }
        })
        .collect();
    let last = parts
        .iter()
        .rev()
        .find(|p| !p.bytes().all(|b| b.is_ascii_digit()))
        .map(String::as_str)
        .unwrap_or(field);
    translated(&starred.join("."))
        .or_else(|| translated(last))
        .unwrap_or_else(|| last.replace('_', " "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct Line {
        name: String,
        qty: i64,
        note: Option<String>,
        #[serde(default)]
        gift: bool,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Size {
        Small,
        Large,
    }

    #[derive(Deserialize, Debug)]
    struct Order {
        title: String,
        lines: Vec<Line>,
        #[serde(default)]
        tags: Vec<String>,
        size: Size,
    }

    #[test]
    fn nested_names_read_into_lists_of_structs() {
        let node = Node::build(&pairs(&[
            ("title", "Coffee"),
            ("lines[1][name]", "Tea"),
            ("lines[1][qty]", "1"),
            ("lines[1][note]", ""),
            ("lines[0][name]", "Coffee"),
            ("lines[0][qty]", " 2 "),
            ("lines[0][note]", "less sugar"),
            ("lines[0][gift]", "on"),
            ("tags[]", "a"),
            ("tags[]", "b"),
            ("size", "large"),
        ]));
        let order: Order = deserialize(node).unwrap();
        assert_eq!(order.title, "Coffee");
        assert_eq!(order.tags, ["a", "b"]);
        assert_eq!(order.size, Size::Large);
        assert_eq!(
            order.lines,
            [
                Line {
                    name: "Coffee".into(),
                    qty: 2,
                    note: Some("less sugar".into()),
                    gift: true
                },
                Line {
                    name: "Tea".into(),
                    qty: 1,
                    note: None,
                    gift: false
                },
            ]
        );
    }

    #[test]
    fn errors_name_the_nested_field() {
        let node = Node::build(&pairs(&[
            ("title", "x"),
            ("size", "small"),
            ("lines[0][name]", "a"),
            ("lines[0][qty]", "lots"),
        ]));
        let err = deserialize::<Order>(node).unwrap_err();
        assert_eq!(normalize(&err.path().to_string()), "lines.0.qty");
        assert!(err.inner().to_string().contains("invalid digit"));
    }

    #[test]
    fn a_single_value_is_a_list_of_one() {
        #[derive(Deserialize)]
        struct Form {
            tags: Vec<String>,
            rows: Vec<std::collections::BTreeMap<String, String>>,
        }
        let node = Node::build(&pairs(&[("tags", "a"), ("rows[0][k]", "v")]));
        let form: Form = deserialize(node).unwrap();
        assert_eq!(form.tags, ["a"]);
        assert_eq!(form.rows[0]["k"], "v");
    }

    #[test]
    fn names_and_lookups() {
        assert_eq!(normalize("items[0][name]"), "items.0.name");
        assert_eq!(normalize("tags[]"), "tags");
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Scalar {
            name: String,
        }
        // A name deeper than any form is ignored.
        let deep = format!("x{}", "[a]".repeat(10_000));
        let node = Node::build(&[(deep, "1".into()), ("name".into(), "ok".into())]);
        assert_eq!(deserialize::<Scalar>(node).unwrap().name, "ok");
        // `name[]=a` is a list: not accepted for a text field.
        assert!(deserialize::<Scalar>(Node::build(&pairs(&[("name[]", "a")]))).is_err());
        assert_eq!(normalize("plain"), "plain");
        assert!(is_nested(["a", "b[0]"].into_iter()));
        assert!(!is_nested(["a", "b.c"].into_iter()));
        let json = Node::build(&pairs(&[
            ("items[0][name]", "Coffee"),
            ("items[1][name]", ""),
        ]))
        .into_json();
        assert_eq!(lookup(&json, "items[0][name]").unwrap(), "Coffee");
        assert_eq!(lookup(&json, "items.1.name").unwrap(), "");
        assert_eq!(lookup(&json, "items").unwrap().as_array().unwrap().len(), 2);
        assert!(lookup(&json, "items.5.name").is_none());
    }

    // #250: the parts of the reader no form had reached yet.

    #[test]
    fn a_name_that_is_both_a_value_and_a_map_keeps_the_deeper_one() {
        #[derive(Deserialize, Debug)]
        struct Form {
            a: std::collections::BTreeMap<String, String>,
        }
        // `a[b]` first, then a plain `a`: the plain value is dropped.
        let node = Node::build(&pairs(&[("a[b]", "2"), ("a", "1")]));
        let form: Form = deserialize(node).unwrap();
        assert_eq!(form.a.get("b").map(String::as_str), Some("2"));
        // A plain `a`, then `a[b]`: the map replaces the value.
        let node = Node::build(&pairs(&[("a", "1"), ("a[b]", "2")]));
        let form: Form = deserialize(node).unwrap();
        assert_eq!(form.a.len(), 1);
    }

    #[test]
    fn booleans_read_every_way_a_form_says_no() {
        #[derive(Deserialize)]
        struct Flags {
            items: Vec<Flag>,
        }
        #[derive(Deserialize)]
        struct Flag {
            on: bool,
        }
        let mut list = Vec::new();
        let values = ["false", "off", "0", "no", "", "TRUE", "yes", "checked"];
        for (i, value) in values.iter().enumerate() {
            list.push((format!("items[{i}][on]"), (*value).to_owned()));
        }
        let flags: Flags = deserialize(Node::build(&list)).unwrap();
        let read: Vec<bool> = flags.items.iter().map(|f| f.on).collect();
        assert_eq!(read, [false, false, false, false, false, true, true, true]);

        let bad = Node::build(&pairs(&[("items[0][on]", "maybe")]));
        let err = deserialize::<Flags>(bad).err().unwrap();
        assert_eq!(err.path().to_string(), "items[0].on");
    }

    #[test]
    fn tuples_newtypes_and_empty_lists() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Cents(i64);
        #[derive(Deserialize, Debug)]
        struct Form {
            point: (i64, i64),
            price: Cents,
            #[serde(default)]
            tags: Vec<String>,
            sizes: Vec<String>,
        }
        let node = Node::build(&pairs(&[
            ("point[0]", "3"),
            ("point[1]", "4"),
            ("price", "1250"),
            ("sizes", ""),
        ]));
        let form: Form = deserialize(node).unwrap();
        assert_eq!(form.point, (3, 4));
        assert_eq!(form.price, Cents(1250));
        assert!(form.tags.is_empty());
        // An empty value where a list is expected is an empty list.
        assert!(form.sizes.is_empty());
    }

    #[test]
    fn an_enum_sent_as_a_map_is_refused_at_its_path() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        enum Shape {
            Circle { r: i64 },
        }
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Form {
            shape: Shape,
        }
        // Forms send an enum as its variant's name; anything deeper is an
        // error on that field, not a panic.
        let node = Node::build(&pairs(&[("shape[Circle][r]", "2")]));
        let err = deserialize::<Form>(node).unwrap_err();
        assert_eq!(err.path().to_string(), "shape");
    }

    #[test]
    fn lookups_stop_at_values_that_have_no_children() {
        let json = Node::build(&pairs(&[("a[0][b]", "x")])).into_json();
        assert_eq!(lookup(&json, "a.0.b"), Some(&Value::String("x".into())));
        assert_eq!(lookup(&json, "a.0.b.c"), None);
        assert_eq!(lookup(&json, "a.x"), None);
    }

    #[test]
    fn numbers_and_booleans_sent_as_maps_are_refused_at_their_path() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Form {
            qty: i64,
            gift: bool,
        }
        let node = Node::build(&pairs(&[("qty[a]", "1"), ("gift", "on")]));
        let err = deserialize::<Form>(node).unwrap_err();
        assert_eq!(err.path().to_string(), "qty");
        assert!(err.inner().to_string().contains("invalid type"), "{err}");
        let node = Node::build(&pairs(&[("qty", "1"), ("gift[a]", "on")]));
        let err = deserialize::<Form>(node).unwrap_err();
        assert_eq!(err.path().to_string(), "gift");
    }

    #[test]
    fn tuple_structs_units_and_unknown_fields_read_like_serde_forms() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Point(i64, i64);
        #[derive(Deserialize, Debug, PartialEq)]
        struct Marker;
        #[derive(Deserialize, Debug, PartialEq)]
        struct Form {
            at: Point,
            marker: Marker,
            nothing: (),
        }
        // `extra` isn't a field: skipped (`deserialize_ignored_any`), as
        // serde skips unknown fields unless told otherwise.
        let node = Node::build(&pairs(&[
            ("at[]", "3"),
            ("at[]", "4"),
            ("marker", ""),
            ("nothing", ""),
            ("extra[deep]", "x"),
        ]));
        let form: Form = deserialize(node).unwrap();
        assert_eq!(
            form,
            Form {
                at: Point(3, 4),
                marker: Marker,
                nothing: ()
            }
        );
    }
}
