//! "About this page" entries for the accounts area (see `crate::explain`).
//!
//! The sign-in pages and the account page come from Renox's `Auth` module
//! (`Auth::new().account()` in `src/lib.rs`); the bike shop only gives them
//! its look (`resources/views/renox/auth/layout.html`). Their entries live
//! here, with the customer account pages #238 adds.

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

/// The sign-in pages and the account page are Renox's `Auth` module.
const AUTH_MODULE: Feature = Feature {
    api: "Auth module",
    why: "Login, registration, password reset, email verification and the account \
          page are Renox's, switched on by one builder in `src/lib.rs` \
          (`Auth::new().account().verify_email()…`), so the shop writes none of that \
          security-sensitive code itself.",
};

/// The shop's own look for Renox's pages.
const AUTH_LAYOUT: Feature = Feature {
    api: "View overrides (renox/auth/layout.html)",
    why: "A file named like a built-in view replaces it: the shop's \
          `renox/auth/layout.html` gives every sign-in page the brand, the motion \
          and this panel, without copying the pages themselves.",
};

/// The CSRF token in every form of these pages.
const CSRF: Feature = Feature {
    api: "CSRF protection",
    why: "The form carries the session's token (`csrf_field()`), checked by the CSRF \
          middleware before the handler runs, so another site can't post it for you \
          (a hidden form elsewhere logging you into the attacker's account, say).",
};

