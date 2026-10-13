//! The public front page (`resources/views/home/index.html`, in
//! `layouts/public.html`), made of the app's page patterns
//! (`resources/views/components/`). Signed-in people go on to `/dashboard`.

use renox::prelude::*;

use super::roles::ROLES;

pub struct Home;

impl Module for Home {
    fn name(&self) -> &'static str {
        "home"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/", index).name("home")
    }
}

/// The page, with the figures its strip shows.
async fn index(State(db): State<Db>) -> Result<View> {
    let people = User::query().count(&db).await?;
    Ok(view(
        "home/index.html",
        context! { people, roles => ROLES.len() },
    ))
}
