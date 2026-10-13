# Live components

A **live component** is a piece of a page whose state lives in a Rust struct. A button calls a
method of the struct, the server changes the state, renders the component again and sends back
its HTML, and the browser morphs the page into it. You write one struct, one template and no
route, no fragment, no out-of-band swap, no JavaScript. It is Renox's answer to Livewire.

htmx and Alpine stay the base of the framework (see [ui.md](ui.md)); a live component is the
tool for a part of a page with several states and several actions. The bike shop shows both
on the same checklist: [`/about/htmx`](../examples/bikeshop/src/app/about/htmx.rs) with
fragments and headers, and
[`/about/htmx/live`](../examples/bikeshop/src/app/about/live.rs) as one live component.

- [How it works](#how-it-works)
- [The component](#the-component)
- [The attribute macro](#the-attribute-macro)
- [Mounting](#mounting)
- [The template and its attributes](#the-template-and-its-attributes)
- [Model modes](#model-modes)
- [Inside an action](#inside-an-action)
- [The snapshot and its security](#the-snapshot-and-its-security)
- [Trade-offs](#trade-offs)
- [Testing](#testing)

## How it works

1. A page handler **mounts** the component. The page contains a wrapper
   `<div data-rx-live="counter" data-rx-snapshot="…">` with the component's template inside.
   The snapshot is the component's state, signed.
2. `rx-click="increment"` (or `rx-submit`, or an `rx-model` field) makes the browser post to
   `POST /_renox/live/counter/increment` with the snapshot and the form's fields.
3. The server checks the snapshot, rebuilds the struct, runs the action, renders the template
   again and answers with the wrapper's new HTML.
4. The browser morphs the old wrapper into the new one with
   [idiomorph](https://github.com/bigskysoftware/idiomorph): only what changed is touched, and
   the focused field keeps its cursor.

The request goes through htmx, so the CSRF token, toasts, `HX-Redirect` and `HX-Trigger` work
as they do everywhere else. The route runs inside the session, authentication, CSRF and
maintenance layers like any page.

## The component

A component is a struct that implements `LiveComponent`. Its fields are the state, so they
must be `Serialize + Deserialize`. `call` is a `match` on the action's name:

```rust
use renox::prelude::*;
use renox::live_component::LiveContext;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Counter {
    count: i64,
}

impl LiveComponent for Counter {
    const NAME: &'static str = "counter";
    const VIEW: &'static str = "live/counter.html";

    async fn call(
        &mut self,
        action: &str,
        args: Vec<serde_json::Value>,
        _ctx: &mut LiveContext,
    ) -> Result {
        match action {
            "increment" => self.count += 1,
            "add" => self.count += renox::live_component::arg::<i64>(&args, 0)?,
            _ => return Err(Error::NotFound),
        }
        Ok(())
    }
}
```

- `NAME` is the component's name in the URL and the snapshot. It must be unique.
- `VIEW` is the template that renders it.
- `data` (optional) returns extra values for the view, such as rows read from the database. They
  are shown as `data` and are **not** part of the snapshot.
- `call` runs an action. Answer unknown names with `Error::NotFound`.
- Names starting with `_` are reserved. `_refresh` is built in: it applies the posted fields
  and renders again.

Register the component once, in the app (or a module's `register`):

```rust
# use renox::prelude::*;
# use renox::live_component::LiveContext;
# #[derive(serde::Serialize, serde::Deserialize)]
# struct Counter { count: i64 }
# impl LiveComponent for Counter {
#     const NAME: &'static str = "counter";
#     const VIEW: &'static str = "live/counter.html";
#     async fn call(&mut self, _: &str, _: Vec<serde_json::Value>, _: &mut LiveContext) -> Result { Ok(()) }
# }
fn app() -> App {
    App::new().live_component::<Counter>()
}
```

Using one name twice is an error when the app boots.

## The attribute macro

Matching on strings and reading arguments by hand is what the attribute macro writes for you.
Put `#[renox::live_component(view = "…")]` on an `impl` block and mark the actions with
`#[live(action)]`:

```rust
use renox::prelude::*;
use renox::live_component::LiveContext;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Todos {
    done: Vec<i64>,
    filter: String,
}

#[renox::live_component(view = "live/todos.html", name = "todos")]
impl Todos {
    /// Extra values for the view (optional).
    async fn data(&self, _ctx: &LiveContext) -> Result<serde_json::Value> {
        Ok(json!({ "shown": self.done.len() }))
    }

    #[live(action)]
    async fn toggle(&mut self, _ctx: &mut LiveContext, id: i64) -> Result {
        if let Some(at) = self.done.iter().position(|d| *d == id) {
            self.done.remove(at);
        } else {
            self.done.push(id);
        }
        Ok(())
    }

    #[live(action)]
    async fn clear(&mut self, _ctx: &mut LiveContext) -> Result {
        self.done.clear();
        Ok(())
    }
}
```

`name` defaults to the type's name in kebab-case. An action takes `&mut self`, the context,
and then its own arguments, read by position from the call (`toggle(3)`). A wrong count or a
value of the wrong type answers `400`. It is an attribute on the **impl** because a derive on
the struct cannot see the methods.

## Mounting

A page handler takes a `LiveContext` like any extractor, mounts the component with its
starting state and hands the result to the template:

```rust
# use renox::prelude::*;
# use renox::live_component::LiveContext;
# #[derive(serde::Serialize, serde::Deserialize, Default)]
# struct Counter { count: i64 }
# impl LiveComponent for Counter {
#     const NAME: &'static str = "counter";
#     const VIEW: &'static str = "live/counter.html";
#     async fn call(&mut self, _: &str, _: Vec<serde_json::Value>, _: &mut LiveContext) -> Result { Ok(()) }
# }
async fn page(ctx: LiveContext) -> Result<View> {
    let counter = ctx.mount(Counter::default()).await?;
    Ok(view("counter.html", context! { counter }))
}
```

In the page's template, write the tag (the name after `live-` is for the reader; the component
comes from `:component`):

```html
<live-counter :component="counter"/>
```

The tag takes no content. Other attributes land on the wrapper (`class`, `data-*`), and
`@saved="open = false"` listens to the events the component dispatches (see
[ui.md](ui.md#components-as-tags)). It compiles to the include form, which still works:

```html
{% with component = counter %}{% include "renox/live.html" %}{% endwith %}
```

`renox/live.html` writes the wrapper, includes the component's view and, once per page, the
two scripts (`renox-live.js` and idiomorph). The layout needs nothing else.

## The template and its attributes

Inside the component's view, `state` is the struct as JSON and `data` is what `data` returned.
Three attributes talk to the server:

```html
<p>{{ state.count }}</p>

<button type="button" rx-click="increment">+1</button>
<button type="button" rx-click="add(10)">+10</button>
<button type="button" rx-click='rename("Main")'>Rename</button>

<form rx-submit="save">
  <input name="title" rx-model="title" value="{{ state.title }}">
  <button>Save</button>
</form>
```

- **`rx-click="name"`** calls the action when the element is clicked.
- **`rx-click="name(args)"`**: the text in the parentheses is read as a JSON list, so strings
  need double quotes (put the attribute in single quotes). The arguments are sent as `_args`.
- **`rx-submit="name"`** on a `<form>` calls the action when it is submitted (Enter works) and
  sends the form's fields.
- **`rx-model="field"`** keeps a state field in step with an input (below).

A component renders in place, so it is a fragment of the page: it has one root of its own (the
wrapper) and cannot contain another live component.

## Model modes

Fields posted with a call overwrite the state's top-level keys of the same name (not names
starting with `_`) before the action runs. Each text is converted to the type the field holds
now:

| The field holds | The posted text becomes |
|---|---|
| a number | an integer, else a float, else `400` |
| a bool | true for `true`, `1` and `on` (a missing checkbox is false) |
| a string | the string |
| `null` | the string; an empty text stays `null` |

Nested fields, arrays and files are not read.

The three modes decide **when** the browser sends the model:

| Attribute | When | Use for |
|---|---|---|
| `rx-model` | with the next action (a click or a submit) | forms: the state is updated when the user presses Save |
| `rx-model.live` | 300 ms after each keystroke, as `_refresh` | search boxes and live previews |
| `rx-model.blur` | when the field changes and loses focus, as `_refresh` | a value checked or saved when the user leaves it |

`_refresh` runs no action: it applies the fields and renders again, so a template that shows
`state.query` follows the typing. The field being typed in keeps its cursor and its text.

## Inside an action

The context gives an action what a handler has:

```rust
# use renox::prelude::*;
# use renox::live_component::LiveContext;
# #[derive(serde::Serialize, serde::Deserialize, Default)]
# struct Note { text: String }
# impl Validate for Note {
#     fn rules(&self, v: &mut Validator) { v.field("text", &self.text).required().max(200); }
# }
#[renox::live_component(view = "live/note.html")]
impl Note {
    #[live(action)]
    async fn save(&mut self, ctx: &mut LiveContext) -> Result {
        ctx.validate(&*self).await?;       // a 422 with the field errors, shown by the view
        let _user = ctx.user()?;           // Error::Unauthorized when nobody is signed in
        let _db = &ctx.state().db;         // the application state
        ctx.toast(Toast::success("Saved")); // a toast with the answer
        ctx.dispatch("saved", json!({ "id": 7 })); // the event rx:note:saved on the component
        Ok(())
    }

    #[live(action)]
    async fn finish(&mut self, ctx: &mut LiveContext) -> Result {
        ctx.redirect("/notes"); // the browser navigates there (HX-Redirect)
        Ok(())
    }
}
```

- `ctx.state()`, `ctx.user()` and `ctx.session()` give the request's application state, user and
  session. Policies are the usual `ctx.user()?.authorize("update", &record)?`.
- `ctx.validate(&value)` runs a `Validate` value's rules. A failure answers `422`, and the
  errors appear next to the inputs, as with any htmx form.
- `ctx.toast(Toast)`, `ctx.redirect(url)` and `ctx.dispatch(name, detail)` are the effects
  that travel with the answer. `dispatch` sends `HX-Trigger` with `rx:<component>:<name>`,
  which htmx fires on the wrapper, so Alpine or a script can listen with
  `@rx:note:saved="…"`.
- The error of an action is the response: `Error::NotFound` is a 404, `Error::BadRequest` a 400.

## The snapshot and its security

The state travels in `data-rx-snapshot`: `base64url(json)` followed by `.` and a hex
HMAC-SHA256 of it, keyed from `APP_KEY`. The server trusts only what it signed.

- **Signed, not encrypted.** Anyone with the page can read the state. **Keep secrets out of the
  component's fields**: tokens, other people's data, prices that decide a payment. Keep ids in
  the state and read the rest from the database in `data` or in the action.
- **Authorize in the action.** The snapshot proves the state came from the server, not that this
  user may do what the action does. Check the policy in every action that changes something.
- **Size.** The snapshot is sent back with every call. `LIVE_SNAPSHOT_MAX_SIZE` (in KB, 64 by
  default) limits it: rendering a component over the limit is a `500` that says so, and
  receiving one is a `400`. Keep large lists in the database.
- **A bad snapshot is a `400`**: a changed body, a wrong signature, a snapshot of another
  component or a malformed one. So is a snapshot signed with an older `APP_KEY`: after the key
  is rotated, open pages answer "This page is out of date. Reload it and try again." until they
  reload.
- Names starting with `_` can't be called from the browser, and `_` fields are never merged.

## Trade-offs

Live components are the right tool for a part of a page that has state. Know what they cost:

- **No fallback without JavaScript.** `rx-click` does nothing without the script. A page that
  must work without it should use plain forms and htmx fragments ([ui.md](ui.md)).
- **The whole component renders again** on every call. The browser changes only what differs,
  but the server does the whole template. Keep components small and `data` cheap.
- **A change is a recompile.** The actions are Rust, so adding one means building the app
  (the template reloads in debug mode as always).
- **No nested components.** A live component can't contain another one; split the page into
  sibling components.
- **No throttle.** The route has no rate limit in v1 and takes no `App::layer` layers. Put
  expensive work behind the action's own checks.
- Not in v1: files and modifiers such as `rx-click.prevent`.

## Testing

`TestApp::live(component)` drives a component without a browser. It signs the snapshot, posts
actions to the real route and keeps the new snapshot between calls, as the page does:

```rust
# use renox::prelude::*;
# use renox::live_component::LiveContext;
# use renox::testing::TestApp;
# #[derive(serde::Serialize, serde::Deserialize, Default)]
# struct Counter { count: i64 }
# #[renox::live_component(view = "live/counter.html")]
# impl Counter {
#     #[live(action)]
#     async fn add(&mut self, _ctx: &mut LiveContext, n: i64) -> Result { self.count += n; Ok(()) }
# }
# async fn demo() {
let app = TestApp::new(App::new().live_component::<Counter>()).await;

let mut counter = app.live(Counter::default());
counter.call_with("add", json!([5])).await.assert_ok();
assert_eq!(counter.component().count, 5);

counter.set("count", 40);                   // a model field, sent with each call
counter.call("_refresh").await.assert_ok(); // applies it
assert_eq!(counter.component().count, 40);

counter.call("nope").await.assert_status(404);
# }
```

- `set(name, value)` sets a form field that is sent with every call from then on.
- `call(action)` and `call_with(action, args)` return a `TestResponse`; `args` is a JSON array.
- `component()` reads the struct back from the last answer's snapshot.

The answer is the component's HTML, so `assert_see` and the other response assertions work on
it. Browser behaviour (morphing, focus, `rx-model.live`) is covered by the framework's own
browser tests ([testing.md](testing.md)).