/// The social login buttons under the sign-in forms.
const OAUTH_BUTTONS: Feature = Feature {
    api: "renox-oauth",
    why: "\"Continue with Google\" and \"Continue with GitHub\" appear under the form \
          when their keys are set (`GOOGLE_CLIENT_ID`…), and not at all otherwise, so \
          the same code runs on a laptop with no keys. The plugin does the OAuth \
          round trip with PKCE and a single-use `state`, and links only by an address \
          the provider verified.",
};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "login",
            path: "/login",
            title: "Log in",
            purpose: "Where customers and staff sign in. After the password (and the \
                      two-factor code, for those who have it) everyone goes back to the \
                      page they were trying to open, else to the home page; staff reach \
                      the back office of the stores they work in from there.",
            who: "Everyone with an account: customers, cashiers, mechanics, store \
                  managers and the owner.",
            audience: &[
                Audience::Visitor,
                Audience::Customer,
                Audience::Staff,
                Audience::Owner,
            ],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Valid<T>",
                    why: "The email and password are checked before the database is asked; \
                          a blank field or a wrong password comes back with the error next \
                          to the field and the email kept (a 422 for the htmx form, a \
                          redirect with old input without JavaScript), so nobody retypes \
                          anything.",
                },
                Feature {
                    api: "Login throttle",
                    why: "Too many failed attempts for one email, from one IP address, or \
                          for the pair, lock that login for a while (the error says how \
                          many seconds), so passwords can't be guessed by brute force. \
                          It's built in, so the shop didn't have to add a rate limit of \
                          its own.",
                },
                Feature {
                    api: "Sessions",
                    why: "Logging in stores the user's id with a fingerprint of the password \
                          hash and a new id for this device, and gives the form a new CSRF \
                          token: changing the password later ends the other sessions, and \
                          \"Remember me\" makes this one last longer \
                          (`REMEMBER_LIFETIME`). No `remember_token` column is needed.",
                },
                OAUTH_BUTTONS,
                Feature {
                    api: "renox-2fa (Registry::second_factor)",
                    why: "Someone with two-factor login on (staff must turn it on, \
                          customers may) isn't logged in after the password: the login \
                          waits in the session at `/two-factor/challenge` for the code \
                          from their phone. The plugin plugs into Renox's own login with \
                          one hook, so the shop's login page didn't change.",
                },
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "On submit: the form is validated, the throttle checks the email \
                         and IP address aren't locked, the user is looked up by the \
                         normalized email and the password checked with Argon2 (on a \
                         blocking thread, with a dummy hash for unknown emails so timing \
                         reveals nothing). A failure counts towards the lock and emits \
                         `LoginFailed` (or `LockedOut`), which the `Audit` module writes \
                         to the activity log. With two-factor login on, the login waits \
                         for the code. Otherwise the throttle is cleared, the user's id \
                         goes into the session, a `LoggedIn` event is emitted and the \
                         visitor goes back to the page they wanted (`Redirect::intended`), \
                         else home. The shop's `LoggedIn` listener notes a member of staff \
                         without two-factor login, who is sent to set it up before the \
                         staff side opens (`src/app/staff/two_factor.rs`).",
            docs: &[
                "docs/authorization.md#the-auth-modules-routes",
                "docs/routing.md#sessions",
                "docs/routing.md#rate-limits",
                "docs/validation.md#when-validation-fails",
                "docs/oauth.md#what-your-users-see",
                "docs/two-factor.md#what-your-users-see",
            ],
            sources: &[
                "crates/renox-core/src/auth/module.rs",
                "crates/renox-core/src/auth/throttle.rs",
                "crates/renox-oauth/views/login_options.html",
                "crates/renox-core/views/auth/login.html",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
                "examples/bikeshop/src/app/staff/two_factor.rs",
                "examples/bikeshop/tests/accounts.rs",
            ],
            code: &[
                Code {
                    title: "Share: the demo accounts, only on `/login` of a seeded shop",
                    region: "login.demo",
                },
                Code {
                    title: "Template: Renox's login options, replaced by a file of the name",
                    region: "login.demo.template",
                },
                Code {
                    title: "Test: five wrong passwords lock even the right one out",
                    region: "login.test",
                },
            ],
        },
        Explanation {
            route: "register",
            path: "/register",
            title: "Register",
            purpose: "A customer creates an account to buy online, book rentals and \
                      services, and see their bikes. Staff accounts are made by the \
                      store managers, not here.",
            who: "Visitors who want to become customers.",
            audience: &[Audience::Visitor],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Password rules",
                    why: "The password must be long enough and typed twice the same; the \
                          rules are Renox's `Password` policy, shared with the reset and \
                          account pages, so one setting changes all three and none can be \
                          weaker than the others.",
                },
                Feature {
                    api: "Valid<T>",
                    why: "Name, email and password are checked, and the email must be \
                          unique (a `unique` rule that asks the database, on the address \
                          lowercased, so `Ana@…` and `ana@…` can't be two accounts). Two \
                          sign-ups with one address at the same moment are stopped by the \
                          database's unique index and get the same message.",
                },
                Feature {
                    api: "Auth::on_registered",
                    why: "The hook runs after the user is saved and before they're logged \
                          in, for `/register` and for a first social login alike: it adds \
                          the `customers` row that orders, rentals and bikes point at, and \
                          saves the language they signed up in. If it fails, Renox removes \
                          the user again, so there is never a login without a customer.",
                },
                Feature {
                    api: "Auth::verify_email",
                    why: "A mail with a signed link (valid for an hour) asks the new \
                          customer to confirm the address, so mails about orders and \
                          rentals reach the right person. One builder call: the shop \
                          writes neither the mail nor the link check.",
                },
                OAUTH_BUTTONS,
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "On submit: validation (with the email lowercased before the \
                         unique check), the password hashed with Argon2, the user \
                         inserted and read back, then `accounts::registration::on_registered` \
                         inserts the `customers` row and sets `users.locale` (if it fails, \
                         the user row is deleted again); the verification mail is sent, a \
                         `Registered` event emitted (the activity log records it) and the \
                         new customer logged in. A walk-in who already has a \
                         record at the counter isn't linked by email here (the address \
                         isn't verified yet): they claim it from an invitation.",
            docs: &[
                "docs/authorization.md#the-auth-modules-routes",
                "docs/validation.md#passwords",
                "docs/authorization.md#logging-in-another-way",
                "docs/oauth.md#which-account-a-sign-in-logs-into",
            ],
            sources: &[
                "crates/renox-core/src/auth/module.rs",
                "crates/renox-core/src/auth/user.rs",
                "crates/renox-core/src/auth/verification.rs",
                "crates/renox-core/views/auth/register.html",
                "examples/bikeshop/src/lib.rs",
                "examples/bikeshop/src/app/accounts/registration.rs",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
                "examples/bikeshop/tests/accounts.rs",
            ],
            code: &[
                Code {
                    title: "Hook: `Auth::on_registered` makes the customer",
                    region: "register.hook",
                },
                Code {
                    title: "Template: the social buttons under Renox's form",
                    region: "accounts.oauth_buttons",
                },
                Code {
                    title: "Test: signing up makes a customer, in the visitor's language",
                    region: "register.test",
                },
            ],
        },
        Explanation {
            route: "password.request",
            path: "/forgot-password",
            title: "Forgot your password?",
            purpose: "Asks for an email address and mails a link to choose a new \
                      password.",
            who: "Customers and staff who can't log in.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Staff],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Mail",
                    why: "The reset link goes out as a mail rendered from Renox's \
                          template, in the visitor's language, so the shop wrote no mail \
                          for it; while developing, `/_renox/mail` shows what was sent.",
                },
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "On submit: the address is validated and lowercased; if it has an \
                         account, a random token is stored hashed (SHA-256) in \
                         `password_reset_tokens` and the mail with the link is sent, at \
                         most once a minute per address. The answer is the same whether \
                         the email exists or not, so the page can't be used to find out \
                         who has an account.",
            docs: &[
                "docs/authorization.md#the-auth-modules-routes",
                "docs/mail.md#sending-a-mail",
            ],
            sources: &[
                "crates/renox-core/src/auth/passwords.rs",
                "crates/renox-core/views/auth/forgot-password.html",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
            ],
            code: &[Code {
                title: "Test: the same answer for any address; the link comes by mail",
                region: "password.request.test",
            }],
        },
        Explanation {
            route: "password.reset",
            path: "/reset-password/{token}",
            title: "Choose a new password",
            purpose: "The page the reset mail's link opens: a new password, typed twice.",
            who: "Whoever received the reset mail.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Staff],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Password rules",
                    why: "The same `Password` policy as registration, so a reset can't \
                          set a weaker password.",
                },
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "When the form is sent, the token from the link is compared with \
                         the stored hash (in constant time) and must be under an hour old; \
                         a wrong or old one answers with an error, not a hint. The new \
                         password is hashed with Argon2 and saved, the user's API tokens \
                         revoked, the reset token deleted, a `PasswordReset` event emitted \
                         (the activity log records it), and every session of that user \
                         ends (their password fingerprint changed). The visitor goes to \
                         the login page to sign in with it.",
            docs: &[
                "docs/authorization.md#the-auth-modules-routes",
                "docs/validation.md#passwords",
                "docs/ui.md#renoxs-own-pages",
            ],
            sources: &[
                "crates/renox-core/src/auth/passwords.rs",
                "crates/renox-core/views/auth/reset-password.html",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
            ],
            code: &[Code {
                title: "Test: the mailed token sets a new password, which logs in",
                region: "password.reset.test",
            }],
        },
        Explanation {
            route: "password.confirm",
            path: "/confirm-password",
            title: "Confirm your password",
            purpose: "Asks for the password again before a sensitive change, such as \
                      turning two-factor login on or off. Someone who signed up with \
                      Google or GitHub and has no password confirms here with that \
                      provider instead, before changing their account.",
            who: "Logged-in customers and staff.",
            audience: &[Audience::Customer, Audience::Staff],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Routes::require_password_confirmed",
                    why: "`renox-2fa`'s pages that change the second step sit behind it: \
                          they send the user here first unless the password was typed in \
                          the last three hours, then back to where they were going. \
                          Someone at a computer left logged in can't turn it off.",
                },
                Feature {
                    api: "renox-oauth",
                    why: "Under the form, only the providers this login is linked to are \
                          offered (another one wouldn't prove who they are), so an \
                          account without a password can still confirm.",
                },
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "On submit the password is checked with Argon2 and the time of \
                         the confirmation is stored in the session (logging in counts \
                         too); the visitor goes back to the page that asked. With a \
                         provider, the round trip to Google or GitHub must come back with \
                         the linked account.",
            docs: &[
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
                "docs/authorization.md#the-auth-modules-routes",
                "docs/oauth.md#users-without-a-password",
                "docs/two-factor.md#its-pages-and-routes",
            ],
            sources: &[
                "crates/renox-core/src/auth/account.rs",
                "crates/renox-core/views/auth/confirm-password.html",
                "crates/renox-2fa/src/handlers.rs",
                "crates/renox-oauth/src/lib.rs",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
            ],
            code: &[
                Code {
                    title: "Test: `require_password_confirmed` sends the user here first",
                    region: "password.confirm.test",
                },
                Code {
                    title: "Template: a linked Google or GitHub login can confirm too",
                    region: "accounts.oauth_buttons",
                },
            ],
        },
        Explanation {
            route: "verification.notice",
            path: "/verify-email",
            title: "Check your email",
            purpose: "Tells a new user to open the verification link mailed to them, \
                      with a button to send it again.",
            who: "Logged-in users whose email isn't verified yet.",
            audience: &[Audience::Customer],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Signed URLs",
                    why: "The link in the mail is signed with the app's key and works for \
                          an hour, so it can't be forged or changed to verify someone \
                          else, and no table of verification codes is needed.",
                },
                CSRF,
                AUTH_LAYOUT,
            ],
            under_hood: "Loading: someone already verified is sent to the home page. \
                         Sending again posts to `verification.send`, which mails a new \
                         signed link to `verification.verify`. Opening that link (logged \
                         in as the same user, with the same address) sets \
                         `users.email_verified_at`, emits `EmailVerified` (the activity \
                         log records it) and goes home with a message.",
            docs: &[
                "docs/authorization.md#the-auth-modules-routes",
                "docs/routing.md#signed-urls",
            ],
            sources: &[
                "crates/renox-core/src/auth/verification.rs",
                "crates/renox-core/views/auth/verify-email.html",
                "examples/bikeshop/resources/views/renox/auth/layout.html",
            ],
            code: &[Code {
                title: "Test: a signed link, valid for an hour, verifies the address",
                region: "verification.notice.test",
            }],
        },
        Explanation {
            route: "account.show",
            path: "/account",
            title: "My account",
            purpose: "A customer's settings with the shop, in one place: their name \
                      and login, contact details and address, whether their ID was \
                      checked (needed to rent), their service plan, two-factor login, \
                      linked Google or GitHub accounts, how they want to be told about \
                      each kind of news, their language, and their data (download it, \
                      or delete the account). Staff without a customer record don't see \
                      the contact and ID cards; the rest concerns any login.",
            who: "Every logged-in customer, and staff for their own login.",
            audience: &[Audience::Customer, Audience::Staff],
            flow: Flow::Account,
            features: &[
                AUTH_MODULE,
                Feature {
                    api: "Registry::account_section",
                    why: "Each module adds its own card to Renox's account page from its \
                          `register`: a template, an order and a closure that loads what \
                          it shows for the logged-in user. The accounts area adds contact, \
                          ID check, notifications, language and privacy \
                          (`accounts::section_order`); `renox-billing` adds the plan, \
                          `renox-2fa` and `renox-oauth` add theirs. No module edits \
                          another's code or the page's template, so a plugin's card \
                          appears just by adding the plugin.",
                },
                Feature {
                    api: "View overrides (renox/auth/account.html)",
                    why: "The page is Renox's handler with the shop's template: a file of \
                          the same name in `resources/views` replaces the built-in one, so \
                          the page sits in the shop's layout, with its cards on a CSS grid \
                          (two columns on a wide screen, one on a phone), while the \
                          password, devices and deletion forms still post to Renox's routes.",
                },
                Feature {
                    api: "Valid<T> + #[derive(Validate)]",
                    why: "The contact, notification and language forms are typed structs \
                          with their rules on the fields (`required`, `max`, `one_of`, \
                          `exists(\"countries\", \"id\")`); a mistake comes back next to \
                          the field with what was typed kept, and the handler only ever \
                          sees valid data.",
                },
                Feature {
                    api: "Live validation",
                    why: "`data-live-validate` on the contact form: leaving a field asks \
                          the server with `X-Renox-Validate`, and `Valid<T>` answers that \
                          field's errors from the same rules without running the handler. \
                          Nothing is written twice.",
                },
                Feature {
                    api: "UI kit: toggle_buttons",
                    why: "The notification preferences are one segmented control per kind \
                          (both, mail, in the app, none) and the language another: the \
                          kit's radio-based `toggle_buttons`, keyboard-usable, so no custom \
                          \"matrix of toggles\" was needed.",
                },
                Feature {
                    api: "Method spoofing",
                    why: "HTML forms can only send GET and POST; the forms send `PUT` and \
                          `DELETE` through a hidden `_method` field (`method_field()`), so \
                          each action is its own route with its own name.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "Deleting the account is Renox's route (the password is typed \
                          again in the form). The shop listens to the `AccountDeleted` \
                          event it announces and makes the customer anonymous: personal \
                          data goes, orders and payments stay for the books, plans are \
                          cancelled, the ID document's files are deleted. Listening keeps \
                          Renox's route untouched instead of copying it.",
                },
                Feature {
                    api: "Job (queue)",
                    why: "\"Download my data\" only queues `ExportMyData`: a worker gathers \
                          everything into a JSON file on the storage disk and mails a link \
                          that works for seven days (`Storage::temporary_url`), so the page \
                          answers at once however much history there is.",
                },
            ],
            under_hood: "Loading runs each section's closure in order (a query or two each: \
                         the customer, their address with city and country, the countries \
                         for the select). Each form answers with a toast and back to this \
                         page. Contact: the city is found by name in the country or added, \
                         the address saved, the customer updated. Notifications: the \
                         choices are stored as JSON in `users.notification_preferences`, \
                         and `accounts::preferences::channels_for(to, Kind)` turns them \
                         into Renox's channels for a notification to use. Language: saved \
                         in `users.locale` and in the session; Renox's notifications write \
                         mails in `users.locale` by themselves. Changing the password ends \
                         the other sessions; \"log out other devices\" bumps \
                         `users.sessions_revoked_at`. \"Download my data\" queues \
                         `ExportMyData`. Deleting: Renox deletes the login, then \
                         `accounts::privacy::on_account_deleted` anonymises the customer in \
                         a transaction, deletes `customers/{id}/` from the disk and writes \
                         `customer.erased` to the activity log.",
            docs: &[
                "docs/authorization.md#a-section-on-the-account-page",
                "docs/authorization.md#the-auth-modules-routes",
                "docs/ui.md#renoxs-own-pages",
                "docs/validation.md#derivevalidate",
                "docs/queue.md#a-job",
                "docs/scheduling.md#events",
                "docs/mail.md#localized-notifications",
                "docs/ui.md#form-fields",
                "docs/ui.md#live-validation",
                "docs/routing.md#method-spoofing",
            ],
            sources: &[
                "examples/bikeshop/src/app/accounts/mod.rs",
                "examples/bikeshop/src/app/accounts/preferences.rs",
                "examples/bikeshop/src/app/accounts/locale.rs",
                "examples/bikeshop/src/app/accounts/privacy.rs",
                "examples/bikeshop/resources/views/renox/auth/account.html",
                "examples/bikeshop/resources/views/accounts/sections/contact.html",
                "examples/bikeshop/resources/views/accounts/sections/id_check.html",
                "examples/bikeshop/resources/views/accounts/sections/notifications.html",
                "examples/bikeshop/resources/views/accounts/sections/language.html",
                "examples/bikeshop/resources/views/accounts/sections/privacy.html",
                "examples/bikeshop/resources/views/mail/accounts/export_ready.html",
                "examples/bikeshop/tests/accounts.rs",
                "crates/renox-core/src/auth/account.rs",
            ],
            code: &[
                Code {
                    title: "Sections: each area adds its card with `Registry::account_section`",
                    region: "account.show.sections",
                },
                Code {
                    title: "Template: Renox's forms, then every module's section in order",
                    region: "account.show.template",
                },
                Code {
                    title: "Form: `#[derive(Validate)]` with `exists` for the country",
                    region: "account.show.contact",
                },
            ],
        },
        Explanation {
            route: "notifications.index",
            path: "/notifications",
            title: "Notifications",
            purpose: "The customer's in-app notifications: an order ready to collect, a \
                      rental due back, a bike ready at the workshop, the next plan visit. \
                      The same list opens as a panel from the bell in the top bar, where \
                      new ones arrive while the page is open.",
            who: "Logged-in customers, and staff for what concerns them.",
            audience: &[Audience::Customer, Audience::Staff],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "Auth::notifications",
                    why: "One builder call adds the `notifications.*` routes (this page, \
                          mark read or unread, delete, clear), `unread_notifications` in \
                          every view, and the live stream; the shop writes none of it.",
                },
                Feature {
                    api: "UI kit: notification_bell",
                    why: "The bell in both layouts shows the unread count; a click opens \
                          the latest in a panel, and new ones arrive as a toast and in the \
                          badge. Without JavaScript it is a link to this page.",
                },
                Feature {
                    api: "DatabaseMessage",
                    why: "A notification sent on `Channel::Database` stores a \
                          `DatabaseMessage` (a status, a title, a line, a link), the shape \
                          the bell and this page show.",
                },
                Feature {
                    api: "Server-Sent Events (Hub)",
                    why: "`/notifications/stream` is a Server-Sent Events stream: Renox's \
                          in-process `Hub` wakes it when a notification is stored, and it \
                          polls the table every 15 seconds for ones stored by another \
                          process (a queue worker on its own).",
                },
                Feature {
                    api: "Notification::channels",
                    why: "Each notification says where it goes. The shop's notices pick \
                          `Channel::Database` for anyone with a login (so they land here) \
                          and add `Channel::Mail` when the moment deserves a mail. The \
                          account page's choices are turned into channels by \
                          `accounts::preferences::channels_for(to, Kind)` (\"mail\", \"in \
                          the app\", \"both\" or \"none\"), for a notification's \
                          `channels` to return.",
                },
            ],
            under_hood: "The page reads a page of the user's rows of `notifications`, \
                         newest first (older ones behind a link), and the unread count. \
                         Opening one marks it read and follows its link; each can be \
                         marked read or unread or deleted, or all at once. The bell's \
                         stream ends after five minutes and the browser opens a new one, \
                         so a logged-out session doesn't keep one open.",
            docs: &[
                "docs/mail.md#database-notifications",
                "docs/mail.md#the-bell",
                "docs/mail.md#how-new-ones-arrive",
            ],
            sources: &[
                "crates/renox-core/src/auth/inbox.rs",
                "crates/renox-core/views/notifications.html",
                "examples/bikeshop/src/app/accounts/preferences.rs",
                "examples/bikeshop/src/app/rentals/notify.rs",
                "examples/bikeshop/src/app/sales/notify.rs",
                "examples/bikeshop/resources/views/layouts/app.html",
                "examples/bikeshop/resources/views/layouts/staff.html",
                "examples/bikeshop/tests/accounts.rs",
            ],
            code: &[
                Code {
                    title: "Channels: the customer's choice decides mail, the bell, or both",
                    region: "notifications.channels",
                },
                Code {
                    title: "Template: the kit's `toggle_buttons`, one row per kind",
                    region: "notifications.template",
                },
                Code {
                    title: "Test: sent for real, each kind goes where it was asked to",
                    region: "notifications.test",
                },
            ],
        },
        Explanation {
            route: "accounts.claim",
            path: "/claim/{customer}/{email}",
            title: "Claim your record",
            purpose: "A walk-in customer the cashier already knows opens the link from \
                      their invitation and links their past purchases, rentals and bikes \
                      to the account they just made (or already had).",
            who: "A customer who was invited by staff, logged in with the invited address.",
            audience: &[Audience::Customer],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "The link is `state.signed_url(\"accounts.claim\", …)`: the \
                          customer's id, the address it went to and an expiry (seven days) \
                          signed with `APP_KEY`. Changing any of them, or opening it late, \
                          answers 403, so nobody can claim another record by editing the \
                          address bar.",
                },
                Feature {
                    api: "Routes::require_auth",
                    why: "The link needs a login: a guest goes to the login page (with a \
                          link to register) and Renox brings them back here afterwards \
                          (`Redirect::intended`).",
                },
                Feature {
                    api: "Transactions",
                    why: "Claiming moves anything the new account already had onto the \
                          walk-in record and links it, in one transaction, so a failure \
                          halfway never leaves orders split between two records; the \
                          update only touches a record with no login yet, so it works \
                          once, even for two clicks at the same moment.",
                },
                Feature {
                    api: "audit::record",
                    why: "The claim is written to the audit log (`customer.claimed`), \
                          since it gives a login access to someone's history.",
                },
            ],
            under_hood: "Loading checks the signature, finds the record, works out \
                         whether it is still unclaimed and the logged-in address is the \
                         invited one (else the page says why), and counts the record's \
                         orders, rentals and bikes (three `COUNT` queries). The button \
                         posts to the same signed address: in a transaction the walk-in \
                         row gets the user's id and address, the rows of the account's own \
                         customer record (`orders`, `payments`, `rentals`, \
                         `customer_bikes`) move to it, and that record is deleted. Then the \
                         audit entry, a toast, and the account page.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/routing.md#guards",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
            ],
            sources: &[
                "examples/bikeshop/src/app/accounts/claim.rs",
                "examples/bikeshop/resources/views/accounts/claim.html",
                "examples/bikeshop/resources/views/mail/accounts/claim_invitation.html",
                "examples/bikeshop/tests/accounts.rs",
            ],
            code: &[
                Code {
                    title: "Handler: `ValidSignature` guards the link from the mail",
                    region: "accounts.claim.show",
                },
                Code {
                    title: "Transaction: the record is linked once, even if clicked twice",
                    region: "accounts.claim.link",
                },
                Code {
                    title: "Template: what the record holds, or why it can't be claimed",
                    region: "accounts.claim.template",
                },
            ],
        },
        Explanation {
            route: "accounts.invite",
            path: "/staff/customers/{customer}/invite",
            title: "Invite a customer to their account",
            purpose: "At the counter: send a walk-in customer (someone with a record but \
                      no login) a mail with a link to claim their record online, so their \
                      past purchases and bikes are there when they sign up.",
            who: "Cashiers and managers (`customers.manage` in the store they work in).",
            audience: &[Audience::Cashier, Audience::Manager, Audience::Owner],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "Routes::require_permission",
                    why: "`customers.manage`, checked in the active store \
                          (`access::staff_routes`): a mechanic gets a 403, a guest the login \
                          page. Customers belong to the company, so any store may invite.",
                },
                Feature {
                    api: "Valid<T> + #[derive(Validate)]",
                    why: "The address is `required`, an `email` and at most 255 \
                          characters, declared on the form's struct; a mistake comes back \
                          next to the field and nothing is sent.",
                },
                Feature {
                    api: "Signed URLs",
                    why: "The mail's link is signed and expires in seven days \
                          (`state.signed_url`), so it needs no table of invitations to \
                          store, look up or clean.",
                },
                Feature {
                    api: "queue_mail",
                    why: "The mail is queued (`state.queue_mail`), so the counter doesn't \
                          wait for the mail server; a worker sends it with retries.",
                },
            ],
            under_hood: "The record must exist, have no login and not be deleted (else a \
                         404). Sending validates the address, saves it (lowercased) on the \
                         record, builds the signed link, renders \
                         `mail/accounts/claim_invitation.html` in the staff member's \
                         language and queues it, then comes back to this page with a toast.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/mail.md#sending-a-mail",
                "docs/validation.md#derivevalidate",
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
            ],
            sources: &[
                "examples/bikeshop/src/app/accounts/claim.rs",
                "examples/bikeshop/resources/views/accounts/invite.html",
                "examples/bikeshop/resources/views/mail/accounts/claim_invitation.html",
                "examples/bikeshop/tests/accounts.rs",
            ],
            code: &[
                Code {
                    title: "Handler: a signed link for seven days, mailed from the queue",
                    region: "accounts.invite.send",
                },
                Code {
                    title: "Routes: staff routes behind `customers.manage`",
                    region: "accounts.invite.routes",
                },
                Code {
                    title: "Template: a form checked live as the address is typed",
                    region: "accounts.invite.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "verification.verify",
            reason: "the signed link from the verification mail: it marks the email \
                     verified and redirects, showing no page of its own",
        },
        NotAPage {
            route: "notifications.stream",
            reason: "the bell's Server-Sent Events stream, not a page",
        },
        NotAPage {
            route: "oauth.redirect",
            reason: "renox-oauth: sends the browser to Google or GitHub (a redirect)",
        },
        NotAPage {
            route: "oauth.callback",
            reason: "renox-oauth: the provider sends the browser back here; it logs in or \
                     links, then redirects",
        },
    ]
}
