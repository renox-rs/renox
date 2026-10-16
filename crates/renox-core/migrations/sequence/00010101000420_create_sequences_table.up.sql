CREATE TABLE sequences (
    series TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT '',
    period TEXT NOT NULL DEFAULT '',
    last INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (series, scope, period)
);
