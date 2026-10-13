# Mail and notifications

Apps often need to tell people things: "here is your receipt", "your order shipped", "someone
replied to you". This guide shows how a Renox app sends **email**, and how it sends
**notifications**: one message that can go out by email, show up in a list inside the app, or
travel through your own channels such as WhatsApp or SMS.

### Words you'll meet

| Word | What it means |
|---|---|
| **SMTP** | The standard way computers hand email to each other. An SMTP server is the "post office" your app gives mail to. |
| **mailer** | The part of Renox that sends mail. Its **driver** decides *how*: through SMTP, into the log, or into memory for tests. |
| **template** (or view) | An HTML file with blanks that Renox fills in, like `{{ order_id }}`. Mails can be made from templates, just like pages. |
| **plain-text part** | Every mail also carries a version without HTML, for mail apps that don't show HTML. |
| **queue** | A to-do list of jobs that run in the background, so a page doesn't have to wait. A **worker** is the loop that takes jobs off it. |
| **notification** | One message to one person, which can be delivered several ways at once. |
| **channel** | One of those ways: mail, a row in the database (the in-app list), or your own (WhatsApp, SMS, Slack…). |
| **recipient** | Who a notification is for: a user with an account, or just an address. |
| **locale** | The language a mail or message is written in, like `en` or `es`. |

### In this guide

