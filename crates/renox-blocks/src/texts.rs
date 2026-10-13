//! The blocks' texts in English. An app translates any of them in its lang
//! files under the same key (`"blocks": {"gallery": {"next": "…"}}` in
//! `resources/lang/es.json`); `:name` placeholders are filled as the `t`
//! function fills them.

/// Every text the blocks show, `(key, English)`. The keys are the ones an
/// app's lang files use to translate them.
///
/// ```
/// let next = renox_blocks::TEXTS.iter().find(|(key, _)| *key == "blocks.gallery.next");
/// assert_eq!(next.map(|(_, text)| *text), Some("Next photo"));
/// ```
pub const TEXTS: &[(&str, &str)] = &[
    ("blocks.gallery.label", "Photos"),
    ("blocks.gallery.carousel", "carousel"),
    ("blocks.gallery.slide", "slide"),
    ("blocks.gallery.position", "Photo :n of :total"),
    (
        "blocks.gallery.keys",
        "Photo: the arrow keys show the next one",
    ),
    ("blocks.gallery.previous", "Previous photo"),
    ("blocks.gallery.next", "Next photo"),
    ("blocks.gallery.enlarge", "Enlarge"),
    ("blocks.gallery.thumbnails", "Choose a photo"),
    ("blocks.gallery.show", "Photo :n: :alt"),
    ("blocks.gallery.close", "Close"),
    ("blocks.range.label", "Range"),
    ("blocks.range.minimum", "Lowest :label"),
    ("blocks.range.maximum", "Highest :label"),
    ("blocks.quantity.label", "Quantity"),
    ("blocks.quantity.decrease", "One less (:label)"),
    ("blocks.quantity.increase", "One more (:label)"),
    ("blocks.keypad.label", "Number pad"),
    ("blocks.keypad.decimal", "Decimal point"),
    ("blocks.keypad.backspace", "Delete the last digit"),
    ("blocks.keypad.clear", "Clear"),
    ("blocks.keypad.enter", "Enter"),
    ("blocks.kanban.label", "Board"),
    (
        "blocks.kanban.help",
        "To move a card: Space or Enter picks it up, the arrow keys move it, Space or Enter drops it, Escape cancels.",
    ),
    (
        "blocks.kanban.picked",
        "Picked up :card, in :column, position :position of :total.",
    ),
    (
        "blocks.kanban.moved",
        ":card: :column, position :position of :total.",
    ),
    (
        "blocks.kanban.dropped",
        ":card dropped in :column, position :position of :total.",
    ),
    (
        "blocks.kanban.cancelled",
        "Move cancelled. :card is back in :column, position :position of :total.",
    ),
    (
        "blocks.kanban.failed",
        "The move of :card couldn't be saved. It is back in :column.",
    ),
    ("blocks.calendar.months", "Months"),
    ("blocks.calendar.previous", "Previous month"),
    ("blocks.calendar.next", "Next month"),
    ("blocks.calendar.today", "Today"),
    ("blocks.calendar.none", "Nothing booked this month."),
    ("blocks.calendar.month_1", "January"),
    ("blocks.calendar.month_2", "February"),
    ("blocks.calendar.month_3", "March"),
    ("blocks.calendar.month_4", "April"),
    ("blocks.calendar.month_5", "May"),
    ("blocks.calendar.month_6", "June"),
    ("blocks.calendar.month_7", "July"),
    ("blocks.calendar.month_8", "August"),
    ("blocks.calendar.month_9", "September"),
    ("blocks.calendar.month_10", "October"),
    ("blocks.calendar.month_11", "November"),
    ("blocks.calendar.month_12", "December"),
    ("blocks.calendar.day_0", "Sun"),
    ("blocks.calendar.day_1", "Mon"),
    ("blocks.calendar.day_2", "Tue"),
    ("blocks.calendar.day_3", "Wed"),
    ("blocks.calendar.day_4", "Thu"),
    ("blocks.calendar.day_5", "Fri"),
    ("blocks.calendar.day_6", "Sat"),
    ("blocks.calendar.day_long_0", "Sunday"),
    ("blocks.calendar.day_long_1", "Monday"),
    ("blocks.calendar.day_long_2", "Tuesday"),
    ("blocks.calendar.day_long_3", "Wednesday"),
    ("blocks.calendar.day_long_4", "Thursday"),
    ("blocks.calendar.day_long_5", "Friday"),
    ("blocks.calendar.day_long_6", "Saturday"),
    ("blocks.availability.label", "Availability"),
    ("blocks.availability.resource", "Resource"),
    ("blocks.availability.free", "Free"),
    ("blocks.availability.booked", "Booked"),
    ("blocks.availability.closed", "Closed"),
    ("blocks.availability.book", "Book :resource at :slot"),
    ("blocks.availability.legend", "What the slots mean"),
    ("blocks.datetime.start_date", "From (day)"),
    ("blocks.datetime.start_time", "From (time)"),
    ("blocks.datetime.end_date", "Until (day)"),
    ("blocks.datetime.end_time", "Until (time)"),
    ("blocks.datetime.hour", "1 hour"),
    ("blocks.datetime.hours", ":n hours"),
    ("blocks.datetime.day", "1 day"),
    ("blocks.datetime.days", ":n days"),
    ("blocks.datetime.minutes", ":n min"),
    (
        "blocks.datetime.order",
        "The end must come after the start.",
    ),
    ("blocks.swatches.sold_out", "sold out"),
    ("blocks.history.label", "History"),
    ("blocks.history.kind_info", "note"),
    ("blocks.history.kind_success", "done"),
    ("blocks.history.kind_warning", "needs attention"),
    ("blocks.history.kind_error", "problem"),
    ("blocks.plans.label", "Plans"),
    ("blocks.plans.popular", "Most popular"),
    ("blocks.plans.month", "month"),
    ("blocks.plans.year", "year"),
    ("blocks.plans.choose", "Choose this plan"),
    ("blocks.plans.compare", "Compare the plans"),
    ("blocks.plans.feature", "Feature"),
    ("blocks.plans.included", "Included"),
    ("blocks.plans.not_included", "Not included"),
];

/// The English text of `key`, if the blocks have one.
pub(crate) fn english(key: &str) -> Option<&'static str> {
    TEXTS.iter().find(|(k, _)| *k == key).map(|(_, text)| *text)
}
