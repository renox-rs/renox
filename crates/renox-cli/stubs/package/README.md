# __crate__

__Title__: a reusable Renox module, made with `rnx make:package __crate__`.

Add it to an app (`Cargo.toml`: `__crate__ = { path = "…" }`, or its version once
published) and register the module:

```
use renox::prelude::*;

# let _ =
App::new().module(__lib__::__Struct__)
# ;
```

The page is at `/__crate__`. The module owns:

- `migrations/`, run with the app's (`rnx migrate`); their names carry the package
  name, so two packages can't clash.
- `views/__crate__/`, compiled in. The app overrides a page by putting a file of the
  same name under its views directory (`resources/views/__crate__/index.html`).
- `src/texts.rs`, the English texts (`__lib__.*`). The app changes or translates them
  in `resources/lang/{locale}.json`.
