//! `GraphStore` trait + SQLite implementation.
//!
//! The trait surface is intentionally narrow: lookup by id, insert by
//! value, list by repo-side. Slice 1 ships only `SqliteGraphStore`;
//! later layers may add an in-memory store for property tests, but the
//! production path stays SQLite (ADR-006).
//!
//! # Concurrency
//!
//! `SqliteGraphStore` wraps a [`std::sync::Mutex`] around the
//! connection so the type is `Send + Sync`. Per ADR-007 we do not need
//! parallel writers; the engine fan-out is per-observation and the
//! reducer is a single task. Read parallelism can be added later by
//! migrating to a connection pool — the `GraphStore` trait does not
//! commit to either choice.
//!
//! # Migrations
//!
//! Migrations are embedded with `include_str!` and applied inside a
//! transaction guarded by `schema_version`. Re-running on an
//! up-to-date database is a no-op (idempotent), per the Layer 1
//! acceptance criterion.

use crate::error::{GraphStoreError, Result};
use crate::model::{Artifact, BuildEdge, Correspondence, Entity, RepoSide};
use polyref_core::{
    correspondence_kind::CorrespondenceKind,
    ids::{ArtifactId, CorrId, EdgeId, EntityId},
    Observation,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::path::Path;
use std::sync::Mutex;

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");

/// Latest known schema version (1-indexed).
pub const LATEST_SCHEMA_VERSION: i64 = 1;

/// Persistent storage for the typed correspondence graph.
///
/// All operations may fail with [`GraphStoreError`]. IDs are validated
/// `polyref-core` newtypes — implementations never parse strings to
/// reconstruct ids; they round-trip the `as_str()` view.
pub trait GraphStore: Send + Sync {
    /// Apply pending migrations. Idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`GraphStoreError::Migration`] if any DDL statement
    /// fails, or [`GraphStoreError::UnsupportedSchemaVersion`] if the
    /// on-disk version is newer than [`LATEST_SCHEMA_VERSION`].
    fn migrate(&self) -> Result<()>;

    /// Save an artifact. Replaces any prior row with the same id.
    fn save_artifact(&self, artifact: &Artifact) -> Result<()>;

    /// Load an artifact by id.
    fn find_artifact(&self, id: &ArtifactId) -> Result<Option<Artifact>>;

    /// Save an entity. Replaces any prior row with the same id.
    fn save_entity(&self, entity: &Entity) -> Result<()>;

    /// Load an entity by id.
    fn find_entity(&self, id: &EntityId) -> Result<Option<Entity>>;

    /// Save a correspondence and all of its endpoints in one
    /// transaction.
    fn save_correspondence(&self, corr: &Correspondence) -> Result<()>;

    /// Load a correspondence by id, including its ordered endpoints.
    fn find_correspondence(&self, id: &CorrId) -> Result<Option<Correspondence>>;

    /// Save a build edge.
    fn save_build_edge(&self, edge: &BuildEdge) -> Result<()>;

    /// Load a build edge by id.
    fn find_build_edge(&self, id: &EdgeId) -> Result<Option<BuildEdge>>;

    /// Save an observation. The payload is canonical-JSON-serialized.
    fn save_observation(&self, observation_id: &str, observation: &Observation) -> Result<()>;

    /// Load an observation by id.
    fn find_observation(&self, observation_id: &str) -> Result<Option<Observation>>;

    /// Count entities for the given repo side.
    fn count_entities(&self, repo_side: RepoSide) -> Result<u64>;

    /// Count correspondences across the whole store.
    fn count_correspondences(&self) -> Result<u64>;
}

/// SQLite-backed implementation of [`GraphStore`].
///
/// Created via [`SqliteGraphStore::open`] (file path) or
/// [`SqliteGraphStore::open_in_memory`] (test-only).
pub struct SqliteGraphStore {
    conn: Mutex<Connection>,
}

impl SqliteGraphStore {
    /// Open or create a SQLite database at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`GraphStoreError::Sqlite`] if the file cannot be
    /// opened, the journal-mode pragma fails, or foreign-key support
    /// cannot be enabled.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory SQLite database (for tests).
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`].
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn configure(conn: &Connection) -> Result<()> {
        // Foreign keys must be enabled per connection.
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        // WAL improves read concurrency and is safer on crash.
        // It is a noop on `:memory:` so the test path tolerates it.
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        Ok(())
    }

    fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T>,
    {
        let mut guard = self.conn.lock().map_err(|_| poisoned_lock_error())?;
        f(&mut guard)
    }

    fn with_tx<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T>,
    {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let out = f(&tx)?;
            tx.commit()?;
            Ok(out)
        })
    }

    fn current_schema_version(conn: &Connection) -> Result<i64> {
        let table_exists: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if table_exists.is_none() {
            return Ok(0);
        }
        let v: Option<i64> = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        Ok(v.unwrap_or(0))
    }
}

fn poisoned_lock_error() -> GraphStoreError {
    GraphStoreError::Sqlite(rusqlite::Error::InvalidQuery)
}

impl GraphStore for SqliteGraphStore {
    fn migrate(&self) -> Result<()> {
        self.with_conn(|conn| {
            let current = Self::current_schema_version(conn)?;
            if current > LATEST_SCHEMA_VERSION {
                return Err(GraphStoreError::UnsupportedSchemaVersion {
                    found: current,
                    supported: LATEST_SCHEMA_VERSION,
                });
            }
            if current >= LATEST_SCHEMA_VERSION {
                return Ok(());
            }
            let tx = conn.transaction()?;
            // Apply 0001
            if current < 1 {
                tx.execute_batch(MIGRATION_0001)
                    .map_err(|e| GraphStoreError::Migration {
                        version: 1,
                        source: e,
                    })?;
                tx.execute(
                    "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                    params![1_i64],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    fn save_artifact(&self, artifact: &Artifact) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO artifact (artifact_id, repo_side, kind, language, local_path, content_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(artifact_id) DO UPDATE SET \
                    repo_side = excluded.repo_side, \
                    kind = excluded.kind, \
                    language = excluded.language, \
                    local_path = excluded.local_path, \
                    content_hash = excluded.content_hash",
                params![
                    artifact.artifact_id.as_str(),
                    artifact.repo_side.as_str(),
                    artifact_kind_tag(artifact.kind),
                    language_tag(artifact.language),
                    &artifact.local_path,
                    &artifact.content_hash,
                ],
            )?;
            Ok(())
        })
    }

    fn find_artifact(&self, id: &ArtifactId) -> Result<Option<Artifact>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT artifact_id, repo_side, kind, language, local_path, content_hash \
                 FROM artifact WHERE artifact_id = ?1",
            )?;
            let row = stmt
                .query_row(params![id.as_str()], |row| {
                    let id_str: String = row.get(0)?;
                    let side_str: String = row.get(1)?;
                    let kind_str: String = row.get(2)?;
                    let lang_str: String = row.get(3)?;
                    let local_path: String = row.get(4)?;
                    let content_hash: String = row.get(5)?;
                    Ok(RawArtifactRow {
                        id: id_str,
                        repo_side: side_str,
                        kind: kind_str,
                        language: lang_str,
                        local_path,
                        content_hash,
                    })
                })
                .optional()?;
            row.map(decode_artifact).transpose()
        })
    }

    fn save_entity(&self, entity: &Entity) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO entity (entity_id, artifact_id, repo_side, language, kind, local_path, stable_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(entity_id) DO UPDATE SET \
                    artifact_id = excluded.artifact_id, \
                    repo_side = excluded.repo_side, \
                    language = excluded.language, \
                    kind = excluded.kind, \
                    local_path = excluded.local_path, \
                    stable_hash = excluded.stable_hash",
                params![
                    entity.entity_id.as_str(),
                    entity.artifact_id.as_str(),
                    entity.repo_side.as_str(),
                    language_tag(entity.language),
                    &entity.kind,
                    &entity.local_path,
                    &entity.stable_hash,
                ],
            )?;
            Ok(())
        })
    }

    fn find_entity(&self, id: &EntityId) -> Result<Option<Entity>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT entity_id, artifact_id, repo_side, language, kind, local_path, stable_hash \
                 FROM entity WHERE entity_id = ?1",
            )?;
            let row = stmt
                .query_row(params![id.as_str()], |row| {
                    Ok(RawEntityRow {
                        id: row.get(0)?,
                        artifact_id: row.get(1)?,
                        repo_side: row.get(2)?,
                        language: row.get(3)?,
                        kind: row.get(4)?,
                        local_path: row.get(5)?,
                        stable_hash: row.get(6)?,
                    })
                })
                .optional()?;
            row.map(decode_entity).transpose()
        })
    }

    fn save_correspondence(&self, corr: &Correspondence) -> Result<()> {
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO correspondence (corr_id, kind, rule_version) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(corr_id) DO UPDATE SET \
                    kind = excluded.kind, \
                    rule_version = excluded.rule_version",
                params![
                    corr.corr_id.as_str(),
                    correspondence_kind_tag(corr.kind),
                    corr.rule_version.as_deref(),
                ],
            )?;
            // Replace endpoints atomically: delete existing, insert new.
            tx.execute(
                "DELETE FROM correspondence_endpoint WHERE corr_id = ?1",
                params![corr.corr_id.as_str()],
            )?;
            for (position, entity_id) in corr.endpoints.iter().enumerate() {
                let pos_i64: i64 = i64::try_from(position).unwrap_or(i64::MAX);
                tx.execute(
                    "INSERT INTO correspondence_endpoint (corr_id, position, entity_id) \
                     VALUES (?1, ?2, ?3)",
                    params![corr.corr_id.as_str(), pos_i64, entity_id.as_str()],
                )?;
            }
            Ok(())
        })
    }

    fn find_correspondence(&self, id: &CorrId) -> Result<Option<Correspondence>> {
        self.with_conn(|conn| {
            let header: Option<RawCorrHeader> = conn
                .query_row(
                    "SELECT corr_id, kind, rule_version FROM correspondence WHERE corr_id = ?1",
                    params![id.as_str()],
                    |row| {
                        Ok(RawCorrHeader {
                            id: row.get(0)?,
                            kind: row.get(1)?,
                            rule_version: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            let Some(header) = header else {
                return Ok(None);
            };
            let mut stmt = conn.prepare(
                "SELECT entity_id FROM correspondence_endpoint \
                 WHERE corr_id = ?1 ORDER BY position ASC",
            )?;
            let endpoints: std::result::Result<Vec<String>, rusqlite::Error> = stmt
                .query_map(params![id.as_str()], |row| row.get::<_, String>(0))?
                .collect();
            let endpoints = endpoints?;
            let mut parsed_endpoints = Vec::with_capacity(endpoints.len());
            for raw in endpoints {
                parsed_endpoints.push(parse_entity_id(&raw)?);
            }
            Ok(Some(Correspondence {
                corr_id: parse_corr_id(&header.id)?,
                kind: parse_correspondence_kind(&header.kind)?,
                rule_version: header.rule_version,
                endpoints: parsed_endpoints,
            }))
        })
    }

    fn save_build_edge(&self, edge: &BuildEdge) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO build_edge (edge_id, src_artifact, dst_artifact) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(edge_id) DO UPDATE SET \
                    src_artifact = excluded.src_artifact, \
                    dst_artifact = excluded.dst_artifact",
                params![
                    edge.edge_id.as_str(),
                    edge.src_artifact.as_str(),
                    edge.dst_artifact.as_str(),
                ],
            )?;
            Ok(())
        })
    }

    fn find_build_edge(&self, id: &EdgeId) -> Result<Option<BuildEdge>> {
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT edge_id, src_artifact, dst_artifact FROM build_edge WHERE edge_id = ?1",
                    params![id.as_str()],
                    |row| {
                        Ok(RawEdgeRow {
                            id: row.get(0)?,
                            src: row.get(1)?,
                            dst: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            row.map(decode_build_edge).transpose()
        })
    }

    fn save_observation(&self, observation_id: &str, observation: &Observation) -> Result<()> {
        let payload = serde_json::to_string(observation)?;
        let (kind, visibility) = observation_tags(observation);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO observation (observation_id, obs_kind, visibility, payload) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(observation_id) DO UPDATE SET \
                    obs_kind = excluded.obs_kind, \
                    visibility = excluded.visibility, \
                    payload = excluded.payload",
                params![observation_id, kind, visibility, &payload],
            )?;
            Ok(())
        })
    }

    fn find_observation(&self, observation_id: &str) -> Result<Option<Observation>> {
        let payload: Option<String> = self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT payload FROM observation WHERE observation_id = ?1",
                    params![observation_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?)
        })?;
        match payload {
            None => Ok(None),
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
        }
    }

    fn count_entities(&self, repo_side: RepoSide) -> Result<u64> {
        self.with_conn(|conn| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM entity WHERE repo_side = ?1",
                params![repo_side.as_str()],
                |row| row.get(0),
            )?;
            Ok(n.max(0) as u64)
        })
    }

    fn count_correspondences(&self) -> Result<u64> {
        self.with_conn(|conn| {
            let n: i64 =
                conn.query_row("SELECT COUNT(*) FROM correspondence", [], |row| row.get(0))?;
            Ok(n.max(0) as u64)
        })
    }
}

// ─── Tag tables ────────────────────────────────────────────────────────
//
// We round-trip enums through their JSON tag strings so the SQL layer
// matches the schemas (single source of truth). Adding a new variant
// touches both this table and the schema CHANGELOG.

fn artifact_kind_tag(kind: polyref_core::artifact_kind::ArtifactKind) -> &'static str {
    use polyref_core::artifact_kind::ArtifactKind as A;
    match kind {
        A::BuildFile => "build_file",
        A::Config => "config",
        A::Dockerfile => "dockerfile",
        A::Generated => "generated",
        A::Query => "query",
        A::Schema => "schema",
        A::SourceFile => "source_file",
        A::Test => "test",
        A::Workflow => "workflow",
        // `ArtifactKind` is `#[non_exhaustive]` from `polyref-core`. A
        // future variant must extend this match before the GraphStore
        // can persist it. We refuse to silently drop unknown kinds.
        other => unreachable_artifact_kind(other),
    }
}

