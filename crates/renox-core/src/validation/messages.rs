/// The message template for a rule. `:attribute` is the field's label,
/// `:Attribute` the same with a capital first letter, and `:min`, `:max`,
/// `:other` fill in rule arguments.
pub(crate) fn template(key: &str) -> &'static str {
    match key {
        "required" => "The :attribute field is required.",
        "min.string" => "The :attribute must be at least :min characters.",
        "min.numeric" => "The :attribute must be at least :min.",
        "min.array" => "The :attribute must have at least :min items.",
        "max.string" => "The :attribute may not be longer than :max characters.",
        "max.numeric" => "The :attribute may not be greater than :max.",
        "max.array" => "The :attribute may not have more than :max items.",
        "between.string" => "The :attribute must be between :min and :max characters.",
        "between.numeric" => "The :attribute must be between :min and :max.",
        "between.array" => "The :attribute must have between :min and :max items.",
        "min.file" => "The :attribute must be at least :min kilobytes.",
        "max.file" => "The :attribute may not be greater than :max kilobytes.",
        "between.file" => "The :attribute must be between :min and :max kilobytes.",
        "file" => "The :attribute must be a file.",
        "image" => "The :attribute must be an image.",
        "mimes" => "The :attribute must be a file of type: :values.",
        "email" => "The :attribute must be a valid email address.",
        "url" => "The :attribute must be a valid URL.",
        "in" => "The selected :attribute is invalid.",
        "confirmed" => "The :attribute confirmation does not match.",
        "password.letters" => "The :attribute must contain at least one letter.",
        "password.mixed" => {
            "The :attribute must contain at least one uppercase and one lowercase letter."
        }
        "password.numbers" => "The :attribute must contain at least one number.",
        "password.symbols" => "The :attribute must contain at least one symbol.",
        "password.uncompromised" => {
            "The :attribute has appeared in a data leak. Please choose a different one."
        }
        "current_password" => "The :attribute is incorrect.",
        "accepted" => "The :attribute must be accepted.",
        "unique" => "The :attribute has already been taken.",
        "exists" => "The selected :attribute is invalid.",
        "numeric" => "The :attribute must be a number.",
        "regex" => "The :attribute format is invalid.",
        "digits" => "The :attribute must be :digits digits.",
        "digits_between" => "The :attribute must be between :min and :max digits.",
        "date" => "The :attribute is not a valid date.",
        "before" => "The :attribute must be a date before :date.",
        "before_or_equal" => "The :attribute must be a date before or equal to :date.",
        "after" => "The :attribute must be a date after :date.",
        "after_or_equal" => "The :attribute must be a date after or equal to :date.",
        "not_in" => "The selected :attribute is invalid.",
        "same" => "The :attribute and :other must match.",
        "different" => "The :attribute and :other must be different.",
        "alpha" => "The :attribute may only contain letters.",
        "alpha_num" => "The :attribute may only contain letters and numbers.",
        "alpha_dash" => "The :attribute may only contain letters, numbers, dashes and underscores.",
        "lowercase" => "The :attribute must be lowercase.",
        "uppercase" => "The :attribute must be uppercase.",
        "starts_with" => "The :attribute must start with one of: :values.",
        "ends_with" => "The :attribute must end with one of: :values.",
        "uuid" => "The :attribute must be a valid UUID.",
        "ip" => "The :attribute must be a valid IP address.",
        "size.string" => "The :attribute must be :size characters.",
        "size.numeric" => "The :attribute must be :size.",
        "size.array" => "The :attribute must contain :size items.",
        "size.file" => "The :attribute must be :size kilobytes.",
        "prohibited" => "The :attribute field must be empty here.",
        "distinct" => "The :attribute is a duplicate.",
        "gt.numeric" => "The :attribute must be greater than :other.",
        "gt.string" => "The :attribute must be longer than :other.",
        "gt.array" => "The :attribute must have more items than :other.",
        "gt.file" => "The :attribute must be larger than :other.",
        "gt.date" => "The :attribute must be after :other.",
        "gte.numeric" => "The :attribute must be greater than or equal to :other.",
        "gte.string" => "The :attribute must be at least as long as :other.",
        "gte.array" => "The :attribute must have at least as many items as :other.",
        "gte.file" => "The :attribute must be at least as large as :other.",
        "gte.date" => "The :attribute must be the same as or after :other.",
        "lt.numeric" => "The :attribute must be less than :other.",
        "lt.string" => "The :attribute must be shorter than :other.",
        "lt.array" => "The :attribute must have fewer items than :other.",
        "lt.file" => "The :attribute must be smaller than :other.",
        "lt.date" => "The :attribute must be before :other.",
        "lte.numeric" => "The :attribute must be less than or equal to :other.",
        "lte.string" => "The :attribute must be at most as long as :other.",
        "lte.array" => "The :attribute must have at most as many items as :other.",
        "lte.file" => "The :attribute must be at most as large as :other.",
        "lte.date" => "The :attribute must be the same as or before :other.",
        "decimal" => "The :attribute must have :decimal decimal places.",
        "dimensions" => "The :attribute has invalid image dimensions.",
        "prohibits" => "The :attribute field can't be sent together with :other.",
        "min_digits" => "The :attribute must have at least :min digits.",
        "max_digits" => "The :attribute must not have more than :max digits.",
        "multiple_of" => "The :attribute must be a multiple of :value.",
        "integer" => "The :attribute must be a whole number.",
        "json" => "The :attribute must be valid JSON.",
        "ulid" => "The :attribute must be a valid ULID.",
        "timezone" => "The :attribute must be a valid time zone.",
        "mac_address" => "The :attribute must be a valid MAC address.",
        "ascii" => "The :attribute may only contain ASCII characters.",
        "hex_color" => "The :attribute must be a valid hexadecimal colour.",
        "doesnt_start_with" => "The :attribute may not start with one of: :values.",
        "doesnt_end_with" => "The :attribute may not end with one of: :values.",
        "not_regex" => "The :attribute format is invalid.",
        "declined" => "The :attribute must be declined.",
        "auth.failed" => "These credentials do not match our records.",
        "auth.throttle" => "Too many login attempts. Please try again in :seconds seconds.",
        _ => "The :attribute is invalid.",
    }
}

