// The Renox kit, version 0.1.0: `#import "@renox/kit:0.1.0": *`.
//
// Design tokens, money and date formatting, a data table and a page header
// for the documents an app prints. Served by renox-pdf itself; no network.

// ---- Design tokens ---------------------------------------------------------

/// Colors. Plain constants; the helpers below use them.
#let colors = (
  brand: rgb("#2f5d8a"),
  ink: rgb("#1c2430"),
  muted: rgb("#667085"),
  line: rgb("#d5dae1"),
  surface: rgb("#f4f6f9"),
  positive: rgb("#1f7a4d"),
  negative: rgb("#b42318"),
)

/// Spacing steps.
#let spacing = (xs: 3pt, sm: 6pt, md: 12pt, lg: 24pt)

/// Type sizes.
#let sizes = (small: 8pt, body: 10pt, large: 14pt, title: 22pt)

/// Base look for a document: `#show: base`.
#let base(body) = {
  set text(size: sizes.body, fill: colors.ink)
  set par(leading: 0.6em)
  body
}

// ---- Money -----------------------------------------------------------------

// Symbol and minor-unit digits per currency: the same table as
// `renox::money::Currency` (a test compares them). One row per line.
#let _currencies = (
  "AED": ("AED", 2),
  "AUD": ("A$", 2),
  "BHD": ("BHD", 3),
  "BRL": ("R$", 2),
  "CAD": ("CA$", 2),
  "CHF": ("CHF", 2),
  "CLP": ("CLP", 0),
  "CNY": ("CN¥", 2),
  "COP": ("COP", 2),
  "CZK": ("CZK", 2),
  "DKK": ("DKK", 2),
  "EGP": ("EGP", 2),
  "EUR": ("€", 2),
  "GBP": ("£", 2),
  "HKD": ("HK$", 2),
  "HUF": ("HUF", 2),
  "IDR": ("Rp", 0),
  "ILS": ("₪", 2),
  "INR": ("₹", 2),
  "IQD": ("IQD", 3),
  "JOD": ("JOD", 3),
  "JPY": ("¥", 0),
  "KRW": ("₩", 0),
  "KWD": ("KWD", 3),
  "LYD": ("LYD", 3),
  "MAD": ("MAD", 2),
  "MXN": ("MX$", 2),
  "MYR": ("RM", 2),
  "NGN": ("₦", 2),
  "NOK": ("NOK", 2),
  "NZD": ("NZ$", 2),
  "OMR": ("OMR", 3),
  "PHP": ("₱", 2),
  "PKR": ("PKR", 2),
  "PLN": ("PLN", 2),
  "QAR": ("QAR", 2),
  "RON": ("RON", 2),
  "RUB": ("RUB", 2),
  "SAR": ("SAR", 2),
  "SEK": ("SEK", 2),
  "SGD": ("S$", 2),
  "THB": ("฿", 2),
  "TND": ("TND", 3),
  "TRY": ("₺", 2),
  "TWD": ("NT$", 2),
  "UGX": ("UGX", 0),
  "USD": ("$", 2),
  "VND": ("₫", 0),
  "ZAR": ("R", 2),
)

// Thousands and decimal separators of a language, as Renox's `number` filter.
#let _separators(lang) = {
  if lang in ("id", "de", "nl", "es", "it", "pt", "tr", "da", "nb", "ms", "vi") {
    (".", ",")
  } else if lang in ("fr", "ru", "pl", "cs", "sv", "fi", "uk") {
    ("\u{202f}", ",")
  } else {
    (",", ".")
  }
}

