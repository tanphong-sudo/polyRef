//! Layer 3 plugin-host protocol helpers.
//!
//! This module owns ADR-002 one-line JSON-RPC framing and validation. It does
//! not spawn plugin processes; process supervision is layered on top so protocol
//! tests can stay deterministic and backend-neutral.

use crate::cgroup::{IsolationBackend, IsolationError, PluginIsolationProfile};
use crate::envelope::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::limits::Limits;
use polyref_core::correspondence_kind::CorrespondenceKind;
use polyref_core::status::UnknownReason;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// JSON-RPC methods supported by the PolyRef plugin SPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PluginMethod {
    /// Extract entities/facts from one artifact.
    Extract,
    /// Describe a kind-checker contract.
    Describe,
    /// Check one typed correspondence obligation.
    Check,
}

/// Non-null JSON-RPC request id used by the host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginRequestId(String);

/// Plugin executable identity used by host supervision and memo keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBinary {
    path: PathBuf,
    digest: String,
}

/// Launch configuration for one plugin process.
#[derive(Clone)]
pub struct PluginLaunchConfig {
    binary: PluginBinary,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    limits: Limits,
    stderr_cap_bytes: usize,
    isolation: PluginIsolationMode,
}

/// Process isolation mode for plugin launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginIsolationMode {
    /// Direct host execution, intended for deterministic unit tests only.
    DirectForTests,
    /// Run through an OS isolation backend before the plugin binary.
    Backend {
        /// Backend executable/profile family.
        backend: IsolationBackend,
        /// Resource/seccomp profile.
        profile: PluginIsolationProfile,
    },
}

/// Deterministic key for memoized plugin response bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginMemoKey(String);

/// In-memory plugin memo store for deterministic replay tests and adapters.
#[derive(Debug, Clone)]
pub struct PluginMemoStore {
    responses: BTreeMap<PluginMemoKey, Vec<u8>>,
    response_limit_bytes: usize,
}

/// Kind partition for bounded plugin pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PluginKind {
    /// Extractor plugin pool.
    Extractor,
    /// Kind-checker plugin pool for one correspondence kind.
    Checker(CorrespondenceKind),
}

/// Bounded plugin-pool configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPoolConfig {
    kind: PluginKind,
    max_processes: usize,
    queue_bound: usize,
}

/// Bounded plugin pool dispatcher.
#[derive(Clone)]
pub struct PluginPool {
    config: PluginPoolConfig,
    launch: PluginLaunchConfig,
    state: Arc<PoolState>,
}

struct PoolState {
    inner: Mutex<PoolInner>,
    available: Condvar,
}

impl fmt::Debug for PluginPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginPool")
            .field("config", &self.config)
            .field("launch", &self.launch)
            .finish_non_exhaustive()
    }
}

struct PoolInner {
    workers: Vec<PluginWorker>,
    live_total: usize,
    waiting: usize,
}

struct PluginWorker {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    stderr_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
}

