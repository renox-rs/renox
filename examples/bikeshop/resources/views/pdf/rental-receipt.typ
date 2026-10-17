// A rental's receipt (#30): the kit's header and table, laid out with start
// and end so the same file reads right in English and in Spanish. Every text
// arrives translated in `data.labels`; amounts are integers in cents.
#import "@renox/kit:0.1.0": *
#show: base

#page-header(
  data.labels.title,
  subtitle: data.store,
  meta: (
    (data.labels.code, data.code),
    (data.labels.returned, date(data.returned)),
  ),
)

#grid(
  columns: (auto, 1fr),
  column-gutter: spacing.md,
  row-gutter: spacing.sm,
  text(fill: colors.muted)[#data.labels.customer], data.customer,
  text(fill: colors.muted)[#data.labels.bike], data.bike,
)

#v(spacing.md)

#data-table(
  (data.labels.item, data.labels.amount),
  data.lines.map(line => (line.label, money(line.amount, currency: data.currency))),
  aligns: (start, end),
  footer: (data.labels.total, money(data.total, currency: data.currency)),
)

#v(spacing.md)

#data-table(
  (data.labels.deposit_title, data.labels.amount),
  data.deposit.map(line => (line.label, money(line.amount, currency: data.currency))),
  aligns: (start, end),
)

#if data.payments.len() > 0 [
  #v(spacing.md)
  #text(weight: "bold")[#data.labels.payments]
  #v(spacing.xs)
  #data-table(
    (data.labels.when, data.labels.method, data.labels.amount),
    data.payments.map(p => (p.when, p.method, money(p.amount, currency: data.currency))),
    aligns: (start, start, end),
  )
]

#v(spacing.lg)
#text(fill: colors.muted)[#data.labels.thanks]
