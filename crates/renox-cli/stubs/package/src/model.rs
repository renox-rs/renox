use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// A row of the package's table (replace it with your own models).
#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "__lib___entries")]
pub struct Entry {
    /// The primary key.
    pub id: i64,
    /// What the entry says.
    pub title: String,
    /// When the row was created.
    pub created_at: Option<DateTime>,
    /// When the row was last changed.
    pub updated_at: Option<DateTime>,
}
