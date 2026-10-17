//! The suppliers resource of the admin panel, contributed by its own module.
//!
//! [`Purchasing`] adds [`Suppliers`] to the app's single `Admin`
//! (`app::staff::admin::panel`, registered in `src/lib.rs`) with
//! [`renox_admin::contribute`] instead of the panel listing it: the panel
//! never names this module, and a module can be left out of `src/lib.rs`
//! without touching the panel. The resource keeps its slug, its
//! `admin.suppliers.*` routes and its "Purchasing" navigation group; the
//! model's permission (`purchasing.manage`) is in the panel's policy list
//! (`staff::admin`), as for every model there.

use renox::grid::Column;
use renox::prelude::*;
use renox_admin::{AdminResource, Field};
use serde::{Deserialize, Serialize};

use crate::app::stock::model::Supplier;

/// The module that contributes the suppliers resource, registered in
/// `src/lib.rs`.
pub struct Purchasing;

impl Module for Purchasing {
    fn name(&self) -> &'static str {
        "purchasing"
    }

    fn register(&self, app: &mut Registry) {
        renox_admin::contribute(app, Suppliers);
    }
}

/// The suppliers the stores buy from.
pub struct Suppliers;

// [explain:admin.suppliers.form]
/// A supplier's form.
#[derive(Deserialize, Serialize, Validate)]
pub struct SupplierForm {
    #[validate(required, max = 150)]
    pub name: String,
    #[validate(email, max = 255)]
    pub email: Option<String>,
    #[validate(max = 40)]
    pub phone: Option<String>,
    #[validate(required, min = 0, max = 365)]
    pub lead_days: i64,
}
// [/explain:admin.suppliers.form]

// [explain:admin.suppliers.show]
impl AdminResource for Suppliers {
    type Model = Supplier;
    type Form = SupplierForm;

    fn label(&self) -> &str {
        "Supplier"
    }
    fn plural_label(&self) -> &str {
        "Suppliers"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Purchasing")
    }
    fn record_title(&self, record: &Supplier) -> String {
        record.name.clone()
    }
    // [/explain:admin.suppliers.show]
    // [explain:admin.suppliers.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::text("email", "Email"),
            Column::text("phone", "Phone"),
            Column::number("lead_days", "Lead time (days)"),
        ]
    }
    // [/explain:admin.suppliers.columns]
    // [explain:admin.suppliers.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::email("email", "Email"),
            Field::tel("phone", "Phone"),
            Field::number("lead_days", "Lead time (days)")
                .required()
                .min(0)
                .max(365)
                .default_value(7),
        ]
    }
    fn fill(&self, supplier: &mut Supplier, form: SupplierForm) {
        supplier.name = form.name;
        supplier.email = form.email.filter(|e| !e.trim().is_empty());
        supplier.phone = form.phone.filter(|p| !p.trim().is_empty());
        supplier.lead_days = form.lead_days;
    }
    // [/explain:admin.suppliers.form]
}