/// Protocol-layer host errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginHostError {
    /// Payload exceeded the configured byte cap.
    #[error("plugin payload exceeds {limit} bytes: {actual}")]
    PayloadTooLarge {
        /// Configured byte limit.
        limit: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// JSON parse or serialization failed.
    #[error("plugin protocol json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Response was structurally invalid.
    #[error("malformed plugin response: {0}")]
    MalformedResponse(String),
    /// Response id did not match the request id.
    #[error("unexpected plugin response id: expected {expected}, actual {actual}")]
    UnexpectedId {
        /// Expected request id.
        expected: String,
        /// Actual response id.
        actual: String,
    },
    /// Method is not in the v1 SPI method set.
    #[error("unsupported plugin method: {0}")]
    UnsupportedMethod(String),
    /// Request id is empty or too large.
    #[error("invalid plugin request id: {0}")]
    InvalidRequestId(String),
    /// Plugin binary identity is invalid.
    #[error("invalid plugin binary: {0}")]
    InvalidPluginBinary(String),
    /// Plugin memo key is invalid.
    #[error("invalid plugin memo key: {0}")]
    InvalidMemoKey(String),
    /// Plugin pool configuration is invalid.
    #[error("invalid plugin pool config: {0}")]
    InvalidPoolConfig(String),
    /// Plugin isolation profile is invalid.
    #[error("invalid plugin isolation: {0}")]
    Isolation(#[from] IsolationError),
    /// Plugin pool queue is full.
    #[error("plugin pool backpressure for {kind:?}: active={active}, waiting={waiting}, queue_bound={queue_bound}")]
    Backpressure {
        /// Pool kind.
        kind: PluginKind,
        /// Active process calls.
        active: usize,
        /// Waiting calls.
        waiting: usize,
        /// Configured waiting-call bound.
        queue_bound: usize,
    },
    /// Plugin executable or process I/O failed.
    #[error("plugin process io error: {0}")]
    Io(#[from] std::io::Error),
    /// Plugin did not finish before its deadline.
    #[error("plugin timed out after {timeout_ms} ms")]
    Timeout {
        /// Timeout in milliseconds.
        timeout_ms: u128,
    },
    /// Plugin exited with a failing status.
    #[error("plugin exited non-zero: code={code:?}, stderr_bytes={stderr_bytes}")]
    NonZeroExit {
        /// Process exit code, if the platform reported one.
        code: Option<i32>,
        /// Number of captured stderr bytes, capped by config.
        stderr_bytes: usize,
    },
}

impl PluginMethod {
    /// Return the JSON-RPC method string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extract => "extract",
            Self::Describe => "describe",
            Self::Check => "check",
        }
    }

    /// Parse a JSON-RPC method string.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::UnsupportedMethod`] for non-SPI methods.
    pub fn parse(input: &str) -> Result<Self, PluginHostError> {
        match input {
            "extract" => Ok(Self::Extract),
            "describe" => Ok(Self::Describe),
            "check" => Ok(Self::Check),
            other => Err(PluginHostError::UnsupportedMethod(other.to_owned())),
        }
    }
}

impl PluginRequestId {
    /// Create a request id.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidRequestId`] when the id is empty or
    /// exceeds [`Limits::max_id_bytes`].
    pub fn new(input: impl Into<String>) -> Result<Self, PluginHostError> {
        Self::with_limits(input, Limits::default())
    }

    /// Create a request id using explicit limits.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidRequestId`] when the id is empty or
    /// exceeds [`Limits::max_id_bytes`].
    pub fn with_limits(input: impl Into<String>, limits: Limits) -> Result<Self, PluginHostError> {
        let input = input.into();
        if input.is_empty() {
            return Err(PluginHostError::InvalidRequestId("empty".to_owned()));
        }
        if input.len() > limits.max_id_bytes {
            return Err(PluginHostError::InvalidRequestId("too long".to_owned()));
        }
        Ok(Self(input))
    }

    /// Return the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn as_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

impl PluginBinary {
    /// Create a plugin binary identity.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidPluginBinary`] when the digest is
    /// empty or the path cannot be represented safely.
    pub fn new(path: impl AsRef<Path>, digest: impl Into<String>) -> Result<Self, PluginHostError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(PluginHostError::InvalidPluginBinary(
                "empty path".to_owned(),
            ));
        }
        if path.to_str().is_none() {
            return Err(PluginHostError::InvalidPluginBinary(
                "non-utf8 path".to_owned(),
            ));
        }
        let digest = digest.into();
        if digest.is_empty() {
            return Err(PluginHostError::InvalidPluginBinary(
                "empty plugin digest".to_owned(),
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            digest,
        })
    }

    /// Plugin executable path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Plugin binary digest string.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

impl PluginLaunchConfig {
    /// Create launch config with empty environment and default limits.
    #[must_use]
    pub fn new(binary: PluginBinary) -> Self {
        Self {
            binary,
            cwd: None,
            env: Vec::new(),
            limits: Limits::default(),
            stderr_cap_bytes: 8 * 1024,
            isolation: PluginIsolationMode::DirectForTests,
        }
    }

