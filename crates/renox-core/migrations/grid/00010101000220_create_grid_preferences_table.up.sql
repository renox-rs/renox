-- What each user chose for a data grid (renox::grid): visible columns per
-- screen size, their order and the frozen ones, as JSON.
CREATE TABLE grid_preferences (
    user_id INTEGER NOT NULL,
    grid TEXT NOT NULL,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, grid)
);
