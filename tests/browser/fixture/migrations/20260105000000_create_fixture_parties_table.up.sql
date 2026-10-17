-- The parties with custom fields (parties.rs).
CREATE TABLE fixture_parties (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    extra TEXT NOT NULL DEFAULT '{}'
);