    /// Set plugin working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_path_buf());
        self
    }

    /// Add one explicit environment variable.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Override limits.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Run plugin through an isolation backend.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::Isolation`] when the profile weakens ADR-009
    /// requirements.
    pub fn with_isolation_backend(
        mut self,
        backend: IsolationBackend,
        profile: PluginIsolationProfile,
    ) -> Result<Self, PluginHostError> {
        profile.validate()?;
        self.isolation = PluginIsolationMode::Backend { backend, profile };
        Ok(self)
    }

    /// Plugin binary identity.
    #[must_use]
    pub fn binary(&self) -> &PluginBinary {
        &self.binary
    }

    /// Configured limits.
    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Build the executable and arguments used to launch the plugin.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::Isolation`] when an isolation profile is
    /// invalid.
    pub fn command_spec(&self) -> Result<(String, Vec<String>), PluginHostError> {
        let plugin_path = self
            .binary
            .path()
            .to_str()
            .ok_or_else(|| PluginHostError::InvalidPluginBinary("non-utf8 path".to_owned()))?;
        match &self.isolation {
            PluginIsolationMode::DirectForTests => Ok((plugin_path.to_owned(), Vec::new())),
            PluginIsolationMode::Backend { backend, profile } => profile
                .backend_command(*backend, plugin_path)
                .map_err(PluginHostError::Isolation),
        }
    }
}

impl fmt::Debug for PluginLaunchConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env_keys = self.env.iter().map(|(key, _)| key).collect::<Vec<_>>();
        f.debug_struct("PluginLaunchConfig")
            .field("binary", &self.binary)
            .field("cwd", &self.cwd)
            .field("env_keys", &env_keys)
            .field("limits", &"Limits { .. }")
            .field("stderr_cap_bytes", &self.stderr_cap_bytes)
            .field("isolation", &self.isolation)
            .finish()
    }
}

impl PluginHostError {
    /// Map host failures into the fail-closed `UnknownReason` used by Layer 3.
    #[must_use]
    pub fn unknown_reason(&self) -> Option<UnknownReason> {
        match self {
            Self::Timeout { .. } => Some(UnknownReason::CheckerTimeout),
            Self::PayloadTooLarge { .. }
            | Self::Json(_)
            | Self::MalformedResponse(_)
            | Self::UnexpectedId { .. }
            | Self::UnsupportedMethod(_)
            | Self::InvalidRequestId(_)
            | Self::InvalidPluginBinary(_)
            | Self::InvalidMemoKey(_)
            | Self::InvalidPoolConfig(_)
            | Self::Isolation(_)
            | Self::Backpressure { .. }
            | Self::Io(_)
            | Self::NonZeroExit { .. } => Some(UnknownReason::PluginFailure),
        }
    }
}

impl PluginPoolConfig {
    /// Create a pool config for one plugin kind.
    #[must_use]
    pub fn new(kind: PluginKind) -> Self {
        Self {
            kind,
            max_processes: 1,
            queue_bound: 32,
        }
    }

    /// Set the maximum number of concurrent plugin process calls.
    #[must_use]
    pub fn with_max_processes(mut self, max_processes: usize) -> Self {
        self.max_processes = max_processes;
        self
    }

    /// Set the number of calls allowed to wait for a process slot.
    #[must_use]
    pub fn with_queue_bound(mut self, queue_bound: usize) -> Self {
        self.queue_bound = queue_bound;
        self
    }

    /// Pool kind.
    #[must_use]
    pub fn kind(&self) -> PluginKind {
        self.kind
    }
}

impl PluginPool {
    /// Create a bounded plugin pool.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidPoolConfig`] when max processes is 0.
    pub fn new(
        config: PluginPoolConfig,
        launch: PluginLaunchConfig,
    ) -> Result<Self, PluginHostError> {
        if config.max_processes == 0 {
            return Err(PluginHostError::InvalidPoolConfig(
                "max_processes must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            config,
            launch,
            state: Arc::new(PoolState {
                inner: Mutex::new(PoolInner {
                    workers: Vec::new(),
                    live_total: 0,
                    waiting: 0,
                }),
                available: Condvar::new(),
            }),
        })
    }

    /// Run one bounded plugin call through this pool.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError`] when the queue is full or the plugin call
    /// fails.
    pub fn call(
        &self,
        method: PluginMethod,
        id: &PluginRequestId,
        params: Value,
        timeout: Duration,
    ) -> Result<JsonRpcResponse, PluginHostError> {
        let request = encode_request_line(method, id, params, self.launch.limits)?;
        let mut worker = self.acquire_worker()?;
        let result = worker.call(request, timeout, self.launch.limits, id);
        match result {
            Ok(response) => {
                self.release_worker(worker);
                Ok(response)
            }
            Err(error) => {
                self.drop_worker(worker);
                Err(error)
            }
        }
    }

    /// Run one bounded call through a memo store.
    ///
    /// Cache hits return exact stored response bytes after protocol validation.
    /// Cache misses run the plugin once, store exact stdout bytes, then return
    /// the decoded response.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError`] when the cache entry is malformed, the queue
    /// is full, or the plugin call fails.
    pub fn call_memoized(
        &self,
        memo: &mut PluginMemoStore,
        protocol_version: &str,
        method: PluginMethod,
        id: &PluginRequestId,
        params: Value,
        timeout: Duration,
    ) -> Result<JsonRpcResponse, PluginHostError> {
        let request_payload = encode_request_payload(method, id, params, self.launch.limits)?;
        let key = PluginMemoKey::new(
            method,
            &request_payload,
            self.launch.binary(),
            protocol_version,
        );
        if let Some(bytes) = memo.get(&key) {
            return decode_response_line(bytes, id, self.launch.limits);
        }
        let mut request_line = request_payload;
        request_line.push(b'\n');
        let mut worker = self.acquire_worker()?;
        let result = worker.call_raw(request_line, timeout, self.launch.limits);
        match result {
            Ok(response_bytes) => {
                let response = decode_response_line(&response_bytes, id, self.launch.limits)?;
                memo.insert(key, response_bytes)?;
                self.release_worker(worker);
                Ok(response)
            }
            Err(error) => {
                self.drop_worker(worker);
                Err(error)
            }
        }
    }

    fn acquire_worker(&self) -> Result<PluginWorker, PluginHostError> {
        let mut inner = self.state.inner.lock().map_err(|_| {
            PluginHostError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "plugin pool mutex poisoned",
            ))
        })?;
        loop {
            if let Some(worker) = inner.workers.pop() {
                return Ok(worker);
            }
            if inner.live_total < self.config.max_processes {
                inner.live_total += 1;
                drop(inner);
                return PluginWorker::spawn(&self.launch).map_err(|error| {
                    self.worker_spawn_failed();
                    error
                });
            }
            if inner.waiting >= self.config.queue_bound {
                return Err(PluginHostError::Backpressure {
                    kind: self.config.kind,
                    active: inner.live_total,
                    waiting: inner.waiting,
                    queue_bound: self.config.queue_bound,
                });
            }
            inner.waiting += 1;
            inner = self.state.available.wait(inner).map_err(|_| {
                PluginHostError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "plugin pool mutex poisoned",
                ))
            })?;
            inner.waiting = inner.waiting.saturating_sub(1);
        }
    }

    fn release_worker(&self, worker: PluginWorker) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.workers.push(worker);
            self.state.available.notify_one();
        }
    }

    fn drop_worker(&self, mut worker: PluginWorker) {
        worker.kill_and_wait();
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.live_total = inner.live_total.saturating_sub(1);
            self.state.available.notify_one();
        }
    }

    fn worker_spawn_failed(&self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.live_total = inner.live_total.saturating_sub(1);
            self.state.available.notify_one();
        }
    }
}

