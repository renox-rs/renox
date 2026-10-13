-- A JSON array of what the token may do; NULL means everything.
ALTER TABLE personal_access_tokens ADD COLUMN abilities TEXT;
