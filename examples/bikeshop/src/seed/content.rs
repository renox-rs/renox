//! What the demo shop sells and where it is: categories, brands, model
//! names, places, people's names and service tasks. English content; the
//! prices are in US dollars (`APP_CURRENCY=USD` in `.env.example`), in
//! cents, the smallest unit, like all money in the app.

use crate::app::catalog::model::CategoryKind;

/// A category of the demo catalogue and how its products look.
pub struct CategorySpec {
    pub name: &'static str,
    pub slug: &'static str,
    pub kind: CategoryKind,
    /// Model names the products are made from.
    pub models: &'static [&'static str],
    /// Brands that make them (names from [`BRANDS`]).
    pub brands: &'static [&'static str],
    /// Sizes of the variants (`&[]`: one size).
    pub sizes: &'static [&'static str],
    /// Colours of the variants (`&[]`: one colour).
    pub colours: &'static [&'static str],
    /// Price range (cents).
    pub price: (i64, i64),
    /// Specification keys and their possible values.
    pub specs: &'static [(&'static str, &'static [&'static str])],
}

const FRAME: (&str, &[&str]) = ("Frame", &["Aluminium", "Carbon", "Steel", "Titanium"]);
const GEARS: (&str, &[&str]) = ("Gears", &["1 × 11", "1 × 12", "2 × 11", "2 × 12", "3 × 8"]);
const BRAKES: (&str, &[&str]) = ("Brakes", &["Hydraulic disc", "Mechanical disc", "Rim"]);
const WEIGHT: (&str, &[&str]) = (
    "Weight",
    &[
        "8.4 kg", "9.1 kg", "10.6 kg", "12.9 kg", "14.2 kg", "23.5 kg",
    ],
);

