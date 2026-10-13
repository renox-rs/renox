# examples/hello

A guestbook that touches many Renox features in one file: named routes, a layout, sessions
and flash messages, CSRF, htmx fragments, validation with old input, a model, migrations, a
seeder, pagination, login and registration, the account page, an event whose listener queues a job, a scheduled
task, an app command, uploads and two languages. It is also the app used for live and browser
testing. Read it for a quick tour; read [examples/bikeshop](../bikeshop) for how a real app is
structured.

```bash
cd examples/hello
cp .env.example .env             # APP_LOCALE=en: English unless the browser prefers Spanish
rnx doctor                       # what is missing, with the fix
rnx key:generate                 # writes APP_KEY to .env
cargo run -- migrate
cargo run -- db:seed             # optional: 30 fake entries
cargo run                        # http://127.0.0.1:3000
```

Other things to try: `/hello/<name>`, `/language/en` and `/language/es` to switch language, and
`cargo run -- entries:prune --days 7` (it asks before deleting; `--force` doesn't, and
`cargo run -- entries:prune --help` lists the options).

## What's where

| Feature | Where |
|---|---|
| Everything in Rust: the model and factory, the form and its rules (`#[derive(Validate)]`), `detect_locale`, the `EntryPosted` event, the `ThankGuest` job, the `entries:prune` command (a clap `AppCommand` that confirms with `renox::prompt`), the every-minute task, the handlers, `app()` | [src/lib.rs](src/lib.rs) |
| The page: the form posted with htmx and Alpine (its reset in `Alpine.data`, so it also works under `CSP=strict`), the `entries` block swapped on post and on page links | [resources/views/guestbook/index.html](resources/views/guestbook/index.html) |
| Texts in English and Spanish; the Spanish file also translates validation messages, field names, the login and account pages and the UI kit's labels | [resources/lang](resources/lang) |
| The entries table, then a second migration adding `photo` | [migrations](migrations) |
| The layout: the logged-in user's name links to `/account` (`route('account.show')`) | [resources/views/layouts/app.html](resources/views/layouts/app.html) |
| Every setting, with comments (the same as `rnx new`'s, with the guestbook's values) | [.env.example](.env.example) |

## Things worth copying

- **Rules as attributes.** `EntryForm` derives `Validate`: `#[validate(required, max = 50)]` on
  the name, `#[validate(image, max = 2048)]` on the optional photo.
- **The visitor's language.** `App::detect_locale()` gives a visitor their browser's language
  (English or Spanish) until they pick one with `/language/{locale}`; `APP_LOCALE=en` is the
  fallback for other languages. Renox's own texts are English; `resources/lang/es.json`
  translates the ones the guestbook shows (`renox.validation.*`, `renox.auth.*`, `ui.*`).

- **Pages written with tags.** The form, the entries list and the layout's menu and buttons are
  `<rx-form>`, `<rx-list>`, `<rx-menu>`, `<rx-link-button>`, with `rx-if` / `rx-for` on plain
  elements instead of `{% if %}` / `{% for %}` blocks. `rnx view:check` loads every template and
  reports a wrong tag, attribute or route name with the file and line.

- **The app is a library.** `app()` lives in `src/lib.rs` and `main.rs` only runs it, so
  `tests/` can boot the same app.
- **One handler for htmx and plain posts.** `index` returns
  `view(...).fragment("entries")`, so htmx requests get only that block. `store` answers htmx
  with the fragment and an `HxTrigger`, and a plain post with a flash message and `Back`.
- **Validation errors need no code in the handler.** `Valid<EntryForm>` sends plain posts back
  with errors and old input, and answers htmx posts with 422.
- **Account pages for free.** `Auth::new().account()` adds `/account`: edit the profile,
  change the password, log out other devices, delete the account.
- **An optional photo.** `photo: Option<Upload>` with `#[validate(image, max = 2048)]` (KB; the content is sniffed), stored with
  `store_public`.

## Tests

```bash
cargo test -p hello
```

Tests don't read `.env`, so [tests/guestbook.rs](tests/guestbook.rs) sets the locale to `en`
itself; one test sends `Accept-Language: es` and gets the Spanish page until `/language/en` is
chosen, and another checks the Spanish validation messages and account page. The prune test moves the clock forty days with `app.travel(..)` between two entries,
then runs the command on the moved clock with the answers typed for it:
`app.at_travelled_time(renox::prompt::answering(["no"], app.kernel().call("entries:prune", ..)))`.
