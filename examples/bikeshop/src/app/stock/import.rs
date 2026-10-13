//! A supplier's price list, loaded from a CSV file with `renox::import`.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `POST /staff/suppliers/{supplier}/import` | `stock.suppliers.import` | `purchasing.manage` |
//! | `GET /staff/suppliers/template.csv` | `stock.suppliers.template` | `purchasing.manage` |
//!
//! Each row (`sku,product,size,colour,cost,price,barcode`) is checked **as a
//! form** ([`PriceRow`]'s `#[derive(Validate)]` rules, in the user's
//! language), then written in **its own savepoint** of one transaction
//! ([`write_row`]): it updates the variant with that SKU (cost, and price
//! and barcode when given) or creates it under the product named by its
//! slug, and keeps the supplier's cost in their price list
//! (`supplier_items`). A row the database refuses, or that names a product
//! that doesn't exist, undoes only its own writes and is reported with its
//! row number in the `ImportReport`; the others are saved.
//!
//! **Large files** (over [`LARGE_IMPORT`] rows) don't make the person
//! wait: the file goes to private storage and an [`ImportPriceList`] job
//! runs the same import in the background, then mails the report.

use renox::db::{Transaction, sql};
use renox::import::{Import, ImportReport};
use renox::mail::Mail;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::model::Supplier;
use crate::app::catalog::model::Product;

/// The price list's columns, in the template's order.
pub const COLUMNS: [&str; 7] = [
    "sku", "product", "size", "colour", "cost", "price", "barcode",
];

/// Files with more rows than this are imported by a queued job.
pub const LARGE_IMPORT: usize = 200;

/// One row of a price list, checked like a form.
#[derive(Deserialize, Validate, Debug, Clone)]
pub struct PriceRow {
    #[validate(required, max = 40)]
    pub sku: String,
    /// The product's slug, for a SKU the shop doesn't have yet.
    #[validate(max = 160)]
    pub product: Option<String>,
    #[validate(max = 20)]
    pub size: Option<String>,
    #[validate(max = 30)]
    pub colour: Option<String>,
    /// The supplier's price to us, in whole units as a price list writes
    /// it (`12.50`); stored in the smallest unit of `APP_CURRENCY` (cents).
    #[validate(required, min = 0)]
    pub cost: Option<f64>,
    /// Our selling price (whole units); needed for a new SKU, kept as it
    /// is when blank.
    #[validate(min = 0)]
    pub price: Option<f64>,
    #[validate(max = 40)]
    pub barcode: Option<String>,
}

/// Writes one row in `tx` (the import's savepoint for this row): see the
/// module docs. An unknown product or a new SKU without a price is a
/// `BadRequest`, reported on the row.
pub async fn write_row(tx: &mut Transaction, supplier_id: i64, row: PriceRow) -> Result {
    let now = renox::db::now();
    let sku = row.sku.trim().to_owned();
    let barcode = row.barcode.filter(|b| !b.trim().is_empty());
    let currency = crate::money::currency();
    let cost = crate::money::from_whole(row.cost.unwrap_or(0.0), &currency);
    let price = row.price.map(|p| crate::money::from_whole(p, &currency));
    let existing: Option<i64> = sql("SELECT id FROM product_variants WHERE sku = ?")
        .bind(&sku)
        .scalar_optional(&mut *tx)
        .await?;
    let variant_id = match existing {
        Some(id) => {
            sql(
                "UPDATE product_variants SET cost = ?, price = COALESCE(?, price), \
                 barcode = COALESCE(?, barcode), updated_at = ? WHERE id = ?",
            )
            .bind(cost)
            .bind(price)
            .bind(barcode)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            id
        }
        None => {
            let slug = row.product.unwrap_or_default();
            let Some(product) = Product::query()
                .where_(Product::SLUG.eq(slug.trim()))
                .first(&mut *tx)
                .await?
            else {
                return Err(Error::BadRequest(format!(
                    "No product \"{}\" for the new SKU {sku}.",
                    slug.trim()
                )));
            };
            let Some(price) = price else {
                return Err(Error::BadRequest(format!(
                    "The new SKU {sku} needs a price."
                )));
            };
            sql(
                "INSERT INTO product_variants (product_id, sku, size, colour, price, cost, \
                 reorder_level, barcode, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
            )
            .bind(product.id)
            .bind(&sku)
            .bind(row.size.filter(|s| !s.is_empty()))
            .bind(row.colour.filter(|c| !c.is_empty()))
            .bind(price)
            .bind(cost)
            .bind(barcode)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            sql("SELECT id FROM product_variants WHERE sku = ?")
                .bind(&sku)
                .scalar(&mut *tx)
                .await?
        }
    };
    sql(
        "INSERT INTO supplier_items (supplier_id, variant_id, cost, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (supplier_id, variant_id) \
         DO UPDATE SET cost = excluded.cost, updated_at = excluded.updated_at",
    )
    .bind(supplier_id)
    .bind(variant_id)
    .bind(cost)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Runs the import of `data` for `supplier_id`, messages in `lang`.
// [explain:stock.suppliers.show.import]
pub async fn run(
    state: &AppState,
    supplier_id: i64,
    data: &[u8],
    lang: &Lang,
) -> Result<ImportReport> {
    Import::csv(data)
        .lang(lang)
        .run(state, move |tx, row: PriceRow| {
            Box::pin(async move { write_row(tx, supplier_id, row).await })
        })
        .await
}
// [/explain:stock.suppliers.show.import]

/// The import form: one CSV file.
#[derive(Deserialize, Validate)]
pub struct ImportForm {
    #[validate(required, mimes(&["csv", "txt"]))]
    pub file: Option<Upload>,
}

/// How many data rows a file has (its lines, less the heading).
pub fn rows_in(data: &[u8]) -> usize {
    let lines = data
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(u8::is_ascii_whitespace))
        .count();
    lines.saturating_sub(1)
}