#[cold]
fn unreachable_artifact_kind(_: polyref_core::artifact_kind::ArtifactKind) -> &'static str {
    // Intentional: any new ArtifactKind variant must be wired here in
    // the same PR that adds it to `polyref-core` and the schema.
    // Returning a sentinel keeps fail-closed semantics: the row will
    // round-trip a stable but obviously wrong tag, and the
    // `parse_artifact_kind` decoder will reject it with
    // `UnsupportedEnum`. Tests + drift check catch this.
    "__unsupported__"
}

fn parse_artifact_kind(s: &str) -> Result<polyref_core::artifact_kind::ArtifactKind> {
    use polyref_core::artifact_kind::ArtifactKind as A;
    match s {
        "build_file" => Ok(A::BuildFile),
        "config" => Ok(A::Config),
        "dockerfile" => Ok(A::Dockerfile),
        "generated" => Ok(A::Generated),
        "query" => Ok(A::Query),
        "schema" => Ok(A::Schema),
        "source_file" => Ok(A::SourceFile),
        "test" => Ok(A::Test),
        "workflow" => Ok(A::Workflow),
        other => Err(GraphStoreError::UnsupportedEnum {
            enum_name: "ArtifactKind",
            value: other.to_string(),
        }),
    }
}

fn language_tag(lang: polyref_core::language::Language) -> &'static str {
    use polyref_core::language::Language as L;
    match lang {
        L::Build => "build",
        L::Dockerfile => "dockerfile",
        L::Java => "java",
        L::Json => "json",
        L::Jsonschema => "jsonschema",
        L::Openapi => "openapi",
        L::Py => "py",
        L::Sql => "sql",
        L::Ts => "ts",
        L::Yaml => "yaml",
        other => unreachable_language(other),
    }
}