/// Formats an amount in minor units (cents) as a string: `format-money(1234567,
/// "USD", "en")` is `$12,345.67`. The decimals follow the currency (`JPY` 0,
/// `KWD` 3); an unknown code is shown as it is, with 2 decimals.
#let format-money(amount, currency, lang) = {
  assert(
    type(amount) == int,
    message: "money: the amount must be an integer in minor units, got " + repr(amount),
  )
  let code = upper(currency)
  let (symbol, digits) = _currencies.at(code, default: (code, 2))
  let (thousands, decimal) = _separators(lang)
  let size = calc.abs(amount)
  let scale = calc.pow(10, digits)
  let whole = str(calc.quo(size, scale))
  let grouped = ""
  for (i, digit) in whole.clusters().enumerate() {
    if i > 0 and calc.rem(whole.len() - i, 3) == 0 { grouped += thousands }
    grouped += digit
  }
  let fraction = if digits == 0 { "" } else {
    let f = str(calc.rem(size, scale))
    decimal + "0" * (digits - f.len()) + f
  }
  let space = if symbol.find(regex("[A-Za-z]$")) != none { " " } else { "" }
  (if amount < 0 { "-" } else { "" }) + symbol + space + grouped + fraction
}

/// An amount in minor units in the document's language: `#money(1234567)` is
/// `$12,345.67`, and `$12.345,67` after `#set text(lang: "es")`. The currency
/// is `"USD"` unless given.
#let money(amount, currency: "USD", lang: auto) = context {
  format-money(amount, currency, if lang == auto { text.lang } else { lang })
}

// ---- Dates -----------------------------------------------------------------

/// A `datetime` from a datetime, a `"2026-10-01"` date or an RFC 3339 text
/// (`"2026-10-01T09:30:00Z"`; the time part is ignored).
#let to-date(value) = {
  if type(value) == datetime { return value }
  assert(type(value) == str, message: "date: expected a date or text, got " + repr(value))
  let found = value.match(regex("^(\\d{4})-(\\d{2})-(\\d{2})"))
  assert(found != none, message: "date: `" + value + "` is not a date (use 2026-10-01)")
  let (year, month, day) = found.captures.map(int)
  datetime(year: year, month: month, day: day)
}

/// Shows a date, `2026-10-01` by default. `format` is Typst's datetime format
/// (`"[day]/[month]/[year]"`).
#let date(value, format: "[year]-[month]-[day]") = to-date(value).display(format)

// ---- Page header -----------------------------------------------------------

/// A header block: the title (and a subtitle) at the start, `meta` as
/// label/value pairs at the end, a brand rule below. `logo` is optional content,
/// for example `image("logo.png", height: 28pt)`.
#let page-header(title, subtitle: none, meta: (), logo: none) = {
  let left = stack(
    spacing: spacing.xs,
    text(size: sizes.title, weight: "bold", fill: colors.brand, title),
    if subtitle != none { text(fill: colors.muted, subtitle) },
  )
  let right = align(end, stack(
    spacing: spacing.xs,
    ..meta.map(pair => {
      text(fill: colors.muted, pair.at(0) + ": ")
      pair.at(1)
    }),
  ))
  grid(
    columns: if logo == none { (1fr, auto) } else { (auto, 1fr, auto) },
    column-gutter: spacing.md,
    ..if logo == none { (left, right) } else { (logo, left, right) },
  )
  v(spacing.sm)
  line(length: 100%, stroke: 1.5pt + colors.brand)
  v(spacing.md)
}

// ---- Table -----------------------------------------------------------------

/// A table with a header row and thin rules. `headers` is an array of cells,
/// `rows` an array of arrays; `aligns` gives each column's alignment (`start`
/// or `end`; numbers read best at the `end`), `widths` its width, and `footer`
/// is an optional array of cells (totals), set in bold.
#let data-table(headers, rows, aligns: auto, widths: auto, footer: none) = {
  let count = headers.len()
  for row in rows + (if footer == none { () } else { (footer,) }) {
    assert(
      row.len() == count,
      message: "data-table: a row has " + str(row.len()) + " cells, the header has " + str(count),
    )
  }
  table(
    columns: if widths == auto { (1fr,) * count } else { widths },
    align: if aligns == auto { (start,) * count } else { aligns },
    inset: (x: spacing.sm, y: spacing.sm),
    stroke: (x: none, y: 0.5pt + colors.line),
    fill: (_, y) => if y == 0 { colors.surface },
    table.header(..headers.map(h => text(weight: "bold", fill: colors.muted, h))),
    ..rows.flatten(),
    ..if footer == none { () } else { footer.map(c => text(weight: "bold", c)) },
  )
}
