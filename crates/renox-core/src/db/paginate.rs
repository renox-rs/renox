use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::Serialize;

/// The `?page=` query parameter, defaulting to 1.
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64 }
/// async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
///     let products = Product::query().latest().paginate(&db, page, 20).await?;
///     Ok(view("products/index.html", context! { products }))
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page(pub u32);

impl<S: Send + Sync> FromRequestParts<S> for Page {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        let page = parts
            .uri
            .query()
            .and_then(|query| {
                form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == "page")
                    .and_then(|(_, value)| value.parse().ok())
            })
            .unwrap_or(1u32)
            .max(1);
        Ok(Self(page))
    }
}

/// One page of results. Render its links with the built-in macro:
///
/// ```jinja
/// {% from "renox/pagination.html" import pagination %}
/// {{ pagination(products) }}
/// ```
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Paginated<T> {
    /// The rows on this page.
    pub items: Vec<T>,
    /// The current page, 1-based.
    pub page: u32,
    /// Rows per page.
    pub per_page: u32,
    /// Rows matching the query on all pages.
    pub total: u64,
    /// The last page number, at least 1 (also when there are no rows).
    pub last_page: u32,
    /// Position of the first and last item on this page (1-based), 0 when empty.
    pub from: u64,
    /// Position of the last item on this page (1-based), 0 when empty.
    pub to: u64,
    /// Whether there is a page before this one.
    pub has_prev: bool,
    /// Whether there is a page after this one.
    pub has_next: bool,
    /// Page numbers to link to; `None` marks a gap ("…").
    pub pages: Vec<Option<u32>>,
}

impl<T> Paginated<T> {
    /// A page of `items` out of `total` rows; works out the other fields.
    pub fn new(items: Vec<T>, page: u32, per_page: u32, total: u64) -> Self {
        let last_page = total.div_ceil(u64::from(per_page)).max(1) as u32;
        let from = if items.is_empty() {
            0
        } else {
            u64::from(page - 1) * u64::from(per_page) + 1
        };
        let to = if items.is_empty() {
            0
        } else {
            from + items.len() as u64 - 1
        };
        Self {
            items,
            page,
            per_page,
            total,
            last_page,
            from,
            to,
            has_prev: page > 1,
            has_next: page < last_page,
            pages: window(page, last_page),
        }
    }

    /// Converts the items, keeping the page information.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Paginated<U> {
        Paginated {
            items: self.items.into_iter().map(f).collect(),
            page: self.page,
            per_page: self.per_page,
            total: self.total,
            last_page: self.last_page,
            from: self.from,
            to: self.to,
            has_prev: self.has_prev,
            has_next: self.has_next,
            pages: self.pages,
        }
    }
}

/// One page without the total (`Query::simple_paginate`): one query, for
/// "previous / next" links on large tables.
///
/// ```jinja
/// {% from "renox/pagination.html" import simple_pagination %}
/// {{ simple_pagination(orders) }}
/// ```
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct SimplePage<T> {
    /// The rows on this page.
    pub items: Vec<T>,
    /// The current page, 1-based.
    pub page: u32,
    /// Rows per page.
    pub per_page: u32,
    /// Whether there is a page before this one.
    pub has_prev: bool,
    /// Whether there is a page after this one (one extra row was found).
    pub has_next: bool,
}

impl<T> SimplePage<T> {
    /// Converts the items, keeping the page information.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> SimplePage<U> {
        SimplePage {
            items: self.items.into_iter().map(f).collect(),
            page: self.page,
            per_page: self.per_page,
            has_prev: self.has_prev,
            has_next: self.has_next,
        }
    }
}

/// Rows after a cursor (`Query::cursor_paginate`), newest first: stable
/// while rows are added, and fast at any depth. Pass `next_cursor` back as
/// the cursor to get the following rows; `None` means there are no more.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CursorPage<T> {
    /// The rows after the cursor.
    pub items: Vec<T>,
    /// Rows per page.
    pub per_page: u32,
    /// The cursor for the following rows; `None` on the last page.
    pub next_cursor: Option<String>,
}

impl<T> CursorPage<T> {
    /// Converts the items, keeping the cursor.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> CursorPage<U> {
        CursorPage {
            items: self.items.into_iter().map(f).collect(),
            per_page: self.per_page,
            next_cursor: self.next_cursor,
        }
    }
}

/// First, last, and two pages either side of the current one.
fn window(page: u32, last: u32) -> Vec<Option<u32>> {
    let mut pages = Vec::new();
    let mut previous = 0;
    for n in 1..=last {
        if n == 1 || n == last || n.abs_diff(page) <= 2 {
            if n > previous + 1 {
                pages.push(None);
            }
            pages.push(Some(n));
            previous = n;
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pages_and_positions() {
        let p = Paginated::new(vec![11, 12, 13], 2, 10, 23);
        assert_eq!((p.last_page, p.from, p.to), (3, 11, 13));
        assert!(p.has_prev && p.has_next);

        let empty = Paginated::<i32>::new(vec![], 1, 10, 0);
        assert_eq!((empty.last_page, empty.from, empty.to), (1, 0, 0));
        assert!(!empty.has_prev && !empty.has_next);
    }

    #[test]
    fn windows_page_links() {
        assert_eq!(window(1, 3), vec![Some(1), Some(2), Some(3)]);
        assert_eq!(
            window(10, 20),
            vec![
                Some(1),
                None,
                Some(8),
                Some(9),
                Some(10),
                Some(11),
                Some(12),
                None,
                Some(20)
            ]
        );
        assert_eq!(
            window(2, 9),
            vec![Some(1), Some(2), Some(3), Some(4), None, Some(9)]
        );
    }

    // #254: map keeps the page information.
    #[test]
    fn map_keeps_the_page_information() {
        let page = Paginated {
            items: vec![1, 2],
            page: 2,
            per_page: 2,
            total: 5,
            last_page: 3,
            from: 3,
            to: 4,
            has_prev: true,
            has_next: true,
            pages: window(2, 3),
        }
        .map(|n| n * 10);
        assert_eq!(page.items, [10, 20]);
        assert_eq!((page.page, page.total, page.last_page), (2, 5, 3));
        assert!(page.has_prev && page.has_next);

        let simple = SimplePage {
            items: vec!["a"],
            page: 1,
            per_page: 10,
            has_prev: false,
            has_next: true,
        }
        .map(str::to_uppercase);
        assert_eq!(simple.items, ["A"]);
        assert!(!simple.has_prev && simple.has_next);

        let cursor = CursorPage {
            items: vec![1],
            per_page: 1,
            next_cursor: Some("abc".into()),
        }
        .map(|n| n + 1);
        assert_eq!(cursor.items, [2]);
        assert_eq!(cursor.next_cursor.as_deref(), Some("abc"));
    }
}
