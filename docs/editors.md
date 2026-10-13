# Rich text, Markdown and code editors

Some fields need more than a text box: an article with bold words and links, notes written in
Markdown, a piece of JSON or a script. The `renox-editors` crate adds three form fields for
them, and a way to show code on a detail page:

- `<rx-rich-editor>`: a word-processor-like editor (bold, italic, links, headings, quotes, lists)
  that sends HTML;
- `<rx-markdown-editor>`: a text area with a formatting toolbar and a **Preview** button;
- `<rx-code-editor>`: a code editor that colours the code as you type;
- `<rx-code-entry>`: code shown coloured and read-only in an infolist, with a copy button.

They're a separate crate, not part of the UI kit, because they bring JavaScript libraries
(about 270 KB) that most pages don't need. A page loads them only when it uses one of these
fields, and each library only when the page has a field that needs it.
[examples/bikeshop](../examples/bikeshop)'s admin panel writes product descriptions with `markdown_editor`.

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [The fields in a form](#the-fields-in-a-form)
- [Reading the form](#reading-the-form)
- [Rich text is cleaned on the server](#rich-text-is-cleaned-on-the-server)
- [Showing what was saved](#showing-what-was-saved)
- [Options](#options)
- [Keyboard and screen readers](#keyboard-and-screen-readers)
- [Texts in other languages](#texts-in-other-languages)
- [How the files load](#how-the-files-load)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **rich text** | Text with formatting (bold, links, lists), stored as HTML. |
| **sanitize** | Remove everything from HTML except an allowed list of harmless tags, so it can't run scripts in someone's browser. |
| **XSS** | "Cross-site scripting": an attacker's script running on your pages, for example through HTML a user typed. Sanitizing rich text prevents it. |
| **syntax highlighting** | Colouring code by what each part is (keywords, strings, numbers). |

> [!NOTE]
> **Coming from Laravel:** these are Filament's `RichEditor`, `MarkdownEditor` and
> `CodeEditor` form fields, and its `CodeEntry` infolist entry.

## Add it to your app

Add the crate next to `renox`, at the same version:

```toml
[dependencies]
renox = "1.1"
renox-editors = "1.1"
```

Then add its module:

```rust
use renox::prelude::*;
use renox_editors::Editors;

pub fn app() -> App {
    App::new().module(Editors::new())
}
```

The module adds the fields' tags (`<rx-rich-editor>`, `<rx-markdown-editor>`, `<rx-code-editor>`, `<rx-code-entry>`), the `rich_text`
filter, the route the Markdown preview uses (`POST /_renox/editors/preview`, named
`editors.preview`) and the editors' scripts and styles under `/_renox/editors/`. The fields
are built on the UI kit, so the page's layout needs `{{ renox_ui() }}` as for the kit's own
fields.

## The fields in a form

Use the plugin's tags like the kit's fields:

```html
<rx-form route="posts.store" live>
  <rx-card title="New post">
    <rx-input name="title" label="Title" :value="post.title" required/>
    <rx-rich-editor name="body" label="Body" :value="post.body" required/>
    <rx-markdown-editor name="notes" label="Notes" :value="post.notes" hint="Markdown"/>
    <rx-code-editor name="settings" label="Settings" :value="post.settings" language="json"/>
    <rx-button label="Save"/>
  </rx-card>
</rx-form>
```

`<rx-code-entry label="Settings" :value="post.settings" language="json"/>` shows code in an
infolist (see below). The tags take the arguments listed under "Options"; the macros they call
live in `renox-editors/editors.html`.

Each one sends a plain form field named like its first argument:

| Field | What it sends | Without JavaScript |
|---|---|---|
| `<rx-rich-editor name="body">` | the HTML, in a hidden `body` input | not editable |
| `<rx-markdown-editor name="notes">` | the Markdown as typed, from a `notes` textarea | a plain textarea |
| `<rx-code-editor name="settings">` | the code as typed, from a `settings` textarea | a plain textarea |

So they behave like the kit's other fields: after a failed submit they show what was typed
(`old`), their error appears under them, a `hint` goes under the label, `required` drops
the "(optional)" note, and a `<rx-form live>` checks them when you leave them.

## Reading the form

Read the Markdown and the code as `String`s. Read rich text as a `RichText`:

```rust
use renox::prelude::*;
use renox_editors::RichText;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
struct PostForm {
    #[validate(required, max = 200)]
    title: String,
    /// `required` fails for an emptied editor; `max` counts letters, not tags.
    #[validate(required, max = 20000)]
    body: RichText,
    notes: Option<String>,
    #[validate(json)]
    settings: Option<String>,
}

async fn store(State(db): State<Db>, Valid(form): Valid<PostForm>) -> Result<Redirect> {
    renox::db::sql("INSERT INTO posts (title, body, notes, settings) VALUES (?, ?, ?, ?)")
        .bind(form.title)
        .bind(form.body.into_string()) // cleaned HTML, stored as text
        .bind(form.notes)
        .bind(form.settings)
        .execute(&db)
        .await?;
    Ok(Redirect::to("/posts"))
}
```

A rich text editor that was emptied still sends a little markup (`<div><br></div>`), so a plain
`String` with `required` would pass. `RichText`'s rules look at its text instead: `required`
fails when there are no words, and `min`/`max` count letters, not tags. `RichText::text()`
gives the words alone (for a search index, an excerpt or a mail's text part), and
`is_empty()` says whether there are any.

Store `RichText` as text: `into_string()`, `as_str()`, or bind it as it is (it implements
`ToDbValue`). A model field for it is a `String` (or `Option<String>`).

## Rich text is cleaned on the server

Never show HTML from a user as it came. Anyone can send a form field with a `<script>`, an
`onerror` attribute or a `javascript:` link, whatever the editor on your page allows. Renox
cleans rich text twice:

1. **When the form is read.** `RichText` passes the HTML through [`ammonia`][ammonia], an HTML
   sanitizer, with an allowlist: paragraphs and `div`s, line breaks, bold, italic, underline,
   struck text, headings `h1`–`h3`, quotes, `pre` and `code`, lists, and links. Links keep only
   `http`, `https`, `mailto` and `tel` addresses (and relative ones like `/pricing`), and get
   `rel="noopener noreferrer nofollow"`. Everything else is removed: scripts and styles with
   their content, event handlers, `style` and `class` attributes, images, frames and forms.
2. **When it is shown.** The `rich_text` filter cleans stored HTML again, the same way:
   `{{ post.body | rich_text }}`. HTML stored before you added the editor, or written to the
   database by something else, is safe too.

The same cleaning is `renox_editors::sanitize(html)`, for HTML from anywhere else:

```rust
use renox_editors::sanitize;

let clean = sanitize(r#"<p onclick="steal()">Hi <a href="javascript:alert(1)">there</a></p><script>steal()</script>"#);
assert_eq!(clean, r#"<p>Hi <a rel="noopener noreferrer nofollow">there</a></p>"#);
```

Markdown and code need no cleaning: they're text. The `markdown` filter shows any HTML in
Markdown as text, and templates escape code.

[ammonia]: https://docs.rs/ammonia

## Showing what was saved

```html
<article class="rx-prose">{{ post.body | rich_text }}</article>

<rx-infolist columns="2">
  <rx-entry label="Notes" :value="post.notes" format="markdown" span="full"/>
  <rx-code-entry label="Settings" :value="post.settings" language="json"/>
  <rx-code-entry label="Webhook payload" :value="call.payload"/>
</rx-infolist>
```

`<rx-code-entry>` shows the code coloured, in a box that scrolls sideways for long lines, with a
copy button (`copyable=false` to leave it out). A value that isn't text, such as a map or a
list from `json!`, is shown as indented JSON (and coloured as JSON unless you give a
`language`). `max_height="20rem"` makes long code scroll inside the box; `placeholder` stands
in for no value ("—").

## Options

All three fields take the kit's usual arguments: `value`, `hint`, `required`, `placeholder`,
`id` (for two fields of one name on a page), `disabled`, `span` (columns in a `form_grid`),
`hide_label` and `bag` (a named error bag). And:

| Field | Its own options |
|---|---|
| `<rx-rich-editor>` | `rows` (the height in lines before it scrolls, 8) |
| `<rx-markdown-editor>` | `rows` (8), `readonly` |
| `<rx-code-editor>` | `language` (below), `rows` (10), `tab` (what Tab inserts: two spaces), `readonly` |
| `<rx-code-entry>` | `language`, `copyable` (true), `max_height`, `placeholder` ("—"), `hint`, `span` ("full"), `hide_label`, `id` |

Languages: `rust`, `json`, `javascript`, `typescript`, `html`, `css`, `sql`, `bash`, `python`,
`yaml`, `toml`, `markdown`, `diff` and `go`; `none` (the default) for plain text.

The rich text editor doesn't take files: dropping or pasting an image into it does nothing.
Upload images with the kit's `file` field instead.

## Keyboard and screen readers

- Each editor is named by its label, described by its hint and error, and marked invalid when
  it has an error, like the kit's fields.
- The toolbars are buttons with names ("Bold", "Bulleted list") and tooltips; the rich
  editor's formatting buttons say whether they're on (`aria-pressed`). Ctrl (⌘ on a Mac) with
  B, I or K makes text bold or italic or adds a link, in the rich text and the Markdown
  editors; Ctrl+Z and Ctrl+Shift+Z undo and redo.
- In the code editor, Tab indents. To leave it with the keyboard, press Escape, then Tab
  (screen readers announce this).
- The Markdown preview is a region that's announced when it changes; press Preview again to
  go back to writing. A required Markdown field left empty switches back to writing when the
  form is sent, so the browser can point at it.

## Texts in other languages

The toolbar's texts are English unless your app's lang files have them:

```json
{
  "editors.formatting": "Formato",
  "editors.bold": "Negrita",
  "editors.italic": "Cursiva",
  "editors.strike": "Tachado",
  "editors.link": "Enlace",
  "editors.unlink": "Quitar enlace",
  "editors.url": "Dirección del enlace",
  "editors.heading": "Título",
  "editors.quote": "Cita",
  "editors.code": "Código",
  "editors.bullets": "Lista con viñetas",
  "editors.numbers": "Lista numerada",
  "editors.undo": "Deshacer",
  "editors.redo": "Rehacer",
  "editors.preview": "Vista previa",
  "editors.preview_empty": "Nada que mostrar todavía.",
  "editors.preview_failed": "No se pudo cargar la vista previa.",
  "editors.code_keys": "Tab sangra. Para salir del editor, pulsa Escape y después Tab."
}
```

To change more than the texts, copy the macros into your app as
`resources/views/renox-editors/editors.html`: your file replaces the crate's.

## How the files load

The first editor or code entry on a page prints two tags: the editors' stylesheet and
`editors.js`, a JavaScript module. The module looks at the page and loads what it needs: Trix
for a rich text editor, CodeJar and Prism for a code editor or a code entry. Libraries and
versions (all MIT, listed with their sources in the crate's `assets/vendor/NOTICE`):

| Library | Version | Used for |
|---|---|---|
| [Trix](https://trix-editor.org) | 2.1.19 | the rich text editor |
| [CodeJar](https://medv.io/codejar/) | 4.3.0 | the code editor |
| [Prism](https://prismjs.com) | 1.30.0 | the colours in the code editor and the code entry |

They're compiled into the crate, so there's nothing to install and no build step. The app
serves them itself, from addresses that carry their version or a hash of their content, with
a year-long cache and no session cookie. They work under both of Renox's content security
policies (`CSP=relaxed`, the default, and `CSP=strict` with its nonces): they're scripts from
your own site, with no inline code.

A browser runs a module once per page, so a form that htmx brings in later (in a sheet, or a
swapped section) works too: its editors start when it arrives.

## Testing

The fields are plain form fields, so test your handlers by posting what the browser would
send. For rich text, send HTML, and check that what you stored is clean:

```rust
use renox::prelude::*;
use renox::testing::TestApp;
use renox_editors::RichText;

#[derive(serde::Deserialize, Validate)]
struct NoteForm {
    #[validate(required)]
    body: RichText,
}

struct Notes;

impl Module for Notes {
    fn name(&self) -> &'static str { "notes" }

    fn routes(&self) -> Routes {
        Routes::new().post("/notes", |Valid(form): Valid<NoteForm>| async move {
            form.body.into_string()
        })
    }
}

#[renox::test]
async fn notes_are_stored_clean() {
    let app = TestApp::new(App::new().module(renox_editors::Editors::new()).module(Notes)).await;
    app.post("/notes", &[("body", "<b>Hi</b><img src=x onerror=alert(1)>")])
        .await
        .assert_ok()
        .assert_see("<b>Hi</b>")
        .assert_dont_see("onerror");
    // An emptied editor is "required".
    app.htmx()
        .post("/notes", &[("body", "<div><br></div>")])
        .await
        .assert_invalid("body");
}
```

`TestApp` doesn't run JavaScript, so check the editors themselves in a browser (see
[testing.md](testing.md)).