/// The catalogue's categories, from bikes to spare parts.
pub const CATEGORIES: &[CategorySpec] = &[
    CategorySpec {
        name: "Road bikes",
        slug: "road-bikes",
        kind: CategoryKind::Bike,
        models: &[
            "Domane", "Emonda", "Allez", "Tarmac", "TCR", "Synapse", "Orca", "Supersix",
        ],
        brands: &["Trek", "Specialized", "Giant", "Cannondale", "Orbea"],
        sizes: &["50 cm", "52 cm", "54 cm", "56 cm", "58 cm"],
        colours: &["Black", "Blue", "Red"],
        price: (120_000, 850_000),
        specs: &[FRAME, ("Wheels", &["700c"]), GEARS, BRAKES, WEIGHT],
    },
    CategorySpec {
        name: "Mountain bikes",
        slug: "mountain-bikes",
        kind: CategoryKind::Bike,
        models: &[
            "Marlin",
            "Roscoe",
            "Fuel",
            "Rockhopper",
            "Stumpjumper",
            "Talon",
            "Trail",
            "Oiz",
        ],
        brands: &["Trek", "Specialized", "Giant", "Cannondale", "Orbea"],
        sizes: &["S", "M", "L", "XL"],
        colours: &["Green", "Black", "Orange"],
        price: (80_000, 700_000),
        specs: &[
            FRAME,
            ("Wheels", &["27.5\"", "29\""]),
            GEARS,
            ("Suspension", &["Hardtail", "Full"]),
            WEIGHT,
        ],
    },
    CategorySpec {
        name: "City bikes",
        slug: "city-bikes",
        kind: CategoryKind::Bike,
        models: &[
            "FX", "Verve", "Escape", "Sirrus", "Quick", "Vector", "Carpe",
        ],
        brands: &["Trek", "Specialized", "Giant", "Cannondale", "Orbea"],
        sizes: &["S", "M", "L"],
        colours: &["Grey", "White", "Teal"],
        price: (50_000, 180_000),
        specs: &[
            FRAME,
            ("Wheels", &["700c"]),
            GEARS,
            BRAKES,
            ("Mudguards", &["Yes", "No"]),
        ],
    },
    CategorySpec {
        name: "Folding bikes",
        slug: "folding-bikes",
        kind: CategoryKind::Bike,
        models: &["C Line", "P Line", "A Line"],
        brands: &["Brompton"],
        sizes: &["One size"],
        colours: &["Racing Green", "Black", "Flame Lacquer"],
        price: (90_000, 320_000),
        specs: &[
            FRAME,
            ("Wheels", &["16\""]),
            ("Gears", &["2-speed", "4-speed", "6-speed"]),
            WEIGHT,
        ],
    },
    CategorySpec {
        name: "E-bikes",
        slug: "e-bikes",
        kind: CategoryKind::Bike,
        models: &[
            "Allant+",
            "Turbo Vado",
            "Explore E+",
            "Rail",
            "Kemen",
            "Tesoro Neo",
        ],
        brands: &["Trek", "Specialized", "Giant", "Cannondale", "Orbea"],
        sizes: &["S", "M", "L"],
        colours: &["Black", "Silver"],
        price: (250_000, 900_000),
        specs: &[
            FRAME,
            (
                "Motor",
                &["Bosch Performance", "Shimano EP8", "Specialized 2.0"],
            ),
            ("Battery", &["500 Wh", "625 Wh", "750 Wh"]),
            WEIGHT,
        ],
    },
    CategorySpec {
        name: "Kids' bikes",
        slug: "kids-bikes",
        kind: CategoryKind::Bike,
        models: &["Precaliber", "Jynx", "Hotrock", "ARX"],
        brands: &["Trek", "Specialized", "Giant", "Orbea"],
        sizes: &["16\"", "20\"", "24\""],
        colours: &["Purple", "Yellow", "Blue"],
        price: (25_000, 75_000),
        specs: &[FRAME, ("Gears", &["Single speed", "1 × 7"])],
    },
    CategorySpec {
        name: "Helmets",
        slug: "helmets",
        kind: CategoryKind::Gear,
        models: &["Aether", "Syntax", "Ventral", "Omne", "Register", "Fixture"],
        brands: &["Giro", "POC"],
        sizes: &["S", "M", "L"],
        colours: &["Matte Black", "White", "Hi-vis Yellow"],
        price: (4_500, 30_000),
        specs: &[
            ("MIPS", &["Yes", "No"]),
            ("Certification", &["CPSC", "EN 1078"]),
        ],
    },
    CategorySpec {
        name: "Lights",
        slug: "lights",
        kind: CategoryKind::Gear,
        models: &[
            "Lite Drive",
            "Micro Drive",
            "Plug",
            "Blinder",
            "Strip Drive",
        ],
        brands: &["Lezyne", "Knog"],
        sizes: &[],
        colours: &[],
        price: (2_500, 22_000),
        specs: &[
            ("Output", &["100 lm", "400 lm", "800 lm", "1200 lm"]),
            ("Charging", &["USB-C"]),
        ],
    },
    CategorySpec {
        name: "Locks",
        slug: "locks",
        kind: CategoryKind::Gear,
        models: &["Granit X-Plus", "Bordo", "New York", "Evolution", "Ugrip"],
        brands: &["Abus", "Kryptonite"],
        sizes: &[],
        colours: &[],
        price: (3_500, 16_000),
        specs: &[
            ("Security level", &["7/15", "10/15", "15/15"]),
            ("Kind", &["U-lock", "Folding", "Chain"]),
        ],
    },
    CategorySpec {
        name: "Clothing",
        slug: "clothing",
        kind: CategoryKind::Gear,
        models: &[
            "Commuter Jacket",
            "Bib Shorts",
            "Jersey",
            "Rain Trousers",
            "Gloves",
        ],
        brands: &["POC", "Giro"],
        sizes: &["XS", "S", "M", "L", "XL"],
        colours: &["Black", "Navy"],
        price: (3_000, 25_000),
        specs: &[("Material", &["Polyester", "Merino", "Nylon"])],
    },
    CategorySpec {
        name: "Bags",
        slug: "bags",
        kind: CategoryKind::Gear,
        models: &["Back-Roller", "Velocity", "Frame-Pack", "Seat-Pack"],
        brands: &["Ortlieb"],
        sizes: &[],
        colours: &["Black", "Signal Red"],
        price: (6_000, 22_000),
        specs: &[
            ("Volume", &["5 l", "11 l", "20 l", "40 l"]),
            ("Waterproof", &["Yes"]),
        ],
    },
    CategorySpec {
        name: "Chains",
        slug: "chains",
        kind: CategoryKind::Part,
        models: &["CN-M8100", "CN-HG701", "PC-1110", "GX Eagle"],
        brands: &["Shimano", "SRAM"],
        sizes: &[],
        colours: &[],
        price: (2_000, 9_000),
        specs: &[("Speeds", &["8", "11", "12"])],
    },
    CategorySpec {
        name: "Tyres",
        slug: "tyres",
        kind: CategoryKind::Part,
        models: &[
            "Grand Prix 5000",
            "Marathon Plus",
            "Gatorskin",
            "Nobby Nic",
            "Contact Urban",
        ],
        brands: &["Continental", "Schwalbe"],
        sizes: &["700 × 25c", "700 × 28c", "29 × 2.35", "16 × 1.35"],
        colours: &[],
        price: (3_000, 11_000),
        specs: &[("Puncture protection", &["Yes", "No"])],
    },
    CategorySpec {
        name: "Brakes",
        slug: "brakes",
        kind: CategoryKind::Part,
        models: &[
            "Brake pads B05S",
            "Disc rotor RT66",
            "Brake pads Code",
            "Rotor Centerline",
        ],
        brands: &["Shimano", "SRAM"],
        sizes: &[],
        colours: &[],
        price: (1_500, 18_000),
        specs: &[("Kind", &["Resin", "Metal", "Rotor"])],
    },
    CategorySpec {
        name: "Drivetrain",
        slug: "drivetrain",
        kind: CategoryKind::Part,
        models: &[
            "Cassette CS-M8100",
            "Derailleur RD-R7000",
            "Chainring X-Sync",
            "Shifter SL-M6100",
        ],
        brands: &["Shimano", "SRAM"],
        sizes: &[],
        colours: &[],
        price: (4_000, 35_000),
        specs: &[("Speeds", &["11", "12"])],
    },
    CategorySpec {
        name: "Saddles",
        slug: "saddles",
        kind: CategoryKind::Part,
        models: &["B17", "Cambium C17", "Power Expert", "Romin"],
        brands: &["Brooks", "Specialized"],
        sizes: &["143 mm", "155 mm"],
        colours: &[],
        price: (4_000, 20_000),
        specs: &[("Rails", &["Steel", "Titanium", "Carbon"])],
    },
];