impl PluginMemoKey {
    /// Build a memo key from canonical request bytes and plugin identity.
    #[must_use]
    pub fn new(
        method: PluginMethod,
        request_bytes: &[u8],
        binary: &PluginBinary,
        protocol_version: &str,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"polyref-plugin-memo-v1\0");
        hasher.update(protocol_version.as_bytes());
        hasher.update([0]);
        hasher.update(method.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(binary.digest().as_bytes());
        hasher.update([0]);
        hasher.update(request_bytes);
        Self(format!("{:x}", hasher.finalize()))
    }

    /// Parse a lowercase hex SHA-256 memo key.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::InvalidMemoKey`] when the input is not a
    /// 64-character lowercase hex digest.
    pub fn from_hex(input: impl Into<String>) -> Result<Self, PluginHostError> {
        let input = input.into();
        if input.len() != 64
            || !input
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(PluginHostError::InvalidMemoKey(
                "expected 64 hex characters".to_owned(),
            ));
        }
        Ok(Self(input))
    }

    /// Return the key as hex.
    #[must_use]
    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl Default for PluginMemoStore {
    fn default() -> Self {
        Self::with_response_limit(Limits::default().max_payload_bytes)
    }
}

impl PluginMemoStore {
    /// Create an empty memo store with a response byte cap.
    #[must_use]
    pub fn with_response_limit(response_limit_bytes: usize) -> Self {
        Self {
            responses: BTreeMap::new(),
            response_limit_bytes,
        }
    }

