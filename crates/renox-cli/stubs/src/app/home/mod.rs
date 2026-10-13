use renox::prelude::*;

pub struct Home;

impl Module for Home {
    fn name(&self) -> &'static str {
        "home"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/", index).name("home")
    }
}

async fn index() -> View {
    view("home/index.html", context! {})
}
