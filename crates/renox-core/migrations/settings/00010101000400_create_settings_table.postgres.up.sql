-- One row per settings group and scope. `scope` is '' for the app's own
-- settings; an app that keeps a group per company or branch names it.
-- `value` is the group as JSON text, or sealed with APP_KEY for an
-- encrypted group. No foreign key on updated_by: the row outlives the user.
CREATE TABLE settings (
    key TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT '',
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    updated_by BIGINT,
    PRIMARY KEY (key, scope)
);
