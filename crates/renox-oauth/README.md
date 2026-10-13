# renox-oauth

Social login for [Renox](https://github.com/renox-rs/renox) apps: "Continue with Google" and
"Continue with GitHub" on the login and register pages, linked to the app's `users` table
(Laravel's Socialite, with the routes and the account page done for you).

```rust
use renox::prelude::*;
use renox_oauth::OAuth;

App::new()
    .module(Auth::new().account()) // the account page, where users link and unlink providers
    .module(OAuth::new().google().github()) // GOOGLE_CLIENT_ID/_SECRET, GITHUB_CLIENT_ID/_SECRET
```

What it adds:

- `GET /auth/{provider}/redirect` and `/callback`: the authorization code flow with PKCE (S256)
  and a single-use `state` bound to the session;
- at a first sign-in, a link to the account with the same **verified** address (never by an
  unverified one), or a new account without a password when registration is open; the second
  login step (two-factor authentication) still applies;
- a card on `/account` to link and unlink providers (never the last way to log in), and
  "confirm it's you" with a linked provider for users without a password;
- the `oauth_accounts` table (no tokens stored), and the events `AccountLinked`,
  `AccountUnlinked` and `LoggedInWith`, recorded in the activity log when the app has the
  `Audit` module;
- Google and GitHub; another provider is one impl of the `Provider` trait.

The guide is [docs/oauth.md](https://github.com/renox-rs/renox/blob/main/docs/oauth.md);
examples/bikeshop uses it (Google and GitHub, when their keys are set). Versioned with `renox`: use the same version for both.