/// The app's translation of a built-in message (`renox.validation.{key}` in
/// its lang file), or the built-in template.
pub(crate) fn template_for(
    texts: Option<&crate::i18n::Texts>,
    key: &str,
) -> std::borrow::Cow<'static, str> {
    match texts.and_then(|t| t.get(&format!("renox.validation.{key}"))) {
        Some(text) => std::borrow::Cow::Owned(text.clone()),
        None => std::borrow::Cow::Borrowed(template(key)),
    }
}

/// Fills a template's placeholders.
pub(crate) fn render(template: &str, label: &str, params: &[(&str, String)]) -> String {
    let mut capitalized = label.to_owned();
    if let Some(first) = capitalized.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    let mut out = template
        .replace(":Attribute", &capitalized)
        .replace(":attribute", label);
    for (name, value) in params {
        out = out.replace(&format!(":{name}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_english_messages() {
        let params = [("min", "3".to_owned())];
        assert_eq!(
            render(template("min.string"), "name", &params),
            "The name must be at least 3 characters."
        );
        assert_eq!(
            render(":Attribute needs :min.", "name", &params),
            "Name needs 3."
        );
    }

    #[test]
    fn every_rule_has_its_own_message() {
        for key in [
            "regex",
            "digits",
            "digits_between",
            "date",
            "before",
            "before_or_equal",
            "after",
            "after_or_equal",
            "not_in",
            "same",
            "different",
            "alpha",
            "alpha_num",
            "alpha_dash",
            "lowercase",
            "uppercase",
            "starts_with",
            "ends_with",
            "uuid",
            "ip",
            "size.string",
            "size.numeric",
            "size.array",
            "size.file",
            "prohibited",
            "distinct",
            "gt.numeric",
            "gte.date",
            "lt.string",
            "lte.file",
            "decimal",
            "dimensions",
            "prohibits",
            "min_digits",
            "max_digits",
            "multiple_of",
            "integer",
            "json",
            "ulid",
            "timezone",
            "mac_address",
            "ascii",
            "hex_color",
            "doesnt_start_with",
            "doesnt_end_with",
            "not_regex",
            "declined",
            "password.uncompromised",
            "current_password",
        ] {
            assert_ne!(template(key), "The :attribute is invalid.", "{key}");
        }
    }
}
