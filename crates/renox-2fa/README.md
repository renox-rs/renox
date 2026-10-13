# renox-2fa

Two-factor authentication for [Renox](https://github.com/renox-rs/renox) apps: after the
password, a six-digit code from an authenticator app (TOTP, RFC 6238: Google Authenticator,
Authy, 1Password, …), or one of eight one-time recovery codes for a lost phone.

```rust
use renox::prelude::*;
use renox_2fa::TwoFactor;

App::new()
    .module(Auth::new().account()) // the account page, where users turn it on
    .module(TwoFactor::new())
```

What it adds:

- a card on `/account`: turn it on (password, then a QR code to scan and a code to confirm),
  new recovery codes, turn it off;
- the challenge after the password (a code, or a recovery code), counted by the login throttle;
- the `two_factor` table, with the secret encrypted with `APP_KEY` and the recovery codes hashed;
- the events `TwoFactorEnabled`, `TwoFactorDisabled` and `RecoveryCodeUsed`, recorded in the
  activity log when the app has the `Audit` module.

The guide is [docs/two-factor.md](https://github.com/renox-rs/renox/blob/main/docs/two-factor.md);
examples/bikeshop uses it (optional for customers, required for staff). Versioned with `renox`: use the same version for both.
