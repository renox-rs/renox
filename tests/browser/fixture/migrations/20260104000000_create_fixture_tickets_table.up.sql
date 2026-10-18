-- The ticket that moves through a state machine (tickets.rs).
CREATE TABLE fixture_tickets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    status TEXT NOT NULL
);
