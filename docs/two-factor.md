# Two-factor authentication

Two-factor authentication (2FA) adds a second step to logging in. After the password, people
type a six-digit code from an app on their phone. Someone who steals or guesses a password
still can't log in without the phone. Renox's `renox-2fa` crate adds it with one module.
[examples/bikeshop](../examples/bikeshop) makes it optional for customers and required for staff.

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [What your users see](#what-your-users-see)
- [Its pages and routes](#its-pages-and-routes)
- [Recovery codes](#recovery-codes)
- [Events and the activity log](#events-and-the-activity-log)
- [How it keeps accounts safe](#how-it-keeps-accounts-safe)
- [Changing the pages](#changing-the-pages)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **authenticator app** | An app on the phone (Google Authenticator, 1Password, Authy…) that shows a new six-digit code every 30 seconds. |
| **TOTP** | "Time-based one-time password": the standard (RFC 6238) those apps use. The app and your server share a secret; each works out the same code from it and the current time. |
| **secret** | The random key the app and the server share. It's given to the app once, through a QR code. |
| **QR code** | The square barcode the app scans to get the secret. |
| **recovery code** | A one-time code for when the phone is lost. Each user gets eight. |

> [!NOTE]
> **Coming from Laravel:** this is Fortify's two-factor authentication (what Jetstream shows on
> its profile page), as a module.

## Add it to your app

Add the crate next to `renox`, at the same version:

```toml
[dependencies]
renox = "1.2"
renox-2fa = "1.2"
```

Then add the module, next to the `Auth` module with its account page:

```rust
use renox::prelude::*;
use renox_2fa::TwoFactor;

/// The app: login pages, the account page, and two-factor authentication.
pub fn app() -> App {
    App::new()
        // `.account()` is the /account page, where users turn it on.
        .module(Auth::new().account())
        // Brings its table (a migration), its pages and its card on /account.
        .module(TwoFactor::new())
}
```

That's all. Run your migrations (`rnx migrate`, or just `rnx serve`) to add its
`two_factor` table.

## What your users see

**Turning it on.** On `/account`, a "Two-factor authentication" card says it's off, with a
**Turn on** button:

1. Renox asks for their password, unless they typed it in the last three hours.
2. A page shows a QR code. They scan it with their authenticator app (or type the key shown
   under it).
3. They type the code the app shows. If it's right, two-factor authentication is on.
4. A page shows their eight recovery codes, once, with a **Download** button.

Until step 3 is done, nothing changes at login.

**Logging in.** After the right password, the login waits. The browser goes to
`/two-factor/challenge`, which asks for the code. The right code finishes the login (and keeps
"remember me" if they ticked it). The wait lasts ten minutes; after that they start again.

**Turning it off.** The card on `/account` then says "On since …", how many recovery codes are
left, and has two buttons:

- **New recovery codes**: eight new codes; the old ones stop working.
- **Turn off**: asks "Turn off two-factor authentication?" first. It deletes the secret and the
  codes, so logging in asks for the password alone again.

Both ask for the password first, like turning it on.

## Its pages and routes

| Method | Address | Name | What it does |
|---|---|---|---|
| GET | `/two-factor/challenge` | `two-factor.challenge` | asks for the code after the password (guests) |
| POST | `/two-factor/challenge` | `two-factor.verify` | checks the code or a recovery code, then logs in |
| POST | `/two-factor/enable` | `two-factor.enable` | starts turning it on: a new secret, not active yet |
| GET | `/two-factor/setup` | `two-factor.setup` | the QR code, the key, and the form for the first code |
| POST | `/two-factor/confirm` | `two-factor.confirm` | turns it on when the code is right |
| GET | `/two-factor/recovery-codes` | `two-factor.recovery-codes` | the codes, right after they were made |
| POST | `/two-factor/recovery-codes` | `two-factor.recovery-codes.regenerate` | new recovery codes |
| DELETE | `/two-factor` | `two-factor.disable` | turns it off |

Every route except the challenge needs a logged-in user. All of them except the challenge and
the codes page also need a recent password confirmation (`require_password_confirmed`).

> [!TIP]
> Without `Auth::new().account()` there is no account page to show the card on. Your app can
> still link to `route('two-factor.enable')` (a POST form) from a page of its own.

## Recovery codes

A recovery code is for the day the phone is lost or broken. Each user gets eight, such as
`k7mqp-x2ndr`, when they turn two-factor authentication on.

- **Each works once.** At the challenge, people type one instead of the six-digit code. Case,
  spaces and the dash don't matter.
- **They're shown once.** Renox keeps only a hash of each, so it can't show them again. That's
  why the page offers a download.
- **Running low?** When two or fewer are left after a login, a message says so. The account
  card always shows how many are left.
- **New ones** replace all the old ones (the **New recovery codes** button).

## Events and the activity log

The module announces three events. Listen to them with `App::listen`, like any other event:

| Event | When | Fields |
|---|---|---|
| `TwoFactorEnabled` | a user confirmed their first code | `user_id` |
| `TwoFactorDisabled` | a user turned it off | `user_id` |
| `RecoveryCodeUsed` | a user logged in with a recovery code | `user_id`, `remaining` |

```rust
use renox::prelude::*;
use renox_2fa::{RecoveryCodeUsed, TwoFactor};

/// The app tells someone when a user is running out of recovery codes.
pub fn app() -> App {
    App::new()
        .module(Auth::new().account())
        .module(TwoFactor::new())
        .listen(|event: RecoveryCodeUsed, _state| async move {
            // `remaining` is how many codes the user has left.
            if event.remaining == 0 {
                // Here you'd mail them, or notify them in the app.
                eprintln!("user {} has no recovery codes left", event.user_id);
            }
            Ok(())
        })
}
```

When your app has the `Audit` module, these are also written to the activity log, as
`two_factor.enabled`, `two_factor.disabled` and `two_factor.recovery_code_used`. Wrong codes at
the challenge are logged like wrong passwords: `auth.login_failed`.

## How it keeps accounts safe

- **The secret is encrypted** in the database with your `APP_KEY` (`db::Encrypted`). It's
  shown only while turning it on, never again.
- **Recovery codes are stored hashed**, like passwords. Someone who reads the database can't
  use them.
- **A code works once.** Renox remembers the last 30-second step used, so a code someone saw
  over your shoulder can't be typed again.
- **Phone clocks drift**, so a code from the step just before or just after now is accepted too.
- **Wrong codes count towards the login throttle**, the same one that slows down password
  guessing. After too many, the challenge answers "too many attempts" for a while.
- **Changing it needs the password** (the last three hours), so someone at an unlocked
  computer can't turn it off.
- **A new password cancels a waiting login.** If the password changes while someone sits at
  the challenge, they start again.

> [!IMPORTANT]
> Registering logs the new user in directly, and a password reset sends people to the login
> page (where the second step applies). Neither skips the step for someone who has it on.

## Changing the pages

The pages are compiled into the crate. To change one, add a file with the same name to your
app's `resources/views/`: yours is used instead.

| File | Page |
|---|---|
| `two-factor/challenge.html` | the code after the password |
| `two-factor/setup.html` | the QR code and the first code (gets `qr`, `key`, `account`) |
| `two-factor/recovery-codes.html` | the codes, shown once (gets `codes`, `text`, `account`) |
| `two-factor/section.html` | the card on `/account` (gets `section.data`: `enabled`, `since`, `recovery_codes_left`) |

Copy the originals from the crate's `views/` folder as a start. They extend
`renox/auth/layout.html`, like Renox's login pages, and use the UI kit.

## Testing

In tests, work out the code the user's app would show with `renox_2fa::totp`:

```rust
use renox::prelude::*;
use renox::testing::TestApp;
use renox_2fa::{TwoFactor, TwoFactorCredential, totp};

/// Turns two-factor authentication on for `user`, as they would from /account.
async fn turn_on(app: &TestApp, user: &User) {
    app.acting_as(user);
    // The password was typed recently (turning it on asks for it).
    app.confirm_password();
    app.post("/two-factor/enable", &[]).await;
    // The secret the QR code holds.
    let secret = TwoFactorCredential::of(app.db(), user.id)
        .await
        .unwrap()
        .unwrap()
        .secret
        .to_string();
    // The code an authenticator app shows right now.
    let code = totp::code_at(&secret, totp::step_at(renox::db::now().timestamp())).unwrap();
    app.post("/two-factor/confirm", &[("code", code.as_str())])
        .await
        .assert_redirect("/two-factor/recovery-codes");
}
# fn main() {
#     let _ = TestApp::new(App::new().module(Auth::new().account()).module(TwoFactor::new()));
#     let _ = turn_on;
# }
```

`TestApp::travel` moves Renox's clock, and the codes follow it: use
`app.at_travelled_time(…)` around `renox::db::now()` to get the code for the moved time. The
crate's own tests (`crates/renox-2fa/tests/two_factor.rs`) cover every case above.