/// `POST /staff/suppliers/{supplier}/import` (`stock.suppliers.import`):
/// a small file is imported now (the kit's import sheet shows the
/// report); a large one is queued and its report mailed.
pub async fn import(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Found(supplier): Found<Supplier>,
    Valid(form): Valid<ImportForm>,
) -> Result<Response> {
    // [explain:stock.suppliers.show.import]
    let file = form.file.ok_or(Error::NotFound)?;
    if rows_in(file.bytes()) > LARGE_IMPORT {
        let key = format!(
            "imports/price-list-{}-{}.csv",
            supplier.id,
            &renox::random_token()[..16]
        );
        state.storage.put(&key, file.bytes().clone()).await?;
        state
            .dispatch(ImportPriceList {
                supplier_id: supplier.id,
                key,
                user_id: user.id,
                locale: lang.locale.clone(),
            })
            .await?;
        return Ok((
            Toast::success(lang.t("stock.import.queued", &[])),
            HxRefresh,
            String::new(),
        )
            .into_response());
    }
    // [/explain:stock.suppliers.show.import]
    Ok(run(&state, supplier.id, file.bytes(), &lang)
        .await?
        .into_response())
}

/// `GET /staff/suppliers/template.csv` (`stock.suppliers.template`): the
/// columns, for the import sheet's "Download a template".
pub async fn template() -> renox::Download {
    renox::import::template("price-list.csv", &COLUMNS)
}

/// A large price list, imported in the background: the stored file, read
/// and imported with the same rules, then the report is mailed to whoever
/// sent it and the file deleted.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImportPriceList {
    pub supplier_id: i64,
    /// The file's key in the app's (private) storage.
    pub key: String,
    pub user_id: i64,
    /// The sender's language, for the rules' messages and the mail.
    pub locale: String,
}

// [explain:stock.suppliers.show.job]
impl Job for ImportPriceList {
    const NAME: &'static str = "stock-import-price-list";

    async fn handle(self, ctx: JobContext) -> Result {
        let state = &ctx.state;
        let Some(data) = state.storage.get(&self.key).await? else {
            return Ok(()); // already done by an earlier attempt
        };
        let lang = state.lang(&self.locale);
        let report = run(state, self.supplier_id, &data, &lang).await?;
        if let Some(user) = User::find(&state.db, self.user_id).await? {
            let supplier = Supplier::find(&state.db, self.supplier_id).await?;
            state
                .queue_mail(report_mail(
                    state,
                    &lang,
                    &user,
                    supplier.as_ref(),
                    &report,
                )?)
                .await?;
        }
        state.storage.delete(&self.key).await?;
        Ok(())
    }
}
// [/explain:stock.suppliers.show.job]

/// The mail with an import's report.
pub fn report_mail(
    state: &AppState,
    lang: &Lang,
    user: &User,
    supplier: Option<&Supplier>,
    report: &ImportReport,
) -> Result<Mail> {
    let failed: Vec<_> = report
        .failed
        .iter()
        .take(100)
        .map(|f| json!({ "row": f.row, "messages": f.messages().join(" ") }))
        .collect();
    let table: Vec<[String; 2]> = report
        .failed
        .iter()
        .take(100)
        .map(|f| {
            [
                lang.t(
                    "stock.mail.import.row",
                    &[("row", &f.row as &dyn std::fmt::Display)],
                ),
                f.messages().join(" "),
            ]
        })
        .collect();
    state.mail_view_in(
        &lang.locale,
        &user.email,
        lang.t(
            "stock.mail.import.subject",
            &[("supplier", &supplier.map(|s| s.name.clone()).unwrap_or_default() as &dyn std::fmt::Display)],
        ),
        "mail/stock/import_report",
        context! {
            summary => report.summary(),
            imported => report.imported,
            failed,
            table,
            more => report.failed.len().saturating_sub(100),
            url => supplier.map(|s| crate::app::rentals::link(state, "stock.suppliers.show", Some(s.id))).transpose()?,
        },
    )
}
