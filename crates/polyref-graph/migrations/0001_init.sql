-- 0001_init.sql — initial GraphStore schema (Layer 1).
--
-- Tables mirror the paper Definition 1 repository tuple:
--   (A, N, L, C, Build, O, owner, type)
--
-- IDs are stored as TEXT (validated against ADR-003 grammar in code,
-- never inside SQL). Foreign keys are enforced; SQLite needs
-- `PRAGMA foreign_keys = ON;` per connection (set in connection setup).
--
-- All correspondence endpoints live in `correspondence_endpoint`
-- (one row per endpoint position). This avoids the Cartesian-product
-- blowup ADR-005 §2 warns against and lets ambiguity collapse lazily.

CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS artifact (
    artifact_id    TEXT PRIMARY KEY,
    repo_side      TEXT NOT NULL CHECK (repo_side IN ('old', 'new')),
    kind           TEXT NOT NULL,
    language       TEXT NOT NULL,
    local_path     TEXT NOT NULL,
    content_hash   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artifact_repo_side ON artifact(repo_side);
CREATE INDEX IF NOT EXISTS idx_artifact_content_hash ON artifact(content_hash);

CREATE TABLE IF NOT EXISTS entity (
    entity_id    TEXT PRIMARY KEY,
    artifact_id  TEXT NOT NULL,
    repo_side    TEXT NOT NULL CHECK (repo_side IN ('old', 'new')),
    language     TEXT NOT NULL,
    kind         TEXT NOT NULL,
    local_path   TEXT NOT NULL,
    stable_hash  TEXT NOT NULL,
    FOREIGN KEY (artifact_id) REFERENCES artifact(artifact_id)
);

CREATE INDEX IF NOT EXISTS idx_entity_artifact ON entity(artifact_id);
CREATE INDEX IF NOT EXISTS idx_entity_repo_kind ON entity(repo_side, kind);

CREATE TABLE IF NOT EXISTS correspondence (
    corr_id      TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    rule_version TEXT
);

CREATE INDEX IF NOT EXISTS idx_correspondence_kind ON correspondence(kind);

-- One row per endpoint position; ordering carries semantic meaning
-- (paper Def. 3 ordered endpoints: ends(c) = (n_1, ..., n_m)).
CREATE TABLE IF NOT EXISTS correspondence_endpoint (
    corr_id    TEXT NOT NULL,
    position   INTEGER NOT NULL,
    entity_id  TEXT NOT NULL,
    PRIMARY KEY (corr_id, position),
    FOREIGN KEY (corr_id)   REFERENCES correspondence(corr_id),
    FOREIGN KEY (entity_id) REFERENCES entity(entity_id)
);

CREATE INDEX IF NOT EXISTS idx_corr_endpoint_entity
    ON correspondence_endpoint(entity_id);
CREATE INDEX IF NOT EXISTS idx_corr_endpoint_kind_entity
    ON correspondence_endpoint(corr_id, entity_id);

CREATE TABLE IF NOT EXISTS build_edge (
    edge_id      TEXT PRIMARY KEY,
    src_artifact TEXT NOT NULL,
    dst_artifact TEXT NOT NULL,
    FOREIGN KEY (src_artifact) REFERENCES artifact(artifact_id),
    FOREIGN KEY (dst_artifact) REFERENCES artifact(artifact_id)
);

CREATE INDEX IF NOT EXISTS idx_build_edge_src ON build_edge(src_artifact);
CREATE INDEX IF NOT EXISTS idx_build_edge_dst ON build_edge(dst_artifact);

-- Observations are persisted as canonical JSON in `payload`. The kind
-- column is denormalized for indexed lookup; visibility is denormalized
-- to enforce ADR-008 / ADR-010 leakage prevention via SQL filters.
CREATE TABLE IF NOT EXISTS observation (
    observation_id  TEXT PRIMARY KEY,
    obs_kind        TEXT NOT NULL,
    visibility      TEXT NOT NULL CHECK (visibility IN ('visible','held_out','evaluation_only')),
    payload         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_observation_kind ON observation(obs_kind);
CREATE INDEX IF NOT EXISTS idx_observation_visibility ON observation(visibility);