/// Brands and their websites.
/// How many photos each category has in `public/images/products/`
/// (`{slug}-1.webp` … `{slug}-N.webp`, credited in `public/images/CREDITS.md`).
/// A product shows photo `(id % N) + 1` of its category, so neighbours differ;
/// migration `20260108000000_use_product_photos` points older databases at them.
pub const PHOTOS: &[(&str, i64)] = &[
    ("road-bikes", 5),
    ("mountain-bikes", 3),
    ("city-bikes", 5),
    ("folding-bikes", 4),
    ("e-bikes", 4),
    ("kids-bikes", 4),
    ("helmets", 5),
    ("lights", 2),
    ("locks", 2),
    ("clothing", 3),
    ("bags", 3),
    ("chains", 3),
    ("tyres", 3),
    ("brakes", 3),
    ("drivetrain", 4),
    ("saddles", 2),
];

/// A product's photo: one of its category's photos, or the category's drawing
/// when the category has none.
pub fn product_photo(category: &str, product_id: i64) -> String {
    match PHOTOS.iter().find(|(slug, _)| *slug == category) {
        Some((_, count)) => format!(
            "images/products/{category}-{}.webp",
            product_id.rem_euclid(*count) + 1
        ),
        None => format!("images/categories/{category}.svg"),
    }
}

pub const BRANDS: &[(&str, &str)] = &[
    ("Trek", "https://www.trekbikes.com"),
    ("Specialized", "https://www.specialized.com"),
    ("Giant", "https://www.giant-bicycles.com"),
    ("Cannondale", "https://www.cannondale.com"),
    ("Orbea", "https://www.orbea.com"),
    ("Brompton", "https://www.brompton.com"),
    ("Shimano", "https://bike.shimano.com"),
    ("SRAM", "https://www.sram.com"),
    ("Continental", "https://www.continental-tires.com"),
    ("Schwalbe", "https://www.schwalbe.com"),
    ("Abus", "https://www.abus.com"),
    ("Kryptonite", "https://www.kryptonitelock.com"),
    ("Giro", "https://www.giro.com"),
    ("POC", "https://www.pocsports.com"),
    ("Lezyne", "https://www.lezyne.com"),
    ("Knog", "https://www.knog.com"),
    ("Ortlieb", "https://www.ortlieb.com"),
    ("Brooks", "https://www.brooksengland.com"),
];

/// Countries and some of their cities (customers come from all of them:
/// tourists rent bikes too).
pub const PLACES: &[(&str, &str, &[&str])] = &[
    (
        "Indonesia",
        "ID",
        &[
            "Jakarta",
            "Bandung",
            "Surabaya",
            "Yogyakarta",
            "Denpasar",
            "Medan",
            "Semarang",
            "Malang",
        ],
    ),
    ("Singapore", "SG", &["Singapore"]),
    ("Malaysia", "MY", &["Kuala Lumpur", "Penang"]),
    ("Australia", "AU", &["Sydney", "Melbourne", "Perth"]),
    ("Netherlands", "NL", &["Amsterdam", "Utrecht"]),
    ("Germany", "DE", &["Berlin", "Munich"]),
    ("Spain", "ES", &["Madrid", "Barcelona", "Valencia"]),
    ("Japan", "JP", &["Tokyo", "Osaka"]),
    ("United States", "US", &["Portland", "Boulder"]),
    ("United Kingdom", "GB", &["London", "Bristol"]),
];

