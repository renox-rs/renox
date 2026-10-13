# renox-editors

Rich text, Markdown and code editor fields, and a code entry for detail pages, for
[Renox](https://github.com/renox-rs/renox) apps. They bring JavaScript the UI kit doesn't
carry (Trix, CodeJar and Prism, compiled into this crate, no build step), and a page loads it
only when it has one of them.

```rust
use renox::prelude::*;
use renox_editors::Editors;

App::new().module(Editors::new())
```

```html
<rx-rich-editor name="body" label="Body" :value="post.body" required/>
<rx-markdown-editor name="notes" label="Notes" :value="post.notes"/>
<rx-code-editor name="settings" label="Settings" :value="post.settings" language="json"/>
```

What it adds:

- three form fields that each send a plain field of their name, so `Valid<T>`, old input,
  errors, hints and live validation work as for the kit's own fields;
- `RichText`, the rich editor's HTML cleaned on the server as it is read (an allowlist of
  formatting tags; no scripts, event handlers, styles or `javascript:` links), and the
  `rich_text` filter that cleans stored HTML again when it is shown;
- the Markdown editor's preview, rendered on the server by the `markdown` filter;
- `<rx-code-entry>`, code shown highlighted in an infolist, read-only, with a copy button.

The bundled libraries, their versions and licences are listed in
[assets/vendor/NOTICE](assets/vendor/NOTICE). The guide is
[docs/editors.md](https://github.com/renox-rs/renox/blob/main/docs/editors.md);
examples/bikeshop uses it (its /about/fields form and the admin panel). Versioned with `renox`: use the same version for both.
