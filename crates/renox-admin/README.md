# renox-admin

An admin panel for [Renox](https://github.com/renox-rs/renox) apps, generated from their
models (Laravel's Filament resources). Declare each model once, as a resource: its list's
columns, its form's fields and its policy. The panel gives it the rest, on the UI kit and
`renox::grid`.

```rust
use renox::prelude::*;
use renox_admin::Admin;

App::new()
    .module(Auth::new())
    .module(
        Admin::new()
            .title("Back office")
            .authorize(|user| user.has_role("staff")) // nobody gets in until you say who
            .resource(ProductResource)                // impl AdminResource for ProductResource
            .resource(CustomerResource),
    )
```

What each resource gets:

- a list (`/admin/products`): a data grid with search, a filter in each heading, named
  filters as tabs, sorting, pages, the user's own columns, and CSV, print and Excel (the
  `xlsx` feature) exports;
- create and edit pages built from its fields (text, email, password, number, money, date,
  date-time, select, checkbox, toggle, belongs-to), checked by the form type's own
  `Validate` rules, with errors under their fields (htmx) and a toast after saving;
- a view page (the kit's infolist), and delete with a confirmation; with soft deletes, a
  trash to restore from or delete for good;
- bulk actions (delete, and the resource's own `AdminAction`s, which can ask for input in a
  sheet) and row actions;
- an after-save hook that gets the user, child rows managed as tabs of a record (relation
  managers: has-many and pivot), edit-only resources, slots in the layout for the app's content,
  and every word of the panel translatable (`renox.admin.*`);
- the model's `Policy` asked before every page and button (`viewAny`, `view`, `create`,
  `update`, `delete`, `deleteAny`, `restore`, `forceDelete`, …);
- a sidebar with every resource the user may see, and a dashboard with their counts;
- settings pages (`Admin::settings::<Company>("Company")`): a form for a group of settings kept
  in the database (`renox::settings`), checked by the group's rules, with sealed groups never
  printed.

The pages are templates (`renox-admin/*.html`) an app replaces with files of the same name.
The guide is [docs/admin.md](https://github.com/renox-rs/renox/blob/main/docs/admin.md);
examples/bikeshop puts its catalogue, workshop, suppliers and stores in a panel made with it. Versioned with `renox`: use the same
version for both.
