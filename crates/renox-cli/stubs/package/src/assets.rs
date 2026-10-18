//! The files the package serves as they are (`Registry::asset`).

use renox::prelude::*;

/// Registers the package's stylesheet. The address carries the version: raise
/// it with the file's content, so a new release gets a new address.
pub(crate) fn register(app: &mut Registry) {
    app.asset(
        "/___lib__/__lib__-0.1.0.css",
        "text/css; charset=utf-8",
        include_bytes!("../public/__lib__.css"),
    );
}

#[cfg(test)]
mod tests {
    use renox::prelude::*;
    use renox::testing::TestApp;

    use crate::__Struct__;

    #[renox::test]
    async fn the_stylesheet_is_served() {
        let app = TestApp::new(App::new().module(__Struct__)).await;
        app.get("/___lib__/__lib__-0.1.0.css")
            .await
            .assert_ok()
            .assert_see(".__lib__-entry");
    }
}