    /// Get exact cached response bytes.
    #[must_use]
    pub fn get(&self, key: &PluginMemoKey) -> Option<&[u8]> {
        self.responses.get(key).map(Vec::as_slice)
    }

    /// Insert exact response bytes.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHostError::PayloadTooLarge`] when the response exceeds
    /// the configured cap.
    pub fn insert(&mut self, key: PluginMemoKey, response: Vec<u8>) -> Result<(), PluginHostError> {
        enforce_payload_limit(response.len(), self.response_limit_bytes)?;
        self.responses.insert(key, response);
        Ok(())
    }
}

/// Encode a JSON-RPC request as one canonical transport line.
///
/// The returned bytes contain exactly one trailing newline. The cached request
/// bytes for memoization should use [`encode_request_payload`] instead.
///
/// # Errors
///
/// Returns [`PluginHostError`] for serialization failure or size-cap overflow.
pub fn encode_request_line(
    method: PluginMethod,
    id: &PluginRequestId,
    params: Value,
    limits: Limits,
) -> Result<Vec<u8>, PluginHostError> {
    let mut payload = encode_request_payload(method, id, params, limits)?;
    payload.push(b'\n');
    Ok(payload)
}

/// Encode a JSON-RPC request payload without transport newline.
///
/// # Errors
///
/// Returns [`PluginHostError`] for serialization failure or size-cap overflow.
pub fn encode_request_payload(
    method: PluginMethod,
    id: &PluginRequestId,
    params: Value,
    limits: Limits,
) -> Result<Vec<u8>, PluginHostError> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        method: method.as_str().to_owned(),
        id: id.as_value(),
        params,
    };
    let payload = serde_json::to_vec(&request)?;
    enforce_payload_limit(payload.len(), limits.max_payload_bytes)?;
    Ok(payload)
}

/// Decode and validate one JSON-RPC response line.
///
/// # Errors
///
/// Returns [`PluginHostError`] for malformed framing, JSON, size-cap overflow,
/// id mismatch, or result/error shape violations.
pub fn decode_response_line(
    line: &[u8],
    expected_id: &PluginRequestId,
    limits: Limits,
) -> Result<JsonRpcResponse, PluginHostError> {
    reject_lsp_framing(line)?;
    let payload = trim_one_line_ending(line)?;
    enforce_payload_limit(payload.len(), limits.max_payload_bytes)?;
    let response: JsonRpcResponse = serde_json::from_slice(payload)?;
    validate_response(&response, expected_id)?;
    Ok(response)
}