- [Configuration](#configuration): choose how mail leaves the app.
- [Sending a mail](#sending-a-mail): build a mail and send it, now or through the queue.
- [Mail views](#mail-views): make mails from templates, with ready-made buttons and tables.
- [Localized mail](#localized-mail): send a mail in another language.
- [The preview page](#the-preview-page): see the mails your app sent while you develop.
- [Notifications](#notifications): one message, several channels.
- [Database notifications](#database-notifications): the in-app list and the bell icon.
- [Your own live events](#your-own-live-events): push events from the server to open pages.
- [Testing](#testing) and [In production](#in-production).

Under the hood, Renox sends email with [lettre](https://lettre.rs) over SMTP. The secure
connection uses rustls, so there is no OpenSSL to install. Mails are made from MiniJinja
templates, always with a plain-text version. They can be sent through the queue, which tries
again when sending fails.

Want working code to look at?

- The short version is in the [cheat-sheet](../CHEATSHEET.md) ("Mail and notifications").
- [examples/bikeshop](../examples/bikeshop) queues order mails and notifications with the bell
  ([src/app/sales/notify.rs](../examples/bikeshop/src/app/sales/notify.rs)), mails each store its monthly report
  with workbooks attached ([src/app/reports/monthly.rs](../examples/bikeshop/src/app/reports/monthly.rs)), and
  previews every mail on `/sales/mails` ([src/app/sales/mails.rs](../examples/bikeshop/src/app/sales/mails.rs)).

## Configuration

Settings live in your app's `.env` file. The first one, `MAIL_MAILER`, picks the driver:
how mail leaves your app.

| Driver | What it does |
|---|---|
| `log` (default) | Writes each mail (to, subject, text) to the server log. Nothing leaves your computer. Good while you build the app. |
| `smtp` | Really sends the mail, through the server in `MAIL_HOST`. Use it in production. |
| `memory` | Keeps every mail in memory, so tests can check them (`TestApp` uses it). |

Any other value stops the app at boot (when it starts), with an error.

For `smtp`, these settings say which server to use and how to log in to it:

| Variable | Default | Meaning |
|---|---|---|
| `MAIL_HOST` | `localhost` | The SMTP server's address. |
| `MAIL_PORT` | the encryption's usual port | e.g. 587 for `starttls`, 465 for `tls`, 1025 for Mailpit (a fake mail server for testing). |
| `MAIL_ENCRYPTION` | `starttls` | How the connection is protected: `tls`, `starttls` or `none` (only for a test server on your own machine). |
| `MAIL_USERNAME`, `MAIL_PASSWORD` | none | The login for the server. Used only when both are set. |
| `MAIL_FROM_ADDRESS` | `hello@example.com` | Who the mail is from. It must be an email address, or `smtp` stops the app at boot. |
| `MAIL_FROM_NAME` | `APP_NAME` | The sender's name, shown next to the address. |
| `MAIL_TIMEOUT` | 10 | How many seconds one send may take, from connecting to the server's last reply. |

> [!IMPORTANT]
> Set `APP_URL` to your app's public address (like `https://shop.example`). Links in mails are
> built from it, so without it they would point to the wrong place.

### More mailers, and failover

Sometimes one mailer isn't enough. You might want:

- a newsletter service for newsletters;
- a separate account, so newsletters and receipts don't mix;
- a second SMTP provider, as a backup.

`App::mailer(name, settings)` adds a mailer with a name. Its settings usually come from
`MailConfig::from_env(config, "BACKUP")`, which reads variables that start with that word:

- `BACKUP_MAILER`: the driver. When it's not set, it uses the app's own `MAIL_MAILER`, so it
  is `log` while you develop and `memory` in tests.
- `BACKUP_HOST`, `BACKUP_PORT`, `BACKUP_USERNAME`, `BACKUP_PASSWORD`, `BACKUP_ENCRYPTION` and
  `BACKUP_TIMEOUT`: the same as the `MAIL_*` settings above.
- `BACKUP_FROM_ADDRESS` and `BACKUP_FROM_NAME`: when not set, the `MAIL_FROM_*` ones are used.

```rust
use renox::prelude::*;
use renox::mail::{Mail, MailConfig};

/// Builds the app with two extra mailers, each with its own settings.
fn app() -> App {
    App::new()
        // Reads NEWSLETTER_MAILER, NEWSLETTER_HOST, ...
        .mailer("newsletter", |config| MailConfig::from_env(config, "NEWSLETTER"))
        // Reads BACKUP_MAILER, BACKUP_HOST, ...
        .mailer("backup", |config| MailConfig::from_env(config, "BACKUP"))
}

/// Sends one mail through the "newsletter" mailer, in two ways.
async fn send(state: AppState) -> Result {
    let news = Mail::new("ann@example.com", "October", "What's new.");
    state.mailer_named("newsletter")?.send(news.clone()).await?; // now
    state.queue_mail_via("newsletter", news).await?;             // through the queue
    Ok(())
}
```

What's going on:

- `.mailer("newsletter", …)` registers a mailer called `newsletter`.
- `state.mailer_named("newsletter")` finds it, and `.send(…)` sends a mail right away.
- `state.queue_mail_via("newsletter", …)` puts the mail on the queue instead, to be sent by that
  mailer in the background.

**Failover** means "if the first one fails, try the next". Set `MAIL_FAILOVER=backup` to use
it. The value is a list of names from `App::mailer`, separated by commas, tried in order.

When the default mailer fails (say, your main SMTP provider is down):

- the mail is sent by the next mailer that works;
- a warning goes to the log;
- the mail still counts as sent by the app: it shows up in `/_renox/mail` and in
  `app.sent_mail()`.

> [!WARNING]
> Failover only helps when the *mailer* is the problem. A mail that can't be sent at all, such
> as one with a bad address, isn't handed on. And a name in `MAIL_FAILOVER` that no mailer has
> stops the app at boot.

## Sending a mail

`Mail::new(to, subject, text)` makes a plain-text mail. Then you add more with **builder**
methods: small methods you chain one after another, each adding one thing. Addresses can be
written as `a@b.c` or `Name <a@b.c>`.

```rust
use renox::prelude::*;
use renox::mail::Mail;

/// A handler that builds an invoice mail with every extra, and sends it right away.
async fn invoice(State(state): State<AppState>) -> Result {
    // The file to attach, as bytes. A real app would make or load a real PDF here.
    let pdf: Vec<u8> = b"%PDF-1.7 ...".to_vec();
    let mail = Mail::new("ben@example.com", "Invoice INV-001", "Your invoice is attached.")
        .html("<p>Your invoice is attached.</p>") // text is still sent as the plain part
        .also_to("sarah@example.com")
        .cc("sales@example.com")
        .bcc("archive@example.com")
        .reply_to("Coffee Shop <hello@shop.example>")
        .from("Coffee Shop Billing <billing@shop.example>") // instead of MAIL_FROM_*
        .attach("INV-001.pdf", "application/pdf", pdf);
    state.mailer.send(mail).await // now, in this request
}
```

What each builder method adds:

- `.html(…)`: an HTML version. The text from `Mail::new` is still sent, as the plain part.
- `.also_to(…)`: one more main recipient.
- `.cc(…)`: a copy to someone else, which every recipient can see.
- `.bcc(…)`: a hidden copy; the other recipients don't see this address.
- `.reply_to(…)`: where answers go when someone clicks "Reply".
- `.from(…)`: a different sender for this mail, instead of `MAIL_FROM_*`.
- `.attach(name, content type, bytes)`: a file. The content type tells mail apps what kind
  of file it is (`application/pdf` is a PDF).

There are two ways to send:

- `state.mailer.send(mail)` sends **now**. The request waits for the SMTP server (up to
  `MAIL_TIMEOUT`), and you get its error if it fails.
- `state.queue_mail(mail)` **queues** it as the job `renox.send-mail` and gives back the job's
  id. A worker sends it, with up to **five attempts**. If the last attempt fails too, the job
  moves to the `failed_jobs` table, where you can see it with `queue:failed` and send it again
  with `queue:retry`. See [queue.md](queue.md).

> [!TIP]
> In request handlers, prefer `queue_mail`. That way a slow mail server doesn't make the page
> slow.

> [!NOTE]
> Some mistakes can never succeed, however often you try: an invalid address, a mail with no
> recipient, or a bad content type on an attachment. These are **permanent errors**: a queued
> mail with one of them fails at once instead of trying five times.

## Mail views

Writing HTML inside Rust strings gets messy fast. A **mail view** is a template file for a mail,
just like a page's template.

`state.mail_view(to, subject, view, ctx)` fills in two files:

- `resources/views/{view}.html` becomes the HTML part;
- `resources/views/{view}.txt` becomes the plain-text part.

The `.txt` file is optional. Without it, Renox makes the text from the HTML: it drops the tags
and writes links as `text (url)`.

`mail_view` gives back a normal `Mail`, so all the builder methods above still work.

```rust
use renox::prelude::*;

/// Builds a receipt mail from a template and puts it on the queue.
async fn receipt(state: &AppState, order_id: i64, pdf: Vec<u8>) -> Result {
    let mail = state
        .mail_view(
            "ben@example.com",
            format!("Receipt for order #{order_id}"),
            "mail/receipt", // mail/receipt.html (+ mail/receipt.txt)
            context! { order_id, total => 75_000 },
        )?
        .bcc("archive@example.com")
        .attach("receipt.pdf", "application/pdf", pdf);
    state.queue_mail(mail).await?;
    Ok(())
}
```

What's going on:

- `context! { … }` holds the values the template can use: here `order_id` and `total`.
- The `?` after `mail_view(…)` passes on an error, for example when the template is missing.
- `.bcc(…)` and `.attach(…)` work as before.
- `queue_mail` sends it in the background.

Besides the values in `ctx`, mail templates can use `app.name`, `app.url`, `app.locale` and
`t(key, …)` (which looks up a translated text).

Renox also ships a **layout** (the frame around every mail: header, footer) and
**components** (ready-made pieces like buttons and tables). They are styled inline, because many
mail apps ignore style sheets.

```html
{# resources/views/mail/receipt.html #}
{# Use Renox's mail frame, and bring in four ready-made pieces. #}
{% extends "renox/mail/layout.html" %}
{% from "renox/mail/components.html" import button, panel, table, divider %}
{% block content %}
<p>{{ t('mail.thanks') }}</p>
{% call panel() %}Order #{{ order_id }} ships tomorrow.{% endcall %}
{{ table([["Coffee", "$4.50"]], head=["Item", "Price"], total=["Total", "$4.50"]) }}
{{ divider() }}
{{ button(app.url ~ "/orders/" ~ order_id, "View your order") }}
{% endblock %}
```

The same mail with tags, which the compiler checks like any page (a misspelt attribute names
the file and line):

```html
<rx-mail subject="{{ t('mail.thanks') }}">
  <p>{{ t('mail.thanks') }}</p>
  <rx-mail-panel>Order #{{ order_id }} ships tomorrow.</rx-mail-panel>
  <rx-mail-table :rows="[['Coffee', '$4.50']]" :head="['Item', 'Price']" :total="['Total', '$4.50']"/>
  <rx-mail-divider/>
  <rx-mail-button :url="app.url ~ '/orders/' ~ order_id" label="View your order"/>
</rx-mail>
```

`<rx-mail>` is the `{% extends %}` (its `subject` fills the `title` block and
`<rx-slot name="footer">` the `footer` block); the other four are the macros below. Both
spellings render the same HTML. See [docs/ui.md](ui.md#mails-as-tags).

The text version of a mail (the `.txt` view next to the `.html` one) is `<rx-mail-text>`: the
text as written, with `{{ }}` and `<template rx-for>` / `<template rx-if>` for loops and
conditions, and the same bytes the `{% for %}` / `{% if %}` version printed.

What you can use:

- `renox/mail/layout.html` has three **blocks** (named holes you fill): `title`, `content` and
  `footer`. The footer shows `© {{ app.name }}` unless you fill it.
- `renox/mail/components.html` has:
  - `button(url, label)`: a button, with its URL also written under it (to copy when the button doesn't work);
  - `panel()`: a box around some text, used with `{% call %}`;
  - `table(rows, head=none, total=none)`: a table, with the last column lined up on the
    right;
  - `divider()`: a line across the mail.
- `renox/mail/button.html` has the same `button` on its own.

> [!TIP]
> To restyle every mail, put your own `resources/views/renox/mail/layout.html` (or
> `components.html`) in your app. Your app's views replace Renox's built-in ones that have the
> same file name. This also works for the password-reset and verification mails
> (`renox/mail/auth/reset-password.html` / `.txt`, `verify-email.html` / `.txt`). Their texts
> come from the `renox.auth.*` translations (e.g. `renox.auth.mail_reset_subject`).

To start a new mail, let `rnx` write the files: `rnx make:mail order_shipped` writes
`resources/views/mail/order_shipped.html` (on the layout, with a button) and
`order_shipped.txt`.

## Localized mail

"Localized" means "in the reader's language". `mail_view` writes the mail in the **current
language**, chosen in this order:

1. a notification recipient's language (see [Localized notifications](#localized-notifications));
2. else the language of the request (the visitor's);
3. else `APP_LOCALE` from your settings.

Outside a request, for example in a background job or a command, there is no visitor, so you
pick the language yourself:

```rust
use renox::prelude::*;

/// Sends a welcome mail in Spanish, and shows how to switch language for a whole job.
async fn welcome(state: &AppState, user: &User) -> Result {
    // This mail only, in Spanish (from lang/es.json): t() in the view and the subject use "es".
    let subject = state.lang("es").t("mail.welcome", &[("name", &user.name)]);
    let mail = state.mail_view_in("es", &user.email, subject, "mail/welcome", context! {})?;
    state.queue_mail(mail).await?;

    // Or for the rest of this job or task:
    renox::i18n::set_current_locale("es");
    let subject = state.current_lang().t("mail.welcome", &[("name", &user.name)]);
    let _ = subject;
    Ok(())
}
```

What's going on:

- `state.lang("es").t(…)` translates one text into Spanish: here, the subject.
- `state.mail_view_in("es", …)` is `mail_view` in a chosen language: `t()` inside the template
  uses Spanish too.
- `renox::i18n::set_current_locale("es")` switches the language for the rest of this job or
  task, and `state.current_lang()` then gives Spanish.

Translations live in `resources/lang/<locale>.json`, like `resources/lang/es.json` (see the
cheat-sheet, "Cache, session, uploads, translations").

## The preview page

While you build your app, you want to see the mails it sends without really sending them.
With `APP_DEBUG` on:

- `/_renox/mail` lists the last 50 mails sent (by any driver);
- `/_renox/mail/{id}` shows one: recipients, cc/bcc, reply-to, from, attachments, the HTML (in
  a sandboxed frame, so it can't affect the page around it) and the text.

> [!TIP]
> With the default `log` driver, this page is where you find the password-reset link while
> developing.

The route doesn't exist when debug is off, so it can't leak mails in production.

## Notifications

A **notification** is a message about something that happened ("Order #7 shipped"). You write
it once, and Renox delivers it on each channel you choose: by mail, as a row in the database
for an in-app list, or through your own channel like WhatsApp.

A notification is a struct that implements the `Notification` trait. It has:

- a `kind`: a short name like `"order-shipped"`, stored with database rows and used in tests;
- its **channels**: where it goes;
- one version of the message for each channel.

| Channel | Method you write | Delivered by |
|---|---|---|
| `Channel::Mail` (the default) | `to_mail(&self, to, state) -> Result<Mail>` | `state.mailer`, to `to.email()` |
| `Channel::Database` | `to_database(&self, to, state) -> Result<Value>` | a row in the `notifications` table (users only) |
| `Channel::Custom("whatsapp")` | `to_channel(&self, channel, to, state) -> Result<Value>` | the function you registered with `App::channel` |

> [!WARNING]
> `to_mail` has no useful default: unless you write it, it returns an error. A notification
> left on the default channel (`Channel::Mail`) without its own `to_mail` fails when it's sent.

Here is a full example: a notification that goes by mail, to the in-app list, and, for people
with a WhatsApp number, by WhatsApp.

```rust
use renox::prelude::*;
use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::mail::Mail;

/// The notification: "your order shipped". It only needs the order's number.
struct OrderShipped { order_id: i64 }

impl Notification for OrderShipped {
    /// A short name for this kind of notification.
    fn kind(&self) -> &'static str { "order-shipped" }

    // Per recipient (Laravel's `via`); only Channel::Mail by default.
    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        let mut channels = vec![Channel::Mail, Channel::Database];
        // Add WhatsApp only when this person has a WhatsApp address.
        if to.address("whatsapp").is_some() {
            channels.push(Channel::Custom("whatsapp"));
        }
        channels
    }

    /// The mail version: a subject in the recipient's language, and a template.
    fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {
        let subject = state.current_lang().t("mail.shipped", &[("id", &self.order_id)]);
        state.mail_view(to.email().unwrap_or_default(), subject, "mail/order_shipped",
            context! { order_id => self.order_id })
    }

    // What the in-app list and the kit's bell show; see "Database notifications".
    fn to_database(&self, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
        Ok(DatabaseMessage::success(format!("Order #{} shipped", self.order_id))
            .url(format!("/orders/{}", self.order_id))
            .with("order_id", self.order_id)
            .into())
    }

    /// The version for custom channels (here WhatsApp): any JSON you like.
    fn to_channel(&self, _channel: &str, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
        Ok(json!({ "text": format!("Order #{} shipped", self.order_id) }))
    }
}

/// The app, with the Auth module (users) and the "whatsapp" channel.
fn app() -> App {
    App::new()
        .module(Auth::new())
        // Your own channel: `message` is what to_channel built.
        .channel("whatsapp", |to: Recipient, message, state| async move {
            // The number: the WhatsApp address, else the user's `phone` column.
            let phone = to.address("whatsapp").or_else(|| to.user()?.get("phone"));
            let _ = (state, phone, message); // call your provider's API here
            Ok(())
        })
}

/// Sends the notification to a user (now, then queued), and to a guest.
async fn ship(state: &AppState, user: &User) -> Result {
    let shipped = OrderShipped { order_id: 7 };
    state.notify(user, &shipped).await?;       // now, every channel
    state.notify_later(user, &shipped).await?; // mail and custom channels through the queue
    // Someone without an account (no database row for them):
    let guest = Recipient::to("mail", "guest@example.com")
        .and("whatsapp", "+6281234567890")
        .in_locale("es");
    state.notify(&guest, &shipped).await
}
```

What's going on, step by step.

**Sending now, or later:**

- `state.notify(&user, &n)` delivers the notification **now**, on every channel.
  `state.notify(&recipient, &n)` does the same for a `Recipient`.
- The order matters: the database row is written first, and the mail is sent last. So if
  something fails halfway, no mail has gone out yet, and trying again won't send it twice.
- `state.notify_later(to, &n)` builds every message now and writes the database row now. Then
  it queues each mail and each custom-channel message as its **own job** (five attempts each,
  then `failed_jobs`). It takes a `&User` or a `&Recipient`.
- A custom channel that you never registered with `App::channel` makes `notify_later` fail at
  once.

**Who it goes to (a `Recipient`):**

- `Recipient::for_user(&user)` (or `(&user).into()`) is a user with an account.
- `Recipient::to(channel, address)` is someone without an account, like a guest who ordered.
  Add more addresses with `.and(channel, address)`.
- `to.address(channel)` gives the address for a channel. For `mail` it falls back to the
  user's email, which is also what `to.email()` returns.

> [!NOTE]
> `Notification` is not `Serialize` (it can't be turned into stored data). That's on purpose:
> the queue stores the messages that were already built, never the notification itself.

To start a new one, `rnx make:notification OrderShipped --module orders` writes a notification
with mail and database versions into the `orders` module.

## Localized notifications

Each message is built in its **recipient's** language. Renox picks the language in this order:

1. `Recipient::in_locale("es")`, if you set it;
2. else the user's `locale` column, when your `users` table has one;
3. else the current language.

While `to_mail`, `to_database` and `to_channel` run, both `t()` in mail views and
`state.current_lang()` in code use that language. So one notification sent to many users
speaks each one's language.

## Database notifications

The `Channel::Database` channel saves each notification as a row: together, a user's rows are
their **in-app list** (like the list behind a bell icon on many websites).

Each row is a `DatabaseNotification`, with:

- `id`;
- `kind`;
- `data`: what `to_database` returned;
- `read_at`: when it was read (`None` while unread);
- `created_at`: when it was made.

`to_database` may return any JSON. But a `DatabaseMessage` is what Renox's own list and the UI
kit's bell know how to show. It holds:

- a status: `success`, `info`, `warning` or `error`. It picks the icon;
- a title;
- a `body`;
- a `url`: where opening the notification goes;
- links: `link(label, url)`, or `action(ToastAction::link(…))`.

`.with(key, value)` keeps your app's own keys next to these (read in a template as
`notification.data.order_id`). `notification.message()` reads the message back.

### The bell

`Auth::new().notifications()` turns on a ready-made notification list, with a bell icon that
shows how many are unread.

> [!NOTE]
> **Coming from Laravel:** this is like Filament's database notifications.

```rust
use renox::prelude::*;

/// The app, with the Auth module and its ready-made notification list turned on.
fn app() -> App {
    App::new().module(Auth::new().notifications())
}
```

A new app can start with both: `rnx new my-app --notifications` turns them on and puts the bell
in the layout (the starter kit, `rnx new --starter`, always has it). In an existing app, put
the bell in your layout's navigation bar (the kit's `navbar`, see [ui.md](ui.md)):

```html
<rx-navbar brand="{{ app.name }}" href="{{ route('home') }}">
  <span class="rx-spacer"></span>
  <rx-notification-bell :count="unread_notifications" /> {# nothing for guests #}
</rx-navbar>
```

What you get:

- **The badge.** `unread_notifications`, the logged-in user's unread count, is in every view
  (0 for guests). The bell's badge shows it ("99+" past 99). For screen readers, its label
  reads "Notifications, 3 unread".
- **The panel.** A click opens a panel with the latest 20. Each shows its icon, title, body,
  time ("5 minutes ago") and links, with a dot while unread. Each has buttons to mark it read
  or unread and to delete it. On top are "Mark all as read" and "Clear all".
- **Opening one.** Clicking its title (when it has a `url`) marks it read and follows the
  link. Esc or a click outside closes the panel.
- **Buttons that do something.** A `DatabaseMessage` action made with
  `ToastAction::post(label, "/orders/7/approve")` (or `put`, `patch`, `delete`) is a button
  that sends that request, with the CSRF token, instead of a link. See
  [ui.md](ui.md#toasts) for what the handler answers. Only this site's own paths are sent.
- **Live updates.** While a page is open, new notifications arrive by themselves. The page
  keeps a **Server-Sent Events** stream open (`/notifications/stream`): a connection the server
  can keep pushing messages down. Each new notification shows as a toast (a small pop-up) with
  an "Open" link, and updates the badge. Reading one in another tab updates the badge too.
- **Without JavaScript.** The bell is then a link to `/notifications`: the same list as a full
  page (with "Older" pages), inside the app's `layouts/app.html` (else Renox's sign-in
  layout).

> [!TIP]
> To change how the list looks, replace `renox/notifications.html` in your app. Its `panel`
> block is what the bell shows.

The routes, all for logged-in users only:

| Route name | Address | What it does |
|---|---|---|
| `notifications.index` | `GET /notifications` | The page. With an `HX-Request` header (an htmx request), the panel alone. |
| `notifications.stream` | `GET /notifications/stream` | The live stream. |
| `notifications.read`, `notifications.unread`, `notifications.open` | `POST /notifications/{id}/…` | Mark one read, mark it unread, open it. |
| `notifications.destroy` | `DELETE /notifications/{id}` | Delete one. |
| `notifications.read_all` | `POST /notifications/read-all` | Mark all as read. |
| `notifications.clear` | `DELETE /notifications` | Delete all. |

The kit's texts are under `ui.notifications.*`, so you can translate them.

### How new ones arrive

- A stream sends `count` (the unread count) first, and again whenever it changes. It sends
  `notification` (the new one, as JSON) when one is stored.
- A notification stored **in the same process** (a request, or the queue workers that run
  inside `serve`) wakes the user's streams at once.
- Streams also look at the table every 15 seconds. So a notification stored **by another
  server**, or by a separate `queue:work` process, arrives within that time.
- A stream ends after five minutes and the browser opens a new one. That way a session that
  logged out or was revoked doesn't keep a stream open.
- Every stream ends when the server shuts down.

> [!WARNING]
> Behind a proxy (like nginx), keep SSE unbuffered, or new notifications get stuck in the
> proxy. See [operations.md](operations.md).

### Your own list

Want to build your own page instead of the bell? These methods on the logged-in user
(`AuthUser`) do the work:

```rust
use renox::prelude::*;

/// An inbox page: the latest notifications and the unread count.
async fn inbox(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let notifications = user.notifications(&db, 20).await?; // newest first, at most 20
    let unread = user.unread_notification_count(&db).await?;
    let _all_unread = user.unread_notifications(&db).await?; // every unread one, newest first
    user.mark_all_notifications_read(&db).await?;           // returns how many
    Ok(view("inbox.html", context! { notifications, unread }))
}

/// Marks one notification as read, or answers "not found".
async fn read_one(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result {
    // false when the notification isn't this user's
    if !user.mark_notification_read(&db, id).await? {
        return Err(Error::NotFound);
    }
    Ok(())
}
```

What's going on:

- `user.notifications(&db, 20)` gives the newest 20.
- `user.unread_notification_count(&db)` counts the unread ones.
- `user.unread_notifications(&db)` gives every unread one, newest first.
- `user.mark_all_notifications_read(&db)` marks them all read, and returns how many it changed.
- `user.mark_notification_read(&db, id)` returns `false` when that notification belongs to
  someone else, so one user can't touch another's list.

### Cleaning up old ones

Read notifications stay in the database until you **prune** them (delete the old ones).
The `Auth` module adds a command for it: `my-app notifications:prune --days 30`.
It deletes notifications that were read more than 30 days ago (30 is the default). Unread ones
stay, however old they are.

Run it on a schedule, or call `renox::auth::prune_read_notifications(&db, age)` from your own
scheduled task (see [scheduling.md](scheduling.md)):

```rust
use renox::prelude::*;
use std::time::Duration;

/// The app, with a task that deletes old read notifications every day at 04:00.
fn app() -> App {
    App::new().module(Auth::new()).schedule(|s| {
        s.daily_at("04:00", "prune-notifications", |state| async move {
            // 30 days, in seconds.
            let month = Duration::from_secs(30 * 24 * 60 * 60);
            renox::auth::prune_read_notifications(&state.db, month).await?;
            Ok(())
        });
    })
}
```

Deleting a user's account deletes their notifications too.

## Your own live events

The same stream can carry **your app's own events**: "order #7 was paid", "the export is
ready", "someone else edited this page". The page gets each one as a DOM event on `document`,
which htmx, Alpine or a script can listen to. No JavaScript of your own is needed.

```rust
use renox::prelude::*;

/// Marks an order paid, and tells every open page so its list can reload.
async fn mark_paid(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Toast> {
    // … save the order …
    state.broadcast("order-updated", json!({ "id": id, "status": "paid" }))?;
    Ok(Toast::success(format!("Order #{id} paid")))
}

/// Tells one user, on every page they have open, that their export is ready.
fn export_ready(state: &AppState, user_id: i64, url: &str) -> Result {
    // `renox:toast` is the event toasts arrive with: this shows one, with a link.
    let toast = Toast::success("Your export is ready").link("Download", url);
    state.broadcast_to(user_id, "renox:toast", json!({ "toasts": [toast] }))
}
```

What's going on:

- `state.broadcast(event, data)` goes to every open page; `state.broadcast_to(user_id, event,
  data)` only to that user's pages (every tab and device). `data` is anything serde can turn
  into JSON. The name may use letters, digits, `-`, `_`, `:` and `.`; anything else is an error.
- The page needs the stream: the bell (`notification_bell`) opens it, and so does
  `{{ event_stream() }}` from `renox/ui.html` on pages without a bell. Both need
  `Auth::new().notifications()`, and both are for logged-in users only. One page opens one
  stream, whatever it has.
- On the page, listen as to any DOM event (`event.detail` is the data):

```html
{# htmx: reload the list when an order changes #}
<div id="orders" hx-get="{{ route('orders.index') }}" hx-trigger="order-updated from:document"
     hx-select="#orders" hx-swap="outerHTML">…</div>

{# Alpine #}
<span x-data="{ status: 'unpaid' }" x-on:order-updated.document="status = $event.detail.status" x-text="status"></span>
```

**The limits, honestly.** A broadcast is fire-and-forget:

- **Nothing is stored.** A page that isn't connected at that moment never gets it: a closed
  tab, a laptop asleep, or the few seconds while a stream reconnects (each stream ends after
  five minutes and the browser opens a new one).
- **Only this process.** Broadcasts travel inside the server process that sends them. The
  queue workers and the scheduler that run inside `serve` are in that process, so their
  broadcasts arrive. A separate `queue:work` process, a command, or a second server behind a
  load balancer has its own streams: its broadcasts don't reach pages connected elsewhere.
  (Database notifications don't have this limit, because every stream also reads the table
  every 15 seconds; broadcasts aren't in a table.)
- **No promise of order across lagging pages.** A page that falls far behind (hundreds of
  events at once) skips some.

So use broadcasts for "look again" hints and short-lived news, where missing one only means
the page shows it on the next load. For what someone must see, store a database notification
(`state.notify`): it waits in the bell until read, and arrives from any process.

The stream's wire format, if you read it yourself: an SSE event named `broadcast` whose data is
`{"event": "order-updated", "data": {…}}`. renox-ui.js turns it into the DOM event.

## Testing

In tests, nothing should really be sent. `TestApp` uses the `memory` mailer, which keeps every
mail so you can check it:

- `app.sent_mail()` (or `app.mailer().sent()`) gives every `Mail` sent.
- `app.assert_mail_sent(to, subject)` checks that one was sent. `subject` only needs to be a
  part of the real subject.
- Queued mail is sent when the test runs the queue: `app.run_jobs().await`.
- `app.fake_notifications()` records notifications instead of delivering them: no mail, no
  database rows, no channel calls. This works whether they were sent with `notify` or
  `notify_later`.

```rust
use renox::prelude::*;
use renox::auth::{Notification, Recipient};
use renox::testing::TestApp;

/// A tiny notification for the test.
struct Hello;

impl Notification for Hello {
    // Mail only, by default. It has no `to_mail`, so sending it for real would fail;
    // here it's faked, so it is only recorded.
    fn kind(&self) -> &'static str { "hello" }
}

/// Checks a queued mail, then a faked notification.
#[renox::test]
async fn mail_and_notifications() {
    let app = TestApp::new(App::new().module(Auth::new())).await;
    let state = app.state();

    // Queue a mail, run the queue, then check it was sent.
    let mail = state.mail_view("ben@example.com", "Welcome", "renox/mail/layout", context! {}).unwrap();
    state.queue_mail(mail).await.unwrap();
    app.run_jobs().await;
    app.assert_mail_sent("ben@example.com", "Welcome");
    assert!(app.sent_mail()[0].is_for("ben@example.com"));

    // From here on, notifications are only recorded, not delivered.
    app.fake_notifications();
    let guest = Recipient::to("mail", "guest@example.com");
    state.notify(&guest, &Hello).await.unwrap();
    app.assert_notified_to("guest@example.com", "hello");
    // also app.assert_notified(&user, "hello"), app.notifications(), app.assert_nothing_notified()
}
```

Broadcasts have a fake too: after `app.fake_broadcasts()`, `state.broadcast(…)` is recorded
instead of sent, `app.broadcasts()` lists them, and
`app.assert_broadcast("order-updated", |b| b.data["id"] == 7)` checks one (`b.user_id` is
`Some(id)` for `broadcast_to`).

More in [testing.md](testing.md) ("Jobs, events, notifications, mail, HTTP").

## In production

A checklist for when your app is live:

- **Use `MAIL_MAILER=smtp`**, and in requests send with `queue_mail` / `notify_later`. Here's
  why: if the SMTP server is down or doesn't answer, a direct `mailer.send` fails after
  `MAIL_TIMEOUT`. Queued mail and notifications get five attempts instead, and then wait in
  `failed_jobs` until you run `queue:retry` (see [operations.md](operations.md), the failure
  table).
- **Think about a backup provider.** The built-in password-reset and verification mails are
  sent directly, in the request (not queued). A second provider in `MAIL_FAILOVER` keeps them
  going when the first one is down.
- **Prune read notifications** (see [Cleaning up old ones](#cleaning-up-old-ones) above).

## Coming from Laravel

If you know Laravel, this table maps what you know to Renox. If you don't, you can skip it.

| Laravel | Renox |
|---|---|
| `MAIL_MAILER=smtp\|log\|array` | `MAIL_MAILER=smtp\|log\|memory` |
| `Mailable` class + Blade view | `state.mail_view(to, subject, "mail/x", ctx)` (`.html` + `.txt`) |
| `Mail::to()->cc()->bcc()`, `replyTo`, `from`, `attach` | `Mail` builder: `also_to`, `cc`, `bcc`, `reply_to`, `from`, `attach` |
| `Mail::send` / `Mail::queue` | `state.mailer.send(mail)` / `state.queue_mail(mail)` |
| `Mail::mailer('postmark')`, the `failover` transport | `App::mailer(name, …)` + `state.mailer_named(name)` / `queue_mail_via`, `MAIL_FAILOVER` |
| Markdown mail components (`x-mail::button`, `panel`, `table`) | `renox/mail/components.html`: `button`, `panel`, `table`, `divider` |
| `Mail::to($u)->locale('es')` | `state.mail_view_in("es", …)` |
| `php artisan make:mail` | `rnx make:mail` (templates only) |
| Mail preview packages, Mailpit | `/_renox/mail` (debug) |
| `Notification` with `via()` | `Notification` with `channels(to)` |
| `toMail`, `toDatabase` / `toArray`, custom channels | `to_mail`, `to_database`, `to_channel` + `App::channel` |
| `$user->notify()`, `ShouldQueue` | `state.notify(&user, &n)`, `state.notify_later(&user, &n)` |
| `Notification::route('mail', …)` (on-demand) | `Recipient::to("mail", …).and(…)` + `state.notify` |
| `HasLocalePreference` | `Recipient::in_locale` or a `users.locale` column |
| `$user->notifications`, `unreadNotifications`, `markAsRead`, `markAsUnread` | `user.notifications(&db, n)`, `notifications_before`, `unread_notifications`, `mark_notification_read`, `mark_notification_unread`, `mark_all_notifications_read`, `delete_notification`, `delete_notifications` |
| Filament's `Notification::make()->title()->body()->sendToDatabase($user)` | `DatabaseMessage::success(title).body(…).url(…)` from `to_database` |
| Filament's database notifications modal, polling or Echo | `Auth::new().notifications()` + `notification_bell(unread_notifications)`, Server-Sent Events |
| Broadcasting with Echo (`broadcast()`, `ShouldBroadcastNow`, private user channels) | `state.broadcast(event, data)` / `broadcast_to(user_id, …)` over the same stream, as DOM events (one process; nothing stored) |
| Filament's notification actions (`->button()->dispatch()` / `->url()`) | `ToastAction::post` / `put` / `patch` / `delete`, `ToastAction::link`, `ToastAction::event` |
| `model:prune` on notifications | `notifications:prune --days 30` |
| `Mail::fake()`, `Notification::fake()`, `assertSentTo` | memory mailer + `sent_mail()` / `assert_mail_sent`, `fake_notifications()` + `assert_notified` / `assert_notified_to` |
| `Event::fake()` + `assertDispatched` for broadcasts | `fake_broadcasts()` + `assert_broadcast` / `broadcasts()` |