#[cold]
fn unreachable_language(_: polyref_core::language::Language) -> &'static str {
    "__unsupported__"
}

fn parse_language(s: &str) -> Result<polyref_core::language::Language> {
    use polyref_core::language::Language as L;
    match s {
        "build" => Ok(L::Build),
        "dockerfile" => Ok(L::Dockerfile),
        "java" => Ok(L::Java),
        "json" => Ok(L::Json),
        "jsonschema" => Ok(L::Jsonschema),
        "openapi" => Ok(L::Openapi),
        "py" => Ok(L::Py),
        "sql" => Ok(L::Sql),
        "ts" => Ok(L::Ts),
        "yaml" => Ok(L::Yaml),
        other => Err(GraphStoreError::UnsupportedEnum {
            enum_name: "Language",
            value: other.to_string(),
        }),
    }
}

fn correspondence_kind_tag(kind: CorrespondenceKind) -> &'static str {
    use CorrespondenceKind as K;
    match kind {
        K::BuildCodegen => "build_codegen",
        K::Call => "call",
        K::Configuration => "configuration",
        K::Event => "event",
        K::GeneratedClient => "generated_client",
        K::QueryTable => "query_table",
        K::Route => "route",
        K::Schema => "schema",
        K::Serialization => "serialization",
        K::TestOracle => "test_oracle",
        K::Workflow => "workflow",
        other => unreachable_correspondence_kind(other),
    }
}

