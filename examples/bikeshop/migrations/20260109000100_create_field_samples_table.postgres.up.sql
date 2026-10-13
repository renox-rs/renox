-- /about/fields (src/app/about/fields.rs) on PostgreSQL: each column the
-- type its Rust field decodes from.
-- [explain:fields.postgres]
CREATE TABLE field_samples (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    stock BIGINT NOT NULL,
    weight_kg DOUBLE PRECISION NOT NULL,
    price BIGINT NOT NULL,
    available BOOLEAN NOT NULL,
    size TEXT NOT NULL,
    colors JSONB NOT NULL DEFAULT '[]'::jsonb,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    specs JSONB NOT NULL DEFAULT '[]'::jsonb,
    details TEXT,
    settings TEXT,
    brand TEXT,
    pickup_at TIME,
    launch_at TIMESTAMP,
    released_on DATE,
    photo TEXT,
    manual TEXT,
    manual_name TEXT,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ
);
-- [/explain:fields.postgres]
CREATE INDEX field_samples_user_id_index ON field_samples (user_id);
