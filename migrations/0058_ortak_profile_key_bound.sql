-- PostgreSQL bounds regex repetition counts to255. Keep the journal key's
-- 256-byte contract using an independent length check and unbounded character
-- class. Migration57 is already applied to disposable acceptance databases.
ALTER TABLE office_identity_profiles
    DROP CONSTRAINT office_identity_profiles_idempotency_key_check;
ALTER TABLE office_identity_profiles
    ADD CONSTRAINT office_identity_profiles_idempotency_key_check
    CHECK (length(idempotency_key) BETWEEN 1 AND 256 AND idempotency_key ~ '^[A-Za-z0-9:_.-]+$');