#[cold]
fn unreachable_correspondence_kind(_: CorrespondenceKind) -> &'static str {
    "__unsupported__"
}

fn parse_correspondence_kind(s: &str) -> Result<CorrespondenceKind> {
    use CorrespondenceKind as K;
    match s {
        "build_codegen" => Ok(K::BuildCodegen),
        "call" => Ok(K::Call),
        "configuration" => Ok(K::Configuration),
        "event" => Ok(K::Event),
        "generated_client" => Ok(K::GeneratedClient),
        "query_table" => Ok(K::QueryTable),
        "route" => Ok(K::Route),
        "schema" => Ok(K::Schema),
        "serialization" => Ok(K::Serialization),
        "test_oracle" => Ok(K::TestOracle),
        "workflow" => Ok(K::Workflow),
        other => Err(GraphStoreError::UnsupportedEnum {
            enum_name: "CorrespondenceKind",
            value: other.to_string(),
        }),
    }
}

fn parse_repo_side(s: &str) -> Result<RepoSide> {
    match s {
        "old" => Ok(RepoSide::Old),
        "new" => Ok(RepoSide::New),
        other => Err(GraphStoreError::UnsupportedEnum {
            enum_name: "RepoSide",
            value: other.to_string(),
        }),
    }
}

fn observation_tags(obs: &Observation) -> (&'static str, &'static str) {
    use polyref_core::observation::Visibility;
    let kind = match obs {
        Observation::ApiCall(_) => "api_call",
        Observation::TestInvocation(_) => "test_invocation",
        Observation::BuildTarget(_) => "build_target",
        Observation::WorkflowRun(_) => "workflow_run",
        Observation::SchemaValidation(_) => "schema_validation",
        // `Observation` is `#[non_exhaustive]`; a new kind must extend
        // this match (and the schema enum). Sentinel keeps the column
        // queryable; the decode path would reject it.
        _ => "__unsupported__",
    };
    let visibility = match observation_header(obs).visibility {
        Visibility::Visible => "visible",
        Visibility::HeldOut => "held_out",
        Visibility::EvaluationOnly => "evaluation_only",
        // `Visibility` is `#[non_exhaustive]`. Same convention.
        _ => "__unsupported__",
    };
    (kind, visibility)
}

