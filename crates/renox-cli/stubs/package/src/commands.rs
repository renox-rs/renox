//! The commands the package adds to the app binary (`my-app __crate__:status`).

use renox::clap;
use renox::command::AppCommand;
use renox::prelude::*;

use crate::Entry;

/// Print how many entries the package holds.
#[derive(clap::Parser)]
#[command(name = "__crate__:status")]
pub(crate) struct Status {}

impl AppCommand for Status {
    /// `my-app __crate__:status`; `--help` lists the arguments.
    async fn run(self, state: AppState) -> Result {
        println!("{}", summary(&state.db).await?);
        Ok(())
    }
}

/// The line the command prints.
async fn summary(db: &Db) -> Result<String> {
    let count = Entry::query().count(db).await?;
    Ok(format!("__Title__: {count} entries"))
}

/// Registers the package's commands.
pub(crate) fn register(app: &mut Registry) {
    app.typed_command::<Status>();
}

#[cfg(test)]
mod tests {
    use renox::prelude::*;
    use renox::testing::TestApp;

    use super::summary;
    use crate::__Struct__;

    #[renox::test]
    async fn status_counts_the_entries() {
        let app = TestApp::new(App::new().module(__Struct__)).await;
        assert_eq!(summary(app.db()).await.unwrap(), "__Title__: 0 entries");
    }
}
