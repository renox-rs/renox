# Social login

Social login lets people sign in with an account they already have, such as Google or GitHub,
instead of making up another password. Renox's `renox-oauth` crate adds "Continue with Google"
and "Continue with GitHub" to the login and register pages, linked to your app's `users`
table, with one module. [examples/bikeshop](../examples/bikeshop) offers it to its customers.

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [What your users see](#what-your-users-see)
- [Which account a sign-in logs into](#which-account-a-sign-in-logs-into)
- [Users without a password](#users-without-a-password)
- [Its routes](#its-routes)
- [Events and the activity log](#events-and-the-activity-log)
- [How it keeps accounts safe](#how-it-keeps-accounts-safe)
- [Another provider](#another-provider)
- [Changing the pages](#changing-the-pages)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **provider** | The service people sign in with: Google, GitHub, … |
| **OAuth 2.0** | The standard behind "Sign in with …": your app sends the browser to the provider, the person approves, and the provider sends the browser back with a one-time **code** that your server exchanges for a **token**, then for the person's profile. |
| **client id and secret** | What the provider gives your app when you register it in its console. The secret stays on your server. |
| **redirect URI** (callback URL) | Where the provider sends the browser back: `APP_URL/auth/google/callback`. You enter it in the provider's console. |
| **state** | A random value sent to the provider and checked when the browser comes back, so a sign-in can't be started by someone else. |
| **PKCE** | "Proof Key for Code Exchange" (RFC 7636): a second random value, of which only a hash goes to the provider, so a stolen code is useless. |
| **linked account** | A provider account (one Google account) tied to one of your users: a row in `oauth_accounts`. |

> [!NOTE]
> **Coming from Laravel:** this is Socialite, with the parts you write around it in Laravel
> (the routes, matching users, the account page) done for you.

## Add it to your app

Add the crate next to `renox`, at the same version:

```toml
[dependencies]
renox = "1.1"
renox-oauth = "1.1"
```

Then add the module, next to the `Auth` module:

```rust
use renox::prelude::*;
use renox_oauth::OAuth;

/// The app: login pages, the account page, and social login.
pub fn app() -> App {
    App::new()
        // `.account()` is the /account page, where users link and unlink providers.
        .module(Auth::new().account())
        // Brings its table (a migration), its routes, the buttons and its card on /account.
        .module(OAuth::new().google().github())
}
```

Each provider reads its credentials from your configuration (`.env` or the environment) when a
request needs them:

```sh
APP_URL=https://shop.example.com     # the callback URLs are built from it
GOOGLE_CLIENT_ID=…
GOOGLE_CLIENT_SECRET=…
GITHUB_CLIENT_ID=…
GITHUB_CLIENT_SECRET=…
```

A provider whose two values are missing is off: no button, and its routes answer 404. So the
same code runs on a laptop without credentials. To give them in code instead, use
`.provider(Google::new(client_id, client_secret))` (and `GitHub::new`).

Make the clients in the providers' consoles:

- **Google**: Google Cloud console → APIs & Services → Credentials → Create credentials →
  OAuth client ID → Web application. Authorized redirect URI: `APP_URL/auth/google/callback`.
- **GitHub**: Settings → Developer settings → OAuth Apps → New OAuth App. Authorization callback
  URL: `APP_URL/auth/github/callback`.

Run your migrations (`rnx migrate`, or just `rnx serve`) to add the `oauth_accounts` table.

## What your users see

**On the login and register pages**, under the form: "Or continue with", then a button per
provider. Renox's built-in pages include `renox/auth/login_options.html`, which this module
provides; a page of your own includes it the same way (see [Changing the
pages](#changing-the-pages)).

**Signing in.** The button goes to the provider, where they approve (the first time) and come
straight back, logged in. Where they land is where the login page would send them: the page
that asked them to log in, else `Auth::redirect_to`, else `home`.

**On `/account`** (with `Auth::new().account()`), a "Linked accounts" card lists the providers:
**Link** for the ones not linked yet (a round trip to the provider), and **Unlink** (with a
question first) for the linked ones, with the address the provider gave.

## Which account a sign-in logs into

When the browser comes back from the provider with a person's profile:

| Situation | What happens |
|---|---|
| This provider account is linked to a user | That user is logged in, whatever address the provider gives now. |
| Not linked, and the provider didn't **verify** the address (or gave none) | Refused: nothing is linked and no account is made. |
| Not linked, a user has this verified address, and **their address is verified too** | The provider account is linked to them, and they're logged in. |
| Not linked, a user has this address, but **their own address isn't verified** | Refused, with "log in with your password, then link Google from your account page". |
| Not linked, nobody has the address, registration is open | A new user (no password, address verified) is made, linked and logged in. |
| Not linked, nobody has the address, `Auth::without_registration()` | Refused: "no account uses this address". |
| A logged-in user comes back (they clicked **Link**) | The provider account is linked to them, unless it's linked to someone else or they already have one at this provider. The address doesn't matter: they proved both sides. |

New accounts go through your `Auth::on_registered` hook, which can read `name`, `email` and
`provider` from its `Registration` (`registration_rules` don't run: there is no form). The
`Registered` event fires as after `/register`.

**The second login step applies.** When a module such as `renox-2fa` asks a user for a code
after their password, a social login asks for it too: the login waits at the challenge, as after
the password.

## Users without a password

A user made by a social login has no password: their `password` column is empty, which no
typed password ever matches, and `user.has_password()` is `false`. They log in with the
provider. On the account page:

- The password card is **Set a password**, without the "current password" field.
- **Log out other devices** and **Delete account** don't ask for a password. They need a recent
  confirmation instead: logging in counts (for three hours); after that, the browser goes to
  `/confirm-password`, which shows "Or confirm it's you with" and the providers **linked to
  them**. Coming back from one counts as typing the password. Routes behind
  `require_password_confirmed` (turning on two-factor authentication) work the same way.
- **Unlinking their only provider is refused**, so nobody is left without a way to log in. They
  set a password first, or link a second provider.

"Forgot your password?" works for them too: the link goes to their verified address.

## Its routes

| Method | Address | Name | What it does |
|---|---|---|---|
| GET | `/auth/{provider}/redirect` | `oauth.redirect` | starts a sign-in: to the provider, with `state` and the PKCE challenge |
| GET | `/auth/{provider}/callback` | `oauth.callback` | back from the provider: logs in, links, or confirms |
| DELETE | `/auth/{provider}` | `oauth.unlink` | unlinks the provider from the logged-in user |

`oauth.redirect` logs in a guest and links for a logged-in user; with `?intent=confirm` a
logged-in user confirms who they are instead, and with `?remember=1` a guest's login lasts as
long as with "remember me". `oauth.unlink` needs a logged-in user. A link in your own page:

```html
<a href="{{ route('oauth.redirect', 'google') }}" hx-boost="false">Continue with Google</a>
```

(`hx-boost="false"`, because the provider's page can't be loaded with htmx.)

## Events and the activity log

| Event | When | Fields |
|---|---|---|
| `AccountLinked` | a provider account was linked to a user (on the account page, or at a first sign-in) | `user_id`, `provider` |
| `AccountUnlinked` | a user unlinked one | `user_id`, `provider` |
| `LoggedInWith` | a user logged in with a provider | `user_id`, `provider`, `second_step` |

`second_step` is `true` when the login waits for a second step (two-factor authentication). The
`Auth` module's own events fire too: `Registered` for a new account, and `LoggedIn` once the
user is in.

```rust
use renox::prelude::*;
use renox_oauth::{LoggedInWith, OAuth};

/// The app counts the logins of each provider.
pub fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(OAuth::new().google().github())
        .listen(|event: LoggedInWith, state| async move {
            let key = format!("logins:{}", event.provider);
            state.cache.increment(&key, 1).await?;
            Ok(())
        })
}
```

When your app has the `Audit` module, they're also written to the activity log as
`oauth.linked`, `oauth.unlinked` and `oauth.login` (with the provider in `data`).

## How it keeps accounts safe

- **`state` is checked and used once.** It's random, kept in the visitor's session, and compared
  in constant time when the browser comes back; the first callback removes it, right or wrong.
  A callback from another browser, after ten minutes, or for another provider is refused. With
  the default cookie sessions, an old copy of the cookie would still hold it, but the provider
  accepts each code once.
- **PKCE (S256).** The verifier never leaves your server; only its SHA-256 goes in the
  authorization URL. A code intercepted on its way back can't be exchanged without it.
- **Never by an unverified address.** Someone can type any address into some providers.
  Accounts are linked or made only by an address the provider verified, and never linked to an
  account whose own address nobody verified (someone may have registered it in another
  person's name, with a password they know).
- **Tokens aren't stored.** The token is used once, to read the profile, then dropped.
  `oauth_accounts` holds the provider's user id and what the profile said (address, name,
  picture's URL).
- **A provider account links to one user**, and a user has one account per provider (the
  table's unique keys).
- **Unlinking never locks a user out** (see above).
- **Failures say little.** The visitor sees "try again"; the reason is logged at `warn`.

## Another provider

Adding a provider is one impl of the `Provider` trait: its name, its endpoints, its scopes and
how to read the profile. The authorization URL, `state`, PKCE and the code exchange (the
standard request; override `Provider::exchange` for one that differs) are done for you.

```rust
use renox::http::Http;
use renox::prelude::*;
use renox_oauth::{BoxFuture, Credentials, OAuth, Profile, Provider, Token};

/// Sign in with GitLab.
struct GitLab {
    credentials: Credentials,
}

impl Provider for GitLab {
    fn name(&self) -> &str {
        "gitlab" // in URLs and the table: never change it once used
    }
    fn label(&self) -> &str {
        "GitLab" // on the buttons
    }
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }
    fn authorize_endpoint(&self) -> &str {
        "https://gitlab.com/oauth/authorize"
    }
    fn token_endpoint(&self) -> &str {
        "https://gitlab.com/oauth/token"
    }
    fn scopes(&self) -> &[&str] {
        &["read_user"]
    }
    fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>> {
        Box::pin(async move {
            let me: renox::serde_json::Value = http
                .get("https://gitlab.com/api/v4/user")
                .bearer(&token.access_token)
                .send()
                .await?
                .error_for_status()?
                .json()?;
            let mut profile = Profile::new(me["id"].to_string());
            if let Some(email) = me["email"].as_str() {
                // GitLab only shows a confirmed address as `email`.
                profile = profile.email(email, me["confirmed_at"].is_string());
            }
            Ok(profile.name(me["name"].as_str().map(str::to_owned)))
        })
    }
}

pub fn app() -> App {
    App::new().module(Auth::new()).module(OAuth::new().google().provider(GitLab {
        // GITLAB_CLIENT_ID and GITLAB_CLIENT_SECRET.
        credentials: Credentials::from_config("GITLAB"),
    }))
}
```

Use the app's HTTP client (`http`, which is `state.http`) for every call: tests fake it.

## Changing the pages

The templates are compiled into the crate. To change one, add a file with the same name to
your app's `resources/views/`: yours is used instead.

| File | What it is |
|---|---|
| `renox/auth/login_options.html` | the buttons under the login, register and confirm-password forms (gets `page`: `login`, `register` or `confirm`, and `oauth_providers`: `name`, `label`, `url`) |
| `oauth/section.html` | the card on `/account` (gets `section.data`: `providers` with `name`, `label`, `link_url`, `linked`, `email`, `since`; `has_password`, `can_unlink`, `set_password_url`) |

A login page of your own shows the buttons with:

```html
{% with page="login" %}{% include "renox/auth/login_options.html" %}{% endwith %}
```

Every view gets `oauth_providers` (the providers that have credentials), for buttons made your
own way.

## Testing

Tests never reach the providers: `TestApp::fake_http` answers the code exchange and the
profile. Start a sign-in, read `state` from the provider URL, and come back with it:

```rust
use renox::http::FakeResponse;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_oauth::{Google, OAuth};

/// Signs in with a fake Google account, as a browser would.
async fn sign_in_with_google(app: &TestApp, email: &str) {
    let http = app.fake_http();
    http.on(
        "POST https://oauth2.googleapis.com/token",
        FakeResponse::json(200, json!({ "access_token": "token" })),
    );
    http.on(
        "https://openidconnect.googleapis.com/v1/userinfo",
        FakeResponse::json(200, json!({ "sub": "1", "email": email, "email_verified": true })),
    );
    // To Google: the address holds `state`.
    let response = app.get("/auth/google/redirect").await;
    let to_google = response.header("location").unwrap();
    let state = to_google
        .split(['?', '&'])
        .find_map(|pair| pair.strip_prefix("state="))
        .unwrap();
    // And back.
    app.get(&format!("/auth/google/callback?code=a-code&state={state}"))
        .await
        .assert_redirect("/");
}

async fn demo() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            // Credentials in code: tests don't read `.env`.
            .module(OAuth::new().provider(Google::new("id", "secret"))),
    )
    .await;
    sign_in_with_google(&app, "nia@example.com").await;
    app.assert_authenticated(None);
}
# fn main() {
#     let _ = demo;
# }
```

The crate's own tests (`crates/renox-oauth/tests/oauth.rs`) cover every case above.
