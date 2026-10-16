CREATE TABLE sequences (
    series TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT '',
    period TEXT NOT NULL DEFAULT '',
    last BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (series, scope, period)
);