/// Run one plugin request against one plugin process.
///
/// This is the Layer 3 single-call primitive. Pooling/reuse is intentionally
/// layered above it so timeout/crash semantics stay easy to audit.
///
/// # Errors
///
/// Returns [`PluginHostError`] for spawn/I/O failure, timeout, non-zero exit,
/// malformed response, or protocol validation failure.
pub fn run_plugin_call(
    config: &PluginLaunchConfig,
    method: PluginMethod,
    id: &PluginRequestId,
    params: Value,
    timeout: Duration,
) -> Result<JsonRpcResponse, PluginHostError> {
    let request = encode_request_line(method, id, params, config.limits)?;
    let stdout = run_plugin_request_line(config, request, timeout)?;
    decode_response_line(&stdout, id, config.limits)
}

fn run_plugin_request_line(
    config: &PluginLaunchConfig,
    request: Vec<u8>,
    timeout: Duration,
) -> Result<Vec<u8>, PluginHostError> {
    let (program, args) = config.command_spec()?;
    let mut command = Command::new(program);
    command.args(args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    if let Some(cwd) = &config.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &config.env {
        command.env(key, value);
    }

    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        PluginHostError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "plugin stdin unavailable",
        ))
    })?;
    stdin.write_all(&request)?;
    drop(stdin);

    let stdout = child.stdout.take().ok_or_else(|| {
        PluginHostError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "plugin stdout unavailable",
        ))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        PluginHostError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "plugin stderr unavailable",
        ))
    })?;
    let stdout_cap = config.limits.max_payload_bytes.saturating_add(1);
    let stdout_reader = thread::spawn(move || read_capped(stdout, stdout_cap));
    let stderr_cap = config.stderr_cap_bytes;
    let stderr_reader = thread::spawn(move || read_capped(stderr, stderr_cap));

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let _ = child.wait();
            let _ = join_reader(stdout_reader)?;
            let _ = join_reader(stderr_reader)?;
            return Err(PluginHostError::Timeout {
                timeout_ms: timeout.as_millis(),
            });
        }
        thread::sleep(Duration::from_millis(5));
    };

    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if !status.success() {
        return Err(PluginHostError::NonZeroExit {
            code: status.code(),
            stderr_bytes: stderr.len(),
        });
    }
    enforce_payload_limit(stdout.len(), config.limits.max_payload_bytes)?;
    Ok(stdout)
}

