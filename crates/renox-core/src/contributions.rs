//! Typed values modules contribute for other modules to collect.
//!
//! A module adds a value in `Module::register` with `Registry::contribute`;
//! another module reads them all, in order, through [`Contributions`].

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// The values of one type, with the name of the module that added each.
struct Slot<T> {
    values: Vec<T>,
    modules: Vec<String>,
}

/// The values modules contributed with `Registry::contribute`, by type.
///
/// Values of one type keep the order they were added in: the app's own
/// first, then each module's, in the order the modules are listed.
///
/// ```
/// use renox::prelude::*;
///
/// struct Link(&'static str);
///
/// struct Shop;
/// impl Module for Shop {
///     fn name(&self) -> &'static str { "shop" }
///     fn register(&self, app: &mut Registry) {
///         app.contribute(Link("/orders"));
///     }
/// }
/// # let _ = Shop;
/// ```
#[derive(Default)]
pub struct Contributions {
    by_type: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// The module whose `register` is running; "app" for the app's own.
    current: Option<String>,
}

impl Contributions {
    /// Every `T` contributed, in order. Empty when nothing of that type
    /// was contributed.
    pub fn all<T: 'static>(&self) -> &[T] {
        self.slot::<T>().map_or(&[], |s| &s.values)
    }

    /// Every `T` contributed, in order, with the name of the module that
    /// contributed it (`"app"` for the app's own).
    pub fn all_from<T: 'static>(&self) -> impl Iterator<Item = (&str, &T)> {
        self.slot::<T>()
            .into_iter()
            .flat_map(|s| s.modules.iter().map(String::as_str).zip(s.values.iter()))
    }

    fn slot<T: 'static>(&self) -> Option<&Slot<T>> {
        self.by_type
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<Slot<T>>())
    }

    /// Names the module whose `register` runs next.
    pub(crate) fn set_module(&mut self, name: &str) {
        self.current = Some(name.to_owned());
    }

    pub(crate) fn push<T: Send + Sync + 'static>(&mut self, value: T) {
        let module = self.current.clone().unwrap_or_else(|| "app".to_owned());
        let slot = self
            .by_type
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                Box::new(Slot::<T> {
                    values: Vec::new(),
                    modules: Vec::new(),
                })
            })
            .downcast_mut::<Slot<T>>()
            .expect("slot is keyed by its type");
        slot.values.push(value);
        slot.modules.push(module);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_type_is_empty() {
        let c = Contributions::default();
        assert!(c.all::<u32>().is_empty());
        assert_eq!(c.all_from::<u32>().count(), 0);
    }

    #[test]
    fn keeps_order_and_types_apart() {
        let mut c = Contributions::default();
        c.push(1u32);
        c.push("a");
        c.set_module("hr");
        c.push(2u32);
        c.set_module("billing");
        c.push(3u32);
        assert_eq!(c.all::<u32>(), &[1, 2, 3]);
        assert_eq!(c.all::<&str>(), &["a"]);
    }

    #[test]
    fn reports_the_contributing_module() {
        let mut c = Contributions::default();
        c.push(1u32);
        c.set_module("hr");
        c.push(2u32);
        let got: Vec<_> = c.all_from::<u32>().collect();
        assert_eq!(got, vec![("app", &1), ("hr", &2)]);
    }
}