fn observation_header(obs: &Observation) -> &polyref_core::observation::ObsHeader {
    match obs {
        Observation::ApiCall(o) => &o.header,
        Observation::TestInvocation(o) => &o.header,
        Observation::BuildTarget(o) => &o.header,
        Observation::WorkflowRun(o) => &o.header,
        Observation::SchemaValidation(o) => &o.header,
        // Fall back on a static empty header for unknown observation
        // kinds. Save will store the sentinel tag and decode will
        // reject; we never panic.
        _ => fallback_obs_header(),
    }
}

fn fallback_obs_header() -> &'static polyref_core::observation::ObsHeader {
    use polyref_core::observation::{ObsHeader, Visibility};
    use std::sync::OnceLock;
    static FALLBACK: OnceLock<ObsHeader> = OnceLock::new();
    FALLBACK.get_or_init(|| ObsHeader {
        visibility: Visibility::HeldOut,
        support: Vec::new(),
        defined_semantics: false,
    })
}

// ─── Decode helpers ────────────────────────────────────────────────────

struct RawArtifactRow {
    id: String,
    repo_side: String,
    kind: String,
    language: String,
    local_path: String,
    content_hash: String,
}

struct RawEntityRow {
    id: String,
    artifact_id: String,
    repo_side: String,
    language: String,
    kind: String,
    local_path: String,
    stable_hash: String,
}

