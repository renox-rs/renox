//! `Path`, axum's path extractor with a 404 for values that don't parse.

use std::ops::{Deref, DerefMut};

use axum::extract::FromRequestParts;
use axum::extract::path::ErrorKind;
use axum::extract::rejection::PathRejection;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::Error;

/// The route's parameters (`/orders/{id}` → `Path(id): Path<i64>`), like
/// axum's `Path`, except that a value that doesn't fit (`/orders/abc`, or an
/// id too large) is a 404 page, as for a route that doesn't exist, rather
/// than a plain-text 400.
///
/// ```
/// # use renox::prelude::*;
/// async fn show(Path(id): Path<i64>) -> String { format!("order {id}") }
/// async fn line(Path((order, line)): Path<(i64, i64)>) -> String { format!("{order}/{line}") }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<T> Deref for Path<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Path<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Path(value)),
            // A visitor's value that doesn't parse (`/products/abc`): not found.
            // A parameter count or type that can't work is the app's mistake.
            Err(PathRejection::FailedToDeserializePathParams(err)) => match err.kind() {
                ErrorKind::WrongNumberOfParameters { .. } | ErrorKind::UnsupportedType { .. } => {
                    Err(anyhow::anyhow!("{err}").into())
                }
                _ => Err(Error::NotFound),
            },
            // The route has no such parameters: a mistake in the app.
            Err(other) => Err(anyhow::anyhow!("{other}").into()),
        }
    }
}

/// The model a route parameter names, loaded from the database (Laravel's
/// route model binding), or a 404 page when there's no such row.
///
/// Which parameter: the one named after the model's table (`{product}`
/// for `Product`), else the route's only parameter. It is read as the
/// model's key (`{product}`, `{id}`), or, when its name is one of the
/// model's columns, matched against that column (`/posts/{slug}`). The
/// query is the model's own, so default scopes (the current tenant) and
/// soft deletes apply.
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
/// # #[derive(Model, serde::Serialize, Default)] struct Post { id: i64, slug: String }
/// // GET /products/{product}
/// async fn show(Found(product): Found<Product>) -> String { product.name }
///
/// // GET /blog/{slug}: the post whose `slug` column matches
/// async fn post(Found(post): Found<Post>) -> String { post.slug }
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found<M>(pub M);

impl<M> Deref for Found<M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.0
    }
}

impl<M> DerefMut for Found<M> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.0
    }
}

impl<M, S> FromRequestParts<S> for Found<M>
where
    M: crate::db::Model,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let params = axum::extract::RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let params: Vec<(String, String)> = params
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let (name, value) = match params.iter().find(|(name, _)| name == M::TABLE) {
            Some(param) => param.clone(),
            None if params.len() == 1 => params[0].clone(),
            None => {
                return Err(anyhow::anyhow!(
                    "Found<{}> needs a route parameter named `{}` (the route has {})",
                    std::any::type_name::<M>(),
                    M::TABLE,
                    params
                        .iter()
                        .map(|(name, _)| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .into());
            }
        };
        let app = parts
            .extensions
            .get::<crate::AppState>()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Found<T> needs Renox's request layers"))?;
        let by_column = name != M::TABLE && name != "id" && M::COLUMNS.contains(&name.as_str());
        let found = if by_column {
            M::query().where_eq(&name, value).first(&app.db).await?
        } else {
            let key = value
                .parse::<M::Key>()
                .ok()
                .filter(|key| !crate::db::ModelKey::is_unsaved(key));
            match key {
                Some(key) => M::find(&app.db, key).await?,
                None => None,
            }
        };
        found.map(Found).ok_or(Error::NotFound)
    }
}
