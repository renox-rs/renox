use crate::db::Migration;
use crate::{Registry, Routes};

/// A self-contained piece of an application: its routes and migrations, and
/// later its jobs, policies and views.
///
/// ```
/// # use renox::prelude::*;
/// # async fn index() -> &'static str { "" }
/// # async fn show() -> &'static str { "" }
/// pub struct Products;
///
/// impl Module for Products {
///     fn name(&self) -> &'static str { "products" }
///
///     fn routes(&self) -> Routes {
///         Routes::new()
///             .get("/products", index).name("products.index")
///             .get("/products/{id}", show).name("products.show")
///     }
/// }
/// ```
pub trait Module: Send + Sync + 'static {
    /// The module's name, shown by `route:list`.
    fn name(&self) -> &'static str;

    /// The module's routes; none by default.
    fn routes(&self) -> Routes {
        Routes::new()
    }

    /// Migrations this module owns, e.g. `renox::migrations!("src/app/products/migrations")`.
    fn migrations(&self) -> &'static [Migration] {
        &[]
    }

    /// Registers the module's jobs, event listeners and scheduled tasks.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use serde::{Deserialize, Serialize};
    /// # #[derive(Serialize, Deserialize)] struct SendReceipt { order_id: i64 }
    /// # impl Job for SendReceipt { const NAME: &'static str = "send-receipt"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
    /// # #[derive(Clone)] struct OrderPlaced { order_id: i64 }
    /// # impl Event for OrderPlaced {}
    /// # async fn close_day(_: AppState) -> Result { Ok(()) }
    /// # struct Orders;
    /// # impl Module for Orders {
    /// # fn name(&self) -> &'static str { "orders" }
    /// fn register(&self, app: &mut Registry) {
    ///     app.job::<SendReceipt>()
    ///         .listen(|e: OrderPlaced, state| async move {
    ///             state.dispatch(SendReceipt { order_id: e.order_id }).await?;
    ///             Ok(())
    ///         });
    ///     app.schedule().daily_at("02:00", "close-day", close_day);
    /// }
    /// # }
    /// ```
    fn register(&self, _app: &mut Registry) {}
}
