-- Ortak Milestone 4: company-scoped run list index.
--
-- The Activity run list (crates/ortak-observability) reads runs newest
-- first with a keyset predicate on (queued_at, id) under a company scope.
-- Migration 0045 indexes runs by (company_id, employee_id, status,
-- queued_at) and the active subset by updated_at; neither serves an
-- unfiltered, ordered company list without a sort. This index does, and
-- its leading company_id keeps the scan inside one tenant. Additive only.

CREATE INDEX idx_runs_company_queued
    ON runs (company_id, queued_at DESC, id DESC);
