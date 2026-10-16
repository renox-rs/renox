//! Settings pages: a form for a group of settings kept in the database
//! (`renox::settings`), added with [`Admin::settings`](crate::Admin::settings).
//! The form comes from the fields [`AdminSettings::fields`] declares; saving
//! runs the group's own rules and goes through `AppState::save_settings`,
//! so the change is audited and emits `SettingsChanged`.

use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::Arc;

use renox::axum::Extension;
use renox::axum::body::{Body, Bytes};
use renox::axum::extract::{FromRequest, Request};
use renox::axum::http::header::CONTENT_TYPE;
use renox::prelude::*;
use renox::serde_json::{Map, Value};
use renox::settings::SettingsGroup;

use crate::field::FieldKind;
use crate::panel::{Panel, fields_view, money_names, parse_pairs, read_form, to_json};
use crate::texts::Texts;
use crate::{Field, panel};

/// A group of settings the admin panel can edit: the fields of its form,
/// and who may open or change it.
///
/// ```
/// use renox::prelude::*;
/// use renox::settings::SettingsGroup;
/// use renox_admin::{Admin, AdminSettings, Field};
///
/// #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
/// struct Company {
///     #[validate(required, max(120))]
///     name: String,
///     tax_number: Option<String>,
///     registered_on: Option<String>,
/// }
///
/// impl SettingsGroup for Company {
///     const KEY: &'static str = "company";
/// }
///
/// impl AdminSettings for Company {
///     fn fields() -> Vec<Field> {
///         vec![
///             Field::text("name", "Company name").required(),
///             Field::text("tax_number", "Tax number"),
///             // Shown, but only changed by a seed file or `settings:set`.
///             Field::text("registered_on", "Registered on").readonly_on_edit(),
///         ]
///     }
///
///     fn allows(user: &AuthUser, ability: &str) -> bool {
///         ability == "view" || user.has_role("owner") // owners change it
///     }
/// }
///
/// # let _ =
/// Admin::new().authorize(|user| user.has_role("staff")).settings::<Company>("Company")
/// # ;
/// ```
pub trait AdminSettings: SettingsGroup {
    /// The form's fields, named like the group's struct fields. A field
    /// marked [`readonly_on_edit`](Field::readonly_on_edit) is shown but its
    /// stored value is kept whatever is sent.
    fn fields() -> Vec<Field>;

    /// Whether `user` may do `ability` on the page: `"view"` (open it) or
    /// `"update"` (save it). Everyone the panel lets in may by default.
    fn allows(user: &AuthUser, ability: &str) -> bool {
        let _ = (user, ability);
        true
    }
}

/// A settings page as the panel lists it, whatever its group.
pub(crate) trait SettingsPage: Send + Sync {
    fn key(&self) -> &str;
    fn label(&self) -> &str;
    fn visible(&self, user: &AuthUser) -> bool;
    /// Declares the group (`App::settings`) so the commands know it too.
    fn declare(&self, registry: &mut Registry);
    fn routes(&self, panel: Arc<Panel>) -> Routes;
}

pub(crate) struct Page<S> {
    label: String,
    group: PhantomData<fn() -> S>,
}

impl<S> Page<S> {
    pub(crate) fn new(label: &str) -> Self {
        Self {
            label: label.to_owned(),
            group: PhantomData,
        }
    }
}

impl<S: AdminSettings> SettingsPage for Page<S> {
    fn key(&self) -> &str {
        S::KEY
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn visible(&self, user: &AuthUser) -> bool {
        S::allows(user, "view")
    }

    fn declare(&self, registry: &mut Registry) {
        registry.settings::<S>();
    }

    fn routes(&self, panel: Arc<Panel>) -> Routes {
        let path = format!("/settings/{}", S::KEY);
        Routes::new()
            .get(&path, show::<S>)
            .name(&format!("settings.{}.show", S::KEY))
            .put(&path, update::<S>)
            .name(&format!("settings.{}.update", S::KEY))
            .route_layer(Extension(Ctx::<S> {
                panel,
                label: self.label.clone(),
                group: PhantomData,
            }))
            .require_auth()
    }
}

/// What a settings page's handlers share.
struct Ctx<S> {
    panel: Arc<Panel>,
    label: String,
    group: PhantomData<fn() -> S>,
}

impl<S> Clone for Ctx<S> {
    fn clone(&self) -> Self {
        Self {
            panel: self.panel.clone(),
            label: self.label.clone(),
            group: PhantomData,
        }
    }
}

impl<S: AdminSettings> Ctx<S> {
    fn url(&self) -> String {
        format!("{}/settings/{}", self.panel.path, S::KEY)
    }