struct RawCorrHeader {
    id: String,
    kind: String,
    rule_version: Option<String>,
}

struct RawEdgeRow {
    id: String,
    src: String,
    dst: String,
}

fn decode_artifact(row: RawArtifactRow) -> Result<Artifact> {
    Ok(Artifact {
        artifact_id: parse_artifact_id(&row.id)?,
        repo_side: parse_repo_side(&row.repo_side)?,
        kind: parse_artifact_kind(&row.kind)?,
        language: parse_language(&row.language)?,
        local_path: row.local_path,
        content_hash: row.content_hash,
    })
}

fn decode_entity(row: RawEntityRow) -> Result<Entity> {
    Ok(Entity {
        entity_id: parse_entity_id(&row.id)?,
        artifact_id: parse_artifact_id(&row.artifact_id)?,
        repo_side: parse_repo_side(&row.repo_side)?,
        language: parse_language(&row.language)?,
        kind: row.kind,
        local_path: row.local_path,
        stable_hash: row.stable_hash,
    })
}

fn decode_build_edge(row: RawEdgeRow) -> Result<BuildEdge> {
    Ok(BuildEdge {
        edge_id: parse_edge_id(&row.id)?,
        src_artifact: parse_artifact_id(&row.src)?,
        dst_artifact: parse_artifact_id(&row.dst)?,
    })
}

fn parse_entity_id(s: &str) -> Result<EntityId> {
    EntityId::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "EntityId",
        value: format!("{s}: {e}"),
    })
}

fn parse_artifact_id(s: &str) -> Result<ArtifactId> {
    ArtifactId::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "ArtifactId",
        value: format!("{s}: {e}"),
    })
}

fn parse_corr_id(s: &str) -> Result<CorrId> {
    CorrId::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "CorrId",
        value: format!("{s}: {e}"),
    })
}

fn parse_edge_id(s: &str) -> Result<EdgeId> {
    EdgeId::parse(s).map_err(|e| GraphStoreError::UnsupportedEnum {
        enum_name: "EdgeId",
        value: format!("{s}: {e}"),
    })
}