impl PluginWorker {
    fn spawn(config: &PluginLaunchConfig) -> Result<Self, PluginHostError> {
        let (program, args) = config.command_spec()?;
        let mut command = Command::new(program);
        command.args(args);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &config.env {
            command.env(key, value);
        }

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| {
            PluginHostError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "plugin stdin unavailable",
            ))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PluginHostError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "plugin stdout unavailable",
            ))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            PluginHostError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "plugin stderr unavailable",
            ))
        })?;
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let stdout_cap = config.limits.max_payload_bytes.saturating_add(1);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line.len() > stdout_cap {
                            let _ = stdout_tx.send(Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "plugin stdout line exceeds cap",
                            )));
                            break;
                        }
                        if stdout_tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = stdout_tx.send(Err(error));
                        break;
                    }
                }
            }
        });
        let stderr_cap = config.stderr_cap_bytes;
        let stderr_reader = Some(thread::spawn(move || read_capped(stderr, stderr_cap)));

        Ok(Self {
            child,
            stdin,
            stdout_rx,
            stderr_reader,
        })
    }

    fn call(
        &mut self,
        request: Vec<u8>,
        timeout: Duration,
        limits: Limits,
        expected_id: &PluginRequestId,
    ) -> Result<JsonRpcResponse, PluginHostError> {
        let response = self.call_raw(request, timeout, limits)?;
        decode_response_line(&response, expected_id, limits)
    }

    fn call_raw(
        &mut self,
        request: Vec<u8>,
        timeout: Duration,
        limits: Limits,
    ) -> Result<Vec<u8>, PluginHostError> {
        if let Some(status) = self.child.try_wait()? {
            return Err(PluginHostError::NonZeroExit {
                code: status.code(),
                stderr_bytes: self.stderr_len(),
            });
        }
        self.stdin.write_all(&request)?;
        self.stdin.flush()?;
        match self.stdout_rx.recv_timeout(timeout) {
            Ok(Ok(response)) => {
                enforce_payload_limit(response.len(), limits.max_payload_bytes)?;
                Ok(response)
            }
            Ok(Err(error)) => Err(PluginHostError::Io(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.kill_and_wait();
                Err(PluginHostError::Timeout {
                    timeout_ms: timeout.as_millis(),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = self.child.try_wait()?;
                Err(PluginHostError::NonZeroExit {
                    code: status.and_then(|status| status.code()),
                    stderr_bytes: self.stderr_len(),
                })
            }
        }
    }

    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn stderr_len(&mut self) -> usize {
        self.stderr_reader
            .take()
            .and_then(|reader| join_reader(reader).ok())
            .map_or(0, |stderr| stderr.len())
    }
}

impl Drop for PluginWorker {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn validate_response(
    response: &JsonRpcResponse,
    expected_id: &PluginRequestId,
) -> Result<(), PluginHostError> {
    if response.jsonrpc != "2.0" {
        return Err(PluginHostError::MalformedResponse(
            "invalid jsonrpc version".to_owned(),
        ));
    }
    if response.id != expected_id.as_value() {
        return Err(PluginHostError::UnexpectedId {
            expected: expected_id.as_str().to_owned(),
            actual: response.id.to_string(),
        });
    }
    match (&response.result, &response.error) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        (Some(_), Some(_)) => Err(PluginHostError::MalformedResponse(
            "response contains both result and error".to_owned(),
        )),
        (None, None) => Err(PluginHostError::MalformedResponse(
            "response contains neither result nor error".to_owned(),
        )),
    }
}

fn reject_lsp_framing(line: &[u8]) -> Result<(), PluginHostError> {
    if line.starts_with(b"Content-Length:") || line.starts_with(b"content-length:") {
        return Err(PluginHostError::MalformedResponse(
            "LSP-style framing is not supported".to_owned(),
        ));
    }
    Ok(())
}

fn trim_one_line_ending(line: &[u8]) -> Result<&[u8], PluginHostError> {
    let trimmed = match line.strip_suffix(b"\n") {
        Some(without_lf) => without_lf.strip_suffix(b"\r").unwrap_or(without_lf),
        None => line,
    };
    if trimmed.contains(&b'\n') || trimmed.contains(&b'\r') {
        return Err(PluginHostError::MalformedResponse(
            "response must be a single JSON line".to_owned(),
        ));
    }
    if trimmed.is_empty() {
        return Err(PluginHostError::MalformedResponse(
            "empty response".to_owned(),
        ));
    }
    Ok(trimmed)
}

fn enforce_payload_limit(actual: usize, limit: usize) -> Result<(), PluginHostError> {
    if actual > limit {
        return Err(PluginHostError::PayloadTooLarge { limit, actual });
    }
    Ok(())
}

fn read_capped<R: Read>(mut reader: R, cap: usize) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    while buffer.len() < cap {
        let remaining = cap - buffer.len();
        let read_len = remaining.min(chunk.len());
        let n = reader.read(&mut chunk[..read_len])?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    Ok(buffer)
}

fn join_reader(
    reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, PluginHostError> {
    reader
        .join()
        .map_err(|_| {
            PluginHostError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "plugin reader thread panicked",
            ))
        })?
        .map_err(PluginHostError::Io)
}

#[allow(dead_code)]
fn _json_rpc_error_is_public_contract(_: JsonRpcError) {}