    /// The panel lets `user` in and the group's policy allows `ability`.
    fn authorize(&self, user: &AuthUser, ability: &str) -> Result {
        self.panel.check(user)?;
        if S::allows(user, ability) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}

async fn show<S: AdminSettings>(
    Extension(cx): Extension<Ctx<S>>,
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<View> {
    cx.authorize(&user, "view")?;
    let texts = Texts::new(&state);
    let settings = state.settings::<S>().await?;
    let info = state.settings_info::<S>().await?;
    let mut fields = S::fields();
    // A sealed group never prints its values: the fields start empty, and a
    // hint says whether they are set.
    let record = if S::ENCRYPTED {
        let hint = match &info {
            Some(info) => texts.get(
                "sealed_set",
                &[(
                    "when",
                    &info.updated_at.format("%Y-%m-%d %H:%M UTC").to_string(),
                )],
            ),
            None => texts.get("sealed_unset", &[]),
        };
        fields = fields
            .into_iter()
            .map(|field| match field.kind() {
                FieldKind::Checkbox | FieldKind::Toggle => field,
                _ => field.hint(&hint),
            })
            .collect();
        Value::Null
    } else {
        to_json(&*settings)?
    };
    let key = format!("settings/{}", S::KEY);
    let views = fields_view(fields, &state, Some(&record), &texts, S::KEY, &[]).await?;
    let mut values = Map::new();
    let label = texts.label("settings", &cx.label);
    values.insert("title".into(), Value::String(label.clone()));
    values.insert("fields".into(), to_json(&views)?);
    values.insert("action".into(), Value::String(cx.url()));
    values.insert("method".into(), Value::String("PUT".into()));
    values.insert(
        "submit_label".into(),
        Value::String(texts.get("save_changes", &[])),
    );
    values.insert("sealed".into(), Value::Bool(S::ENCRYPTED));
    values.insert("editable".into(), Value::Bool(S::allows(&user, "update")));
    values.insert(
        "admin".into(),
        to_json(&cx.panel.frame(&state, &user, Some(&key), &texts))?,
    );
    Ok(view("renox-admin/settings.html", Value::Object(values)))
}

async fn update<S: AdminSettings>(
    Extension(cx): Extension<Ctx<S>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    req: Request,
) -> Result<Response> {
    cx.authorize(&user, "update")?;
    let texts = Texts::new(&state);
    let current = to_json(&*state.settings::<S>().await?)?;
    let fields = S::fields();
    let (req, sent) = match buffered(&state, req).await {
        Ok(read) => read,
        Err(answer) => return Ok(*answer),
    };
    // What the form must not change: read-only fields, and (in a sealed
    // group, where the page shows nothing) fields left empty.
    let mut fixed = Vec::new();
    for field in &fields {
        let bool_field = matches!(field.kind(), FieldKind::Checkbox | FieldKind::Toggle);
        let keep = field.is_readonly_on_edit()
            || (S::ENCRYPTED && !bool_field && !sent.contains(field.name()));
        if keep {
            fixed.push((
                field.name().to_owned(),
                scalar_text(current.get(field.name())),
            ));
        }
    }
    let form: S = match read_form(&state, req, &money_names(&fields), &fixed).await {
        Ok(form) => form,
        Err(answer) => return Ok(*answer),
    };
    state.save_settings(&form, Some(&user)).await?;
    let label = texts.label("settings", &cx.label);
    let toast = Toast::success(texts.get("saved", &[("label", &label)]));
    Ok(panel::done(&htmx, toast, &cx.url()))
}

/// A stored value as the text a form would have sent.
fn scalar_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// Reads the body so the page can tell which fields were filled in, then
/// puts it back for the form reader.
async fn buffered(
    state: &AppState,
    req: Request,
) -> std::result::Result<(Request, BTreeSet<String>), Box<Response>> {
    let kind = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (parts, body) = req.into_parts();
    let bytes = Bytes::from_request(Request::from_parts(parts.clone(), body), state)
        .await
        .map_err(|rejection| Box::new(rejection.into_response()))?;
    let mut sent = BTreeSet::new();
    if kind.starts_with("application/json") {
        if let Ok(Value::Object(object)) = renox::serde_json::from_slice::<Value>(&bytes) {
            for (name, value) in object {
                if !matches!(&value, Value::Null) && value.as_str() != Some("") {
                    sent.insert(name);
                }
            }
        }
    } else {
        for (name, value) in parse_pairs(&bytes) {
            if !value.is_empty() {
                sent.insert(name);
            }
        }
    }
    Ok((Request::from_parts(parts, Body::from(bytes)), sent))
}