/// The three stores: name, slug, street, district, phone.
pub const STORES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "North",
        "north",
        "Jalan Gunung Sahari 18",
        "Kemayoran",
        "+62 21 555 0101",
    ),
    (
        "South",
        "south",
        "Jalan Fatmawati 42",
        "Cilandak",
        "+62 21 555 0202",
    ),
    (
        "West",
        "west",
        "Jalan Panjang 7",
        "Kebon Jeruk",
        "+62 21 555 0303",
    ),
];

/// Given names for demo people.
pub const FIRST_NAMES: &[&str] = &[
    "Ana", "Budi", "Citra", "Dewi", "Eko", "Fajar", "Gita", "Hana", "Indra", "Joko", "Kartika",
    "Lina", "Made", "Nadia", "Oscar", "Putri", "Rizky", "Sari", "Tomas", "Umar", "Vera", "Wayan",
    "Yusuf", "Zahra", "Emma", "Liam", "Sofia", "Noah", "Mia", "Lucas", "Chloe", "Kenji", "Aiko",
    "Marta", "Pablo", "Jonas", "Lea", "Oliver", "Grace", "Daniel",
];

/// Family names for demo people.
pub const LAST_NAMES: &[&str] = &[
    "Santoso", "Wijaya", "Pratama", "Hartono", "Kusuma", "Halim", "Saputra", "Nugroho", "Lestari",
    "Gunawan", "Tan", "Lim", "Smith", "Jones", "Garcia", "Muller", "de Vries", "Tanaka", "Sato",
    "Brown", "Wilson", "Lopez", "Fischer", "Bakker", "Ng", "Chen",
];

/// Streets for customers' addresses.
pub const STREETS: &[&str] = &[
    "Jalan Merdeka",
    "Jalan Sudirman",
    "Jalan Thamrin",
    "Jalan Kemang Raya",
    "High Street",
    "Market Street",
    "Canal Road",
    "Park Lane",
    "Station Road",
    "Calle Mayor",
    "Hauptstrasse",
];

/// Service tasks: name, slug, minutes, price (cents).
pub const SERVICE_TASKS: &[(&str, &str, i64, i64)] = &[
    ("Safety check", "safety-check", 20, 3_000),
    ("Tyre and tube change", "tyre-change", 20, 2_000),
    ("Brake adjustment", "brake-adjustment", 25, 2_500),
    ("Brake pads replacement", "brake-pads", 30, 3_500),
    ("Gear indexing", "gear-indexing", 25, 2_500),
    ("Chain clean and lube", "chain-clean", 20, 2_000),
    ("Chain replacement", "chain-replacement", 20, 2_500),
    ("Wheel truing", "wheel-truing", 40, 3_500),
    ("Hydraulic brake bleed", "brake-bleed", 45, 6_000),
    ("Suspension service", "suspension-service", 90, 15_000),
    ("E-bike diagnostics", "ebike-diagnostics", 40, 6_000),
    ("Full service", "full-service", 150, 15_000),
];

/// Service plans: name, slug, frequency, price per visit (cents), description, task slugs.
pub type PlanSpec = (
    &'static str,
    &'static str,
    &'static str,
    i64,
    &'static str,
    &'static [&'static str],
);

/// The service plans customers can subscribe a bike to.
pub const SERVICE_PLANS: &[PlanSpec] = &[
    (
        "Commuter check",
        "commuter-check",
        "weekly",
        800,
        "A quick look every week: tyres, brakes and chain, so the daily ride never stops.",
        &["safety-check", "chain-clean"],
    ),
    (
        "E-bike care",
        "ebike-care",
        "fortnightly",
        3_000,
        "Every two weeks: diagnostics, brakes and drivetrain for bikes with a motor.",
        &["ebike-diagnostics", "brake-adjustment", "chain-clean"],
    ),
    (
        "Monthly tune-up",
        "monthly-tune-up",
        "monthly",
        6_000,
        "Once a month: gears indexed, brakes adjusted, chain cleaned and the bike checked.",
        &[
            "safety-check",
            "gear-indexing",
            "brake-adjustment",
            "chain-clean",
        ],
    ),
    (
        "Quarterly full service",
        "quarterly-full-service",
        "quarterly",
        21_000,
        "Every three months the full service, with wheel truing and a brake bleed.",
        &["full-service", "wheel-truing", "brake-bleed"],
    ),
];

/// Suppliers: name, email, lead days.
pub const SUPPLIERS: &[(&str, &str, i64)] = &[
    (
        "Cycle Distribution Asia",
        "orders@cycle-distribution.example",
        14,
    ),
    ("Parts Direct", "sales@partsdirect.example", 5),
    ("Urban Gear Wholesale", "hello@urbangear.example", 7),
    ("E-Mobility Imports", "trade@emobility.example", 21),
];
