# renox-blocks

Interactive blocks for [Renox](https://github.com/renox-rs/renox) apps: components the UI kit
doesn't carry, as template macros built on the kit's tokens. A page loads the blocks' script
only when it has a block, and then only the code of the blocks it has (no library, no build
step).

```rust
use renox::prelude::*;
use renox_blocks::Blocks;

App::new().module(Blocks::new())
```

```html
{% from "renox-blocks/blocks.html" import quantity, swatches, gallery %}
{{ gallery(product.photos, id="photos", label="Photos of " ~ product.name) }}
{{ swatches("size", "Frame size", sizes, selected="M", required=true) }}
{{ quantity("qty", 1, min=1, max=10) }}
```

The blocks:

- input: `quantity` (a stepper), `range_slider` (two handles), `keypad` (a point-of-sale
  number pad), `swatches` (variant chips), `datetime_range` (a start and an end, with times);
- showing data: `gallery` (a carousel), `history` (a timeline), `compare_plans` (pricing cards
  and a comparison table), `status_stepper` (a record's progress through its statuses);
- scheduling and work: `month_calendar`, `availability` (resources against hours),
  `week_calendar` (resources against a week; a free slot posts its resource and times with
  htmx), `kanban` (cards dragged or moved by keyboard, each move sent with htmx);
- printing: `receipt` (an 80 mm or 58 mm till receipt that prints alone, with a code slot and a
  Print button);
- maps: `map_picker` (a point, circle or polygon, by clicking or by typed coordinates) and `map`
  (markers and areas). They use Leaflet 1.9.4, vendored here with its licence, fetched only on a
  page with a map block; the tiles are yours to choose (`BLOCKS_MAP_TILES`,
  `BLOCKS_MAP_ATTRIBUTION`), there is no default tile service.

The input blocks send plain form fields, so `Valid<T>` reads them; every block works with the
keyboard, says its states in words, follows `prefers-reduced-motion`, works under
`CSP=strict`, and has English texts an app translates in its lang files
(`renox_blocks::TEXTS`). The guide is
[docs/blocks.md](https://github.com/renox-rs/renox/blob/main/docs/blocks.md);
examples/bikeshop uses every block. Versioned with `renox`: use the same version for both.
