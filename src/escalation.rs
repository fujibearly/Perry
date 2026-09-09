//! Inter-agent escalation & control channel (backlog #6d).
//!
//! This module owns the **transport** for the escalation/control protocol whose
//! *message types* live in [`crate::safety`] ([`UpstreamMsg`]/[`DownstreamMsg`]
//! and their payloads). Separation of concerns:
//!
//!   - `safety.rs` — the deterministic decision core + the transport-independent
//!     message schema (stable wire types).
//!   - `escalation.rs` (here) — the mutual-TLS connection, the length-delimited
//!     JSON framing, the auth handshake, and (later tasks) the child/parent
//!     connection handlers, rollback journal, and human-in-the-loop paths.
//!
//! ## Transport (Path 1′ — see the spec's "As-Built Notes — #6d")
//!
//! Mutual TLS over a **raw loopback TCP stream** (`tokio-rustls`), with a
//! hand-rolled **length-delimited JSON** framing: each frame is a 4-byte
//! big-endian length prefix followed by that many bytes of `serde_json`. This is
//! deliberately *not* WebSocket — WS framing (masking/opcodes/ping-pong) is
//! browser/proxy machinery a parent↔child loopback channel does not need, and it
//! avoids the `tokio-tungstenite` dependency. TLS — the security-critical,
//! remote-valuable part — is built now and reuses the `tokio-rustls` already in
//! the tree.
//!
//! ## Remote generalization (FR-6d.12)
//!
//! The [`EscalationTransport`] trait abstracts *how bytes move* from *what is
//! exchanged*. The loopback-TLS impl is the only concrete transport today; a
//! WebSocket-over-routable-TLS transport can be added as a second impl behind the
//! same trait when a remote deployment (proxy/browser in the path) needs it —
use crate::safety::{DownstreamMsg, UpstreamMsg};

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex as ParkingMutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

/// Hard cap on a single frame's payload, to bound memory from a malformed or
/// hostile length prefix. The control channel carries small JSON control
/// messages (never bulk artifacts — those stay isolated per Pillars 1/2), so a
/// generous-but-finite cap is safe. 8 MiB.
pub const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;

// ===========================================================================
// Mutual authentication (#6d, FR-6d.3) — the security-critical core.
//
// Model (mTLS-style, no CA/PKI):
//   * The **parent** generates ONE ephemeral self-signed cert+key per agent tree,
//     in memory, never written to disk (`generate_tree_identity`). Its TLS server
//     uses this cert.
//   * The **child** authenticates the parent by *pinning the cert fingerprint*
//     (SHA-256 of the DER), passed via env. Its custom `ServerCertVerifier`
//     accepts ONLY that exact fingerprint — no CA, **fail-closed** on any other
//     certificate. (Parent → child authentication.)
//   * The **child** authenticates itself to the parent with a **channel-bound
//     challenge–response** at the application layer (not a client cert): the
//     parent sends a random nonce; the child replies
//     `HMAC(tree_secret, nonce || parent_fingerprint)`. Binding the parent's
//     fingerprint into the MAC makes a captured credential non-replayable against
//     a different parent/connection. (Child → parent authentication.)
//
// TLS (via `tokio-rustls`, reusing the in-tree rustls) provides confidentiality
// and parent authentication; the HMAC challenge, bound to the TLS cert, provides
// child authentication. Together this is mutual auth without a client-cert PKI.
// ===========================================================================

/// An ephemeral, in-memory identity for one agent tree: a self-signed cert, its
/// private key, and the SHA-256 fingerprint children pin. Never persisted.
pub struct TreeIdentity {
    /// DER-encoded self-signed certificate.
    pub cert_der: Vec<u8>,
    /// DER-encoded PKCS#8 private key.
    pub key_der: Vec<u8>,
    /// Lowercase hex SHA-256 of `cert_der` — the value a child pins.
    pub fingerprint: String,
}

/// Generate a fresh ephemeral self-signed identity for a tree (FR-6d.3).
///
/// Uses `rcgen`'s default (ECDSA P-256). The cert is bound to a fixed SAN
/// (`aichat-agent.local`) that children present as the server name; identity is
/// established by fingerprint pinning, not the name, so the SAN is only a
/// formality rustls requires.
pub fn generate_tree_identity() -> Result<TreeIdentity> {
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec!["aichat-agent.local".to_string()])
            .context("failed to generate ephemeral agent-tree certificate")?;
    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();
    let fingerprint = crate::utils::sha256_bytes(&cert_der);
    Ok(TreeIdentity {
        cert_der,
        key_der,
        fingerprint,
    })
}

/// The fixed server name children present. Identity is by fingerprint, not name.
pub const AGENT_SERVER_NAME: &str = "aichat-agent.local";

/// A rustls `ServerCertVerifier` that accepts a certificate **iff** its SHA-256
/// fingerprint matches the pinned value. No CA, no name check — fingerprint only.
/// Fails closed: any mismatch, or an unexpected chain, is rejected.
#[derive(Debug)]
pub struct FingerprintPinVerifier {
    pinned_fingerprint: String,
}

impl FingerprintPinVerifier {
    pub fn new(pinned_fingerprint: impl Into<String>) -> Self {
        Self {
            pinned_fingerprint: pinned_fingerprint.into(),
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for FingerprintPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let presented = crate::utils::sha256_bytes(end_entity.as_ref());
        // Constant-time-ish compare is overkill for a public fingerprint, but the
        // decision is strict equality and fails closed on any mismatch.
        if presented == self.pinned_fingerprint {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "escalation channel: server certificate fingerprint does not match the pinned \
                 parent identity (rejecting — possible MITM or wrong parent)"
                    .to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Compute the channel-bound child credential:
/// `HMAC-SHA256(tree_secret, nonce || "|" || parent_fingerprint)`, hex-encoded.
///
/// Binding `parent_fingerprint` means a credential captured on one connection
/// cannot be replayed against a different parent (different cert → different
/// fingerprint → different expected MAC). `nonce` makes it per-connection.
pub fn compute_channel_credential(tree_secret: &str, nonce: &str, parent_fingerprint: &str) -> String {
    let msg = format!("{nonce}|{parent_fingerprint}");
    crate::utils::hex_encode(&crate::utils::hmac_sha256(tree_secret.as_bytes(), &msg))
}

/// Verify a child's presented credential in constant time w.r.t. length, failing
/// closed. Returns `true` only on an exact match of the expected MAC.
pub fn verify_channel_credential(
    tree_secret: &str,
    nonce: &str,
    parent_fingerprint: &str,
    presented: &str,
) -> bool {
    let expected = compute_channel_credential(tree_secret, nonce, parent_fingerprint);
    // Length-independent equality to avoid trivial timing leaks on the hex string.
    constant_time_eq(expected.as_bytes(), presented.as_bytes())
}

/// Constant-time byte comparison (no early exit on first mismatch).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// A transport that carries the typed escalation protocol.
// ---------------------------------------------------------------------------

/// Abstracts the wire so the loopback mutual-TLS transport (now) and a future
/// remote transport (WebSocket-over-routable-TLS) share one protocol + auth
/// model. Direction is fixed per connection end: a child sends [`UpstreamMsg`]
/// and receives [`DownstreamMsg`]; a parent's per-child handler does the inverse.
#[async_trait::async_trait]
pub trait EscalationTransport: Send {
    async fn send_upstream(&mut self, msg: &UpstreamMsg) -> Result<()>;
    async fn recv_upstream(&mut self) -> Result<Option<UpstreamMsg>>;
    async fn send_downstream(&mut self, msg: &DownstreamMsg) -> Result<()>;
    #[allow(dead_code)]
    async fn recv_downstream(&mut self) -> Result<Option<DownstreamMsg>>;
}

// ---------------------------------------------------------------------------
// TLS config construction (rustls 0.23, explicit `ring` provider)
// ---------------------------------------------------------------------------

/// Build the rustls `ServerConfig` for the **parent**: presents the tree's
/// ephemeral self-signed cert, requires no client cert (child auth is the
/// application-layer channel-bound challenge, not a client cert).
///
/// Uses an explicit `ring` provider so we don't depend on a process-global
/// default provider being installed (reqwest may or may not have installed one).
fn build_server_config(identity: &TreeIdentity) -> Result<rustls::ServerConfig> {
    let cert = rustls::pki_types::CertificateDer::from(identity.cert_der.clone());
    let key = rustls::pki_types::PrivateKeyDer::try_from(identity.key_der.clone())
        .map_err(|e| anyhow!("invalid ephemeral private key: {e}"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("server tls protocol versions")?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .context("server tls single cert")?;
    Ok(config)
}

/// Build the rustls `ClientConfig` for the **child**: pins the parent's cert
/// fingerprint via a custom verifier (no CA, fail-closed), presents no client
/// cert. Explicit `ring` provider (see [`build_server_config`]).
fn build_client_config(pinned_fingerprint: &str) -> Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("client tls protocol versions")?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(FingerprintPinVerifier::new(
            pinned_fingerprint.to_string(),
        )))
        .with_no_client_auth();
    Ok(config)
}

// ---------------------------------------------------------------------------
// Concrete loopback mutual-TLS transport
// ---------------------------------------------------------------------------

/// A concrete [`EscalationTransport`] over a `tokio_rustls` TLS stream (either
/// the server or client side of a loopback connection). Splits the stream so the
/// framing read/write halves can be used independently, but exposes the simple
/// send/recv trait surface.
pub struct TlsTransport<S> {
    stream: S,
}

impl<S> TlsTransport<S>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send,
{
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    pub fn into_inner(self) -> S {
        self.stream
    }
}

#[async_trait::async_trait]
impl<S> EscalationTransport for TlsTransport<S>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send,
{
    async fn send_upstream(&mut self, msg: &UpstreamMsg) -> Result<()> {
        write_frame(&mut self.stream, msg).await
    }
    async fn recv_upstream(&mut self) -> Result<Option<UpstreamMsg>> {
        read_frame(&mut self.stream).await
    }
    async fn send_downstream(&mut self, msg: &DownstreamMsg) -> Result<()> {
        write_frame(&mut self.stream, msg).await
    }
    async fn recv_downstream(&mut self) -> Result<Option<DownstreamMsg>> {
        read_frame(&mut self.stream).await
    }
}

// ---------------------------------------------------------------------------
// Handshake wire format (the tiny pre-protocol auth exchange, sent as frames
// BEFORE the typed UpstreamMsg/DownstreamMsg protocol begins).
// ---------------------------------------------------------------------------

/// Parent → child, first frame after TLS: the per-connection challenge nonce.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChallengeFrame {
    nonce: String,
}

/// Child → parent, response: the channel-bound credential proving knowledge of
/// the tree secret, bound to this connection's nonce + the parent's fingerprint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CredentialFrame {
    agent_id: String,
    credential: String,
}

// ---------------------------------------------------------------------------
// Parent side: bind a loopback listener, run the handshake, authenticate a child
// ---------------------------------------------------------------------------

/// A bound parent listener plus the env values a child needs to dial back.
pub struct ParentListener {
    listener: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    identity: Arc<TreeIdentity>,
    tree_secret: String,
    tree_id: String,
}

impl ParentListener {
    /// Bind a loopback-only TLS listener on an ephemeral port for this tree.
    pub async fn bind(identity: Arc<TreeIdentity>, tree_id: String, tree_secret: String) -> Result<Self> {
        let server_config = build_server_config(&identity)?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        // Loopback ONLY (defense in depth) — never 0.0.0.0.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("binding loopback escalation listener")?;
        Ok(Self {
            listener,
            acceptor,
            identity,
            tree_secret,
            tree_id,
        })
    }

    /// The `host:port` a child dials (loopback).
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// The parent cert fingerprint a child pins.
    #[allow(dead_code)]
    pub fn fingerprint(&self) -> &str {
        &self.identity.fingerprint
    }

    /// The env vars to pass to a spawned child so it can dial back and authenticate.
    pub fn child_env(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![
            ("AICHAT_AGENT_PARENT_ADDR".to_string(), self.local_addr()?.to_string()),
            ("AICHAT_AGENT_PARENT_FP".to_string(), self.identity.fingerprint.clone()),
            ("AICHAT_TREE_SECRET".to_string(), self.tree_secret.clone()),
            ("AICHAT_TREE_ID".to_string(), self.tree_id.clone()),
        ])
    }

    /// Accept the next child: complete the TLS handshake, issue the challenge,
    /// verify the child's channel-bound credential, and read its `Hello`.
    /// Returns an authenticated transport + the child's `HelloMsg`, or an error
    /// (the connection is dropped on any auth failure — **fail closed**).
    pub async fn accept_authenticated(
        &self,
    ) -> Result<(TlsTransport<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>, crate::safety::HelloMsg)>
    {
        let (tcp, _peer) = self.listener.accept().await.context("accept tcp")?;
        let tls = self.acceptor.accept(tcp).await.context("server tls handshake")?;
        let mut stream = tls;

        // 1. Send the per-connection challenge nonce.
        let nonce = crate::utils::sha256_bytes(
            format!(
                "{}-{}-{}",
                self.tree_id,
                std::process::id(),
                uuid::Uuid::new_v4()
            )
            .as_bytes(),
        );
        write_frame(&mut stream, &ChallengeFrame { nonce: nonce.clone() }).await?;

        // 2. Read the child's credential and verify it (channel-bound).
        let cred: CredentialFrame = read_frame(&mut stream)
            .await?
            .ok_or_else(|| anyhow!("child closed before presenting credential"))?;
        if !verify_channel_credential(
            &self.tree_secret,
            &nonce,
            &self.identity.fingerprint,
            &cred.credential,
        ) {
            bail!("escalation: child failed channel-bound authentication (rejecting connection)");
        }

        // 3. Read the child's Hello (first typed protocol message).
        let transport = TlsTransport::new(stream);
        let mut transport = transport;
        let hello = match transport.recv_upstream().await? {
            Some(UpstreamMsg::Hello(h)) => h,
            Some(other) => bail!("expected Hello as first message, got {other:?}"),
            None => bail!("child closed before sending Hello"),
        };
        Ok((transport, hello))
    }
}

// ---------------------------------------------------------------------------
// Child side: dial the parent, authenticate, send Hello
// ---------------------------------------------------------------------------

/// Read the parent-connection env vars (set by the parent when it spawned us).
/// Returns `None` when unset (top-level process, or #6d disabled) — the caller
/// then behaves exactly as pre-#6d (block instead of escalate).
pub struct ParentConnInfo {
    pub addr: String,
    pub fingerprint: String,
    pub tree_secret: String,
    pub tree_id: String,
}

impl ParentConnInfo {
    pub fn from_env() -> Option<ParentConnInfo> {
        let addr = std::env::var("AICHAT_AGENT_PARENT_ADDR").ok()?;
        let fingerprint = std::env::var("AICHAT_AGENT_PARENT_FP")
            .or_else(|_| std::env::var("AICHAT_AGENT_PARENT_FINGERPRINT"))
            .ok()?;
        let tree_secret = std::env::var("AICHAT_TREE_SECRET")
            .or_else(|_| std::env::var("AICHAT_AGENT_TREE_SECRET"))
            .ok()?;
        let tree_id = std::env::var("AICHAT_TREE_ID")
            .or_else(|_| std::env::var("AICHAT_AGENT_TREE_ID"))
            .unwrap_or_default();
        Some(ParentConnInfo {
            addr,
            fingerprint,
            tree_secret,
            tree_id,
        })
    }
}

/// Dial the parent, complete the mutual-TLS + channel-bound handshake, and send
/// `Hello`. Returns an authenticated transport on success. Fails closed: a
/// fingerprint mismatch aborts the TLS handshake before any bytes are exchanged.
pub async fn dial_parent(
    info: &ParentConnInfo,
    agent_id: &str,
    depth: usize,
) -> Result<TlsTransport<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>> {
    let client_config = build_client_config(&info.fingerprint)?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));

    let tcp = tokio::net::TcpStream::connect(&info.addr)
        .await
        .with_context(|| format!("connecting to parent at {}", info.addr))?;
    let server_name = rustls::pki_types::ServerName::try_from(AGENT_SERVER_NAME)
        .context("server name")?
        .to_owned();
    let mut stream = connector
        .connect(server_name, tcp)
        .await
        .context("client tls handshake (fingerprint pin)")?;

    // 1. Read the challenge nonce.
    let challenge: ChallengeFrame = read_frame(&mut stream)
        .await?
        .ok_or_else(|| anyhow!("parent closed before sending challenge"))?;

    // 2. Send the channel-bound credential.
    let credential =
        compute_channel_credential(&info.tree_secret, &challenge.nonce, &info.fingerprint);
    write_frame(
        &mut stream,
        &CredentialFrame {
            agent_id: agent_id.to_string(),
            credential,
        },
    )
    .await?;

    // 3. Send Hello (first typed protocol message).
    let mut transport = TlsTransport::new(stream);
    transport
        .send_upstream(&UpstreamMsg::Hello(crate::safety::HelloMsg {
            agent_id: agent_id.to_string(),
            depth,
            capabilities: vec![],
        }))
        .await?;
    Ok(transport)
}

/// Dial the parent with transient retry (up to `max_retries` attempts with exponential backoff)
/// to tolerate socket handshake flukes while strictly failing closed on permanent failure.
pub async fn dial_parent_with_retry(
    info: &ParentConnInfo,
    agent_id: &str,
    depth: usize,
    max_retries: usize,
) -> Result<TlsTransport<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>> {
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(100 * (1 << (attempt - 1)))).await;
        }
        match dial_parent(info, agent_id, depth).await {
            Ok(transport) => return Ok(transport),
            Err(e) => {
                // If it's a security rejection (fingerprint mismatch), fail closed immediately without retry.
                let err_str = e.to_string();
                if err_str.contains("fingerprint") || err_str.contains("channel-bound") {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("failed to connect to parent after {max_retries} retries")))
}

/// An outbound message queued to the dedicated writer task.
#[derive(Debug)]
pub enum OutboundMsg {
    /// Fire-and-forget event message (drop-newest if queue is full).
    Event(serde_json::Value),
    /// Request/response escalation message.
    Escalation(crate::safety::EscalationMsg),
    /// Terminal result or error with optional flush acknowledgment.
    Result {
        msg: UpstreamMsg,
        flush_ack: Option<oneshot::Sender<()>>,
    },
}

/// A persistent, reused mutual-TLS client connection from a child sub-agent
/// to its parent listener (backlog #6d Part 2).
///
/// # Architecture:
/// - Reuses a single mTLS TCP connection for the entire child lifetime.
/// - **Sole-Writer Actor:** A dedicated background Writer Task owns `WriteHalf`.
///   Frames are sent through `writer_tx: mpsc::Sender<OutboundMsg>` to eliminate
///   any chance of byte interleaving or framing desync.
type DemuxMap =
    Arc<ParkingMutex<HashMap<String, oneshot::Sender<Result<crate::safety::VerdictMsg, String>>>>>;

/// A persistent, reused mutual-TLS client connection from a child sub-agent
/// to its parent listener (backlog #6d Part 2).
///
/// # Architecture:
/// - Reuses a single mTLS TCP connection for the entire child lifetime.
/// - **Sole-Writer Actor:** A dedicated background Writer Task owns `WriteHalf`.
///   Frames are sent through `writer_tx: mpsc::Sender<OutboundMsg>` to eliminate
///   any chance of byte interleaving or framing desync.
/// - **Drop-Newest Events:** Events use `try_send` on a bounded channel (capacity: 1024),
///   guaranteeing non-blocking execution for rapid agent loops.
/// - **Demultiplexed Reader:** A dedicated background Reader Task owns `ReadHalf`,
///   routing inbound `VerdictMsg` to waiting callers via an in-memory `oneshot` registry.
/// - **Immediate Fail-Closed:** Socket EOF or error immediately drains all in-flight
///   escalations and fails them closed.
pub struct ChildEscalationClient {
    writer_tx: mpsc::Sender<OutboundMsg>,
    demux: DemuxMap,
    is_closed: Arc<AtomicBool>,
}

impl ChildEscalationClient {
    /// Establish the persistent authenticated connection to the parent listener,
    /// perform the mTLS + HMAC handshake, send the initial `Hello`, and spawn
    /// the dedicated reader and writer tasks.
    pub async fn connect(
        info: &ParentConnInfo,
        agent_id: &str,
        depth: usize,
    ) -> Result<Arc<Self>> {
        let transport = dial_parent_with_retry(info, agent_id, depth, 3).await?;
        let stream = transport.into_inner();
        let (mut read_half, mut write_half) = tokio::io::split(stream);

        let (writer_tx, mut writer_rx) = mpsc::channel::<OutboundMsg>(1024);
        let demux: DemuxMap = Arc::new(ParkingMutex::new(HashMap::new()));
        let is_closed = Arc::new(AtomicBool::new(false));

        // 1. Dedicated background Writer Task (sole owner of WriteHalf)
        let is_closed_w = is_closed.clone();
        tokio::spawn(async move {
            while let Some(outbound) = writer_rx.recv().await {
                if is_closed_w.load(Ordering::Relaxed) {
                    break;
                }
                match outbound {
                    OutboundMsg::Event(ev) => {
                        let upstream = UpstreamMsg::Event(ev);
                        if write_frame(&mut write_half, &upstream).await.is_err() {
                            is_closed_w.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    OutboundMsg::Escalation(esc) => {
                        let upstream = UpstreamMsg::Escalation(esc);
                        if write_frame(&mut write_half, &upstream).await.is_err() {
                            is_closed_w.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    OutboundMsg::Result { msg, flush_ack } => {
                        let res = write_frame(&mut write_half, &msg).await;
                        if let Some(ack) = flush_ack {
                            let _ = ack.send(());
                        }
                        if res.is_err() {
                            is_closed_w.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
        });

        // 2. Dedicated background Reader Task (sole owner of ReadHalf)
        let demux_r = demux.clone();
        let is_closed_r = is_closed.clone();
        tokio::spawn(async move {
            loop {
                match read_frame::<_, DownstreamMsg>(&mut read_half).await {
                    Ok(Some(DownstreamMsg::Verdict(v))) => {
                        let mut map = demux_r.lock();
                        if let Some(tx) = map.remove(&v.escalation_id) {
                            let _ = tx.send(Ok(v));
                        }
                    }
                    Ok(Some(DownstreamMsg::Cancel(c))) => {
                        let mut map = demux_r.lock();
                        let reason = format!("parent cancelled execution: {}", c.reason);
                        for (_id, tx) in map.drain() {
                            let _ = tx.send(Err(reason.clone()));
                        }
                    }
                    Ok(None) => {
                        // Clean EOF at frame boundary
                        is_closed_r.store(true, Ordering::Relaxed);
                        let mut map = demux_r.lock();
                        for (_id, tx) in map.drain() {
                            let _ = tx.send(Err("parent closed connection (fail-closed)".to_string()));
                        }
                        break;
                    }
                    Err(e) => {
                        is_closed_r.store(true, Ordering::Relaxed);
                        let mut map = demux_r.lock();
                        let err_msg = format!("parent connection error: {e} (fail-closed)");
                        for (_id, tx) in map.drain() {
                            let _ = tx.send(Err(err_msg.clone()));
                        }
                        break;
                    }
                }
            }
        });

        Ok(Arc::new(Self {
            writer_tx,
            demux,
            is_closed,
        }))
    }

    /// Check whether the client connection remains open and active.
    pub fn is_connected(&self) -> bool {
        !self.is_closed.load(Ordering::Relaxed) && !self.writer_tx.is_closed()
    }

    /// Emit an event non-blockingly over the persistent connection.
    /// Drops the event (drop-newest) if the 1024-element buffer is saturated.
    pub fn send_event(&self, event: serde_json::Value) {
        if self.is_closed.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.writer_tx.try_send(OutboundMsg::Event(event));
    }

    /// Perform a synchronous request/response escalation over the persistent connection.
    /// Registers a oneshot in the demux table and awaits the verdict with timeout.
    /// Strictly fails closed on timeout, cancellation, or connection loss.
    pub async fn escalate(
        &self,
        esc: crate::safety::EscalationMsg,
        timeout_secs: u64,
    ) -> Result<crate::safety::VerdictMsg> {
        if self.is_closed.load(Ordering::Relaxed) {
            bail!("escalation channel: parent connection is closed (fail-closed)");
        }
        let escalation_id = esc.id.clone();
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.demux.lock();
            map.insert(escalation_id.clone(), tx);
        }

        if let Err(e) = self.writer_tx.send(OutboundMsg::Escalation(esc)).await {
            self.demux.lock().remove(&escalation_id);
            self.is_closed.store(true, Ordering::Relaxed);
            bail!("escalation channel: failed to transmit escalation: {e}");
        }

        let timeout_duration = std::time::Duration::from_secs(timeout_secs.max(1));
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(Ok(verdict))) => Ok(verdict),
            Ok(Ok(Err(err_msg))) => {
                bail!("escalation channel: {err_msg}");
            }
            Ok(Err(_dropped)) => {
                bail!("escalation channel: parent reader dropped without sending verdict (fail-closed)");
            }
            Err(_timeout) => {
                self.demux.lock().remove(&escalation_id);
                bail!("escalation channel: timed out awaiting verdict after {timeout_secs}s (fail-closed)");
            }
        }
    }

    /// Send terminal task outcome (Result or Error) and await acknowledgment of the write flush.
    pub async fn send_result(
        &self,
        result: Result<serde_json::Value, String>,
        cost: f64,
    ) -> Result<()> {
        let msg = match result {
            Ok(output) => UpstreamMsg::Result(crate::safety::ResultMsg { output, cost }),
            Err(message) => UpstreamMsg::Error(crate::safety::ErrorMsg { message }),
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        let outbound = OutboundMsg::Result {
            msg,
            flush_ack: Some(ack_tx),
        };
        self.writer_tx
            .send(outbound)
            .await
            .map_err(|e| anyhow!("failed to enqueue result: {e}"))?;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), ack_rx).await;
        Ok(())
    }
}

/// Execute a complete, self-contained escalation transaction (FR-6d.4):
/// Connect to parent, send EscalationMsg, await VerdictMsg within timeout, and close connection.
/// Strictly fails closed on timeout, disconnect, or denial.
pub async fn escalate_to_parent(
    info: &ParentConnInfo,
    agent_id: &str,
    depth: usize,
    escalation: crate::safety::EscalationMsg,
    timeout_secs: u64,
) -> Result<crate::safety::VerdictMsg> {
    let client = ChildEscalationClient::connect(info, agent_id, depth)
        .await
        .context("escalation channel: failed to establish authenticated connection to parent")?;
    client.escalate(escalation, timeout_secs).await
}

/// Notify the parent of a sub-agent execution event (FR-6d.4).
#[allow(dead_code)]
pub async fn notify_parent_event(
    info: &ParentConnInfo,
    agent_id: &str,
    depth: usize,
    event: serde_json::Value,
) -> Result<()> {
    let client = ChildEscalationClient::connect(info, agent_id, depth).await?;
    client.send_event(event);
    Ok(())
}

/// Notify the parent of terminal task completion or error (FR-6d.4).
pub async fn notify_parent_result(
    info: &ParentConnInfo,
    agent_id: &str,
    depth: usize,
    result: Result<serde_json::Value, String>,
    cost: f64,
) -> Result<()> {
    let client = ChildEscalationClient::connect(info, agent_id, depth).await?;
    client.send_result(result, cost).await
}

// ---------------------------------------------------------------------------
// Length-delimited JSON framing (pure, transport-agnostic over any AsyncRead/Write)
// ---------------------------------------------------------------------------

/// Write one length-delimited JSON frame: a 4-byte big-endian length prefix
/// followed by the serialized `value`.
pub async fn write_frame<W, T>(w: &mut W, value: &T) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: serde::Serialize,
{
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MAX_FRAME_BYTES as u64 {
        bail!(
            "escalation frame too large: {} bytes exceeds cap {}",
            bytes.len(),
            MAX_FRAME_BYTES
        );
    }
    let len = (bytes.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

/// Read one length-delimited JSON frame. Returns `Ok(None)` on a clean EOF at a
/// frame boundary (the peer closed the connection) — this is how liveness is
/// detected (FR-6d.9), not an error. A truncated frame mid-read *is* an error.
pub async fn read_frame<R, T>(r: &mut R) -> Result<Option<T>>
where
    R: AsyncReadExt + Unpin,
    T: serde::de::DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    // Distinguish a clean EOF at the boundary from a truncated frame.
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        bail!(
            "escalation frame length {} exceeds cap {} (framing desync or hostile peer)",
            len,
            MAX_FRAME_BYTES
        );
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload).await?;
    let value = serde_json::from_slice(&payload)?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::{
        CancelMsg, DownstreamMsg, EscalationMsg, HelloMsg, UpstreamMsg, VerdictDecision, VerdictMsg,
    };
    use crate::function::BlastRadius;

    #[tokio::test]
    async fn frame_round_trips_over_a_duplex_pipe() {
        // Use an in-memory duplex stream as a stand-in for the TLS stream.
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);

        let sent = UpstreamMsg::Hello(HelloMsg {
            agent_id: "child-1".into(),
            depth: 2,
            capabilities: vec![],
        });
        write_frame(&mut a, &sent).await.unwrap();

        let got: Option<UpstreamMsg> = read_frame(&mut b).await.unwrap();
        assert_eq!(got, Some(sent));
    }

    #[tokio::test]
    async fn multiple_frames_are_delimited_correctly() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);

        let m1 = DownstreamMsg::Verdict(VerdictMsg {
            escalation_id: "e1".into(),
            decision: VerdictDecision::Continue,
            added_context: None,
            token: None,
            risk_verdict: None,
        });
        let m2 = DownstreamMsg::Cancel(CancelMsg { reason: "stop".into() });
        write_frame(&mut a, &m1).await.unwrap();
        write_frame(&mut a, &m2).await.unwrap();

        let g1: Option<DownstreamMsg> = read_frame(&mut b).await.unwrap();
        let g2: Option<DownstreamMsg> = read_frame(&mut b).await.unwrap();
        assert_eq!(g1, Some(m1));
        assert_eq!(g2, Some(m2));
    }

    #[tokio::test]
    async fn clean_eof_at_boundary_returns_none() {
        let (a, mut b) = tokio::io::duplex(1024);
        drop(a); // peer closes with nothing pending
        let got: Option<UpstreamMsg> = read_frame(&mut b).await.unwrap();
        assert!(got.is_none(), "clean EOF at a frame boundary is None, not an error");
    }

    #[tokio::test]
    async fn escalation_frame_survives_the_wire() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let esc = UpstreamMsg::Escalation(EscalationMsg {
            id: "e1".into(),
            agent_id: "a".into(),
            tree_id: "t".into(),
            action: serde_json::json!({"tool": "fs_rm", "args": {"path": "/x"}}),
            reason: "destructive".into(),
            enrichment: serde_json::json!({"note": "n"}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "nonce".into(),
        });
        write_frame(&mut a, &esc).await.unwrap();
        let got: Option<UpstreamMsg> = read_frame(&mut b).await.unwrap();
        assert_eq!(got, Some(esc));
    }

    // --- #6d auth core (6d.2) ---

    #[test]
    fn tree_identity_has_stable_self_consistent_fingerprint() {
        let id = generate_tree_identity().unwrap();
        assert!(!id.cert_der.is_empty());
        assert!(!id.key_der.is_empty());
        // Fingerprint is SHA-256 hex (64 chars) of the cert DER.
        assert_eq!(id.fingerprint.len(), 64);
        assert_eq!(id.fingerprint, crate::utils::sha256_bytes(&id.cert_der));
        // Two identities differ (fresh keypair each time).
        let id2 = generate_tree_identity().unwrap();
        assert_ne!(id.fingerprint, id2.fingerprint);
    }

    #[test]
    fn channel_credential_verifies_and_is_channel_bound() {
        let secret = "tree-secret-xyz";
        let nonce = "nonce-123";
        let fp = "aabbcc";
        let cred = compute_channel_credential(secret, nonce, fp);

        // Correct secret + nonce + fingerprint verifies.
        assert!(verify_channel_credential(secret, nonce, fp, &cred));

        // Wrong secret fails.
        assert!(!verify_channel_credential("other-secret", nonce, fp, &cred));
        // Wrong nonce fails (per-connection binding).
        assert!(!verify_channel_credential(secret, "different-nonce", fp, &cred));
        // Wrong fingerprint fails (channel binding — replay against another parent
        // with a different cert is rejected).
        assert!(!verify_channel_credential(secret, nonce, "ddeeff", &cred));
        // Garbage credential fails.
        assert!(!verify_channel_credential(secret, nonce, fp, "deadbeef"));
        assert!(!verify_channel_credential(secret, nonce, fp, ""));
    }

    #[test]
    fn constant_time_eq_matches_semantics_of_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn fingerprint_pin_verifier_rejects_mismatch_fails_closed() {
        use rustls::client::danger::ServerCertVerifier;
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

        let id = generate_tree_identity().unwrap();
        let cert = CertificateDer::from(id.cert_der.clone());
        let server_name = ServerName::try_from(AGENT_SERVER_NAME).unwrap();
        let now = UnixTime::now();

        // Correct pin → accepted.
        let ok_verifier = FingerprintPinVerifier::new(id.fingerprint.clone());
        assert!(ok_verifier
            .verify_server_cert(&cert, &[], &server_name, &[], now)
            .is_ok());

        // Wrong pin → rejected (fail-closed), NOT accepted.
        let bad_verifier = FingerprintPinVerifier::new("0".repeat(64));
        assert!(bad_verifier
            .verify_server_cert(&cert, &[], &server_name, &[], now)
            .is_err());
    }

    // --- #6d end-to-end mTLS handshake (6d.1 + 6d.2) ---

    async fn spawn_parent() -> (Arc<ParentListener>, Vec<(String, String)>) {
        let identity = Arc::new(generate_tree_identity().unwrap());
        let listener = ParentListener::bind(
            identity,
            "tree-1".to_string(),
            "shared-tree-secret".to_string(),
        )
        .await
        .unwrap();
        let env = listener.child_env().unwrap();
        (Arc::new(listener), env)
    }

    fn conn_info_from(env: &[(String, String)]) -> ParentConnInfo {
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        ParentConnInfo {
            addr: get("AICHAT_AGENT_PARENT_ADDR"),
            fingerprint: get("AICHAT_AGENT_PARENT_FP"),
            tree_secret: get("AICHAT_TREE_SECRET"),
            tree_id: get("AICHAT_TREE_ID"),
        }
    }

    #[tokio::test]
    async fn mutual_tls_handshake_authenticates_a_legit_child() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let accept = {
            let listener = listener.clone();
            tokio::spawn(async move { listener.accept_authenticated().await })
        };
        let dial = tokio::spawn(async move { dial_parent(&info, "child-1", 1).await });

        let (_child_transport, dial_res) = (accept.await.unwrap(), dial.await.unwrap());
        let (_parent_transport, hello) = _child_transport.expect("parent should authenticate child");
        assert!(dial_res.is_ok(), "child should connect");
        assert_eq!(hello.agent_id, "child-1");
        assert_eq!(hello.depth, 1);
    }

    #[tokio::test]
    async fn wrong_fingerprint_is_rejected_by_the_child() {
        let (listener, env) = spawn_parent().await;
        let mut info = conn_info_from(&env);
        // Tamper the pinned fingerprint → the child's TLS verifier must reject the
        // parent's cert (fail closed), aborting the handshake.
        info.fingerprint = "0".repeat(64);

        let accept = {
            let listener = listener.clone();
            tokio::spawn(async move { listener.accept_authenticated().await })
        };
        let dial = tokio::spawn(async move { dial_parent(&info, "child-1", 1).await });

        let dial_res = dial.await.unwrap();
        assert!(
            dial_res.is_err(),
            "child must reject a parent whose cert fingerprint does not match the pin"
        );
        // Parent side also fails (handshake never completes).
        let _ = accept.await.unwrap();
    }

    #[tokio::test]
    async fn wrong_tree_secret_is_rejected_by_the_parent() {
        let (listener, env) = spawn_parent().await;
        let mut info = conn_info_from(&env);
        // Child pins the RIGHT fingerprint (TLS succeeds) but holds the WRONG
        // secret → its channel-bound credential fails the parent's check.
        info.tree_secret = "attacker-secret".to_string();

        let accept = {
            let listener = listener.clone();
            tokio::spawn(async move { listener.accept_authenticated().await })
        };
        let dial = tokio::spawn(async move { dial_parent(&info, "child-1", 1).await });

        let parent_res = accept.await.unwrap();
        assert!(
            parent_res.is_err(),
            "parent must reject a child that fails the channel-bound credential check"
        );
        let _ = dial.await.unwrap();
    }

    #[tokio::test]
    async fn escalation_transaction_receives_verdict() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let msg = transport.recv_upstream().await.unwrap();
            match msg {
                Some(UpstreamMsg::Escalation(esc)) => {
                    transport
                        .send_downstream(&DownstreamMsg::Verdict(VerdictMsg {
                            escalation_id: esc.id,
                            decision: VerdictDecision::Continue,
                            added_context: Some(serde_json::json!({"note": "approved"})),
                            token: None,
                            risk_verdict: None,
                        }))
                        .await
                        .unwrap();
                }
                other => panic!("expected Escalation, got {other:?}"),
            }
        });

        let esc = EscalationMsg {
            id: "esc-1".into(),
            agent_id: "child-1".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "fs_write"}),
            reason: "over-ceiling".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let verdict = escalate_to_parent(&info, "child-1", 1, esc, 10).await.unwrap();
        assert_eq!(verdict.escalation_id, "esc-1");
        assert_eq!(verdict.decision, VerdictDecision::Continue);
        assert_eq!(
            verdict.added_context,
            Some(serde_json::json!({"note": "approved"}))
        );

        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn escalation_transaction_timeout_fails_closed() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let _parent_task = tokio::spawn(async move {
            let (_transport, _hello) = listener.accept_authenticated().await.unwrap();
            // Sleep longer than timeout without sending verdict
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });

        let esc = EscalationMsg {
            id: "esc-timeout".into(),
            agent_id: "child-1".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "fs_write"}),
            reason: "over-ceiling".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let res = escalate_to_parent(&info, "child-1", 1, esc, 1).await;
        assert!(res.is_err(), "escalation must time out and fail closed");
        assert!(
            res.unwrap_err().to_string().contains("timed out awaiting verdict"),
            "error should indicate timeout fail-closed"
        );
    }

    #[tokio::test]
    async fn escalation_transaction_parent_eof_fails_closed() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            let (transport, _hello) = listener.accept_authenticated().await.unwrap();
            // Drop transport immediately upon receiving connection
            drop(transport);
        });

        let esc = EscalationMsg {
            id: "esc-eof".into(),
            agent_id: "child-1".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "fs_write"}),
            reason: "over-ceiling".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let res = escalate_to_parent(&info, "child-1", 1, esc, 5).await;
        assert!(res.is_err(), "parent closing connection must fail closed");
        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn escalation_transaction_parent_cancel_fails_closed() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let msg = transport.recv_upstream().await.unwrap().unwrap();
            if let UpstreamMsg::Escalation(_) = msg {
                transport
                    .send_downstream(&DownstreamMsg::Cancel(crate::safety::CancelMsg {
                        reason: "branch tear down".into(),
                    }))
                    .await
                    .unwrap();
            }
        });

        let esc = EscalationMsg {
            id: "esc-cancel".into(),
            agent_id: "child-1".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "fs_write"}),
            reason: "over-ceiling".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let res = escalate_to_parent(&info, "child-1", 1, esc, 5).await;
        assert!(res.is_err(), "parent cancel must fail closed");
        assert!(
            res.unwrap_err().to_string().contains("parent cancelled execution: branch tear down"),
            "error should contain cancel reason"
        );
        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn notify_parent_event_and_results_round_trip() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        assert!(!listener.fingerprint().is_empty());

        let parent_task = tokio::spawn(async move {
            // 1. Accept event
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let msg1 = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(msg1, UpstreamMsg::Event(serde_json::json!({"turn": 1})));

            // 2. Accept success result
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let msg2 = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(
                msg2,
                UpstreamMsg::Result(crate::safety::ResultMsg {
                    output: serde_json::json!({"status": "ok"}),
                    cost: 0.05,
                })
            );

            // 3. Accept error result
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let msg3 = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(
                msg3,
                UpstreamMsg::Error(crate::safety::ErrorMsg {
                    message: "failed step".into(),
                })
            );
        });

        notify_parent_event(&info, "child-1", 1, serde_json::json!({"turn": 1}))
            .await
            .unwrap();

        notify_parent_result(
            &info,
            "child-1",
            1,
            Ok(serde_json::json!({"status": "ok"})),
            0.05,
        )
        .await
        .unwrap();

        notify_parent_result(&info, "child-1", 1, Err("failed step".into()), 0.0)
            .await
            .unwrap();

        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_persistent_multiplexed_client_lifecycle() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            // SINGLE accept for the entire child lifetime
            let (mut transport, hello) = listener.accept_authenticated().await.unwrap();
            assert_eq!(hello.agent_id, "child-persist");
            assert_eq!(hello.depth, 1);

            // 1. Receive 2 Events
            let ev1 = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(ev1, UpstreamMsg::Event(serde_json::json!({"step": 1})));
            let ev2 = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(ev2, UpstreamMsg::Event(serde_json::json!({"step": 2})));

            // 2. Receive Escalation and reply with Verdict
            let esc_msg = transport.recv_upstream().await.unwrap().unwrap();
            if let UpstreamMsg::Escalation(esc) = esc_msg {
                transport
                    .send_downstream(&DownstreamMsg::Verdict(VerdictMsg {
                        escalation_id: esc.id,
                        decision: VerdictDecision::Continue,
                        added_context: Some(serde_json::json!({"ok": true})),
                        token: None,
                        risk_verdict: None,
                    }))
                    .await
                    .unwrap();
            } else {
                panic!("expected Escalation, got {esc_msg:?}");
            }

            // 3. Receive Terminal Result
            let res_msg = transport.recv_upstream().await.unwrap().unwrap();
            assert_eq!(
                res_msg,
                UpstreamMsg::Result(crate::safety::ResultMsg {
                    output: serde_json::json!({"summary": "done"}),
                    cost: 0.12,
                })
            );
        });

        let client = ChildEscalationClient::connect(&info, "child-persist", 1)
            .await
            .unwrap();
        assert!(client.is_connected());

        // Emit events
        client.send_event(serde_json::json!({"step": 1}));
        client.send_event(serde_json::json!({"step": 2}));

        // Escalate
        let esc = EscalationMsg {
            id: "esc-persist-1".into(),
            agent_id: "child-persist".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "bash", "args": {"cmd": "ls"}}),
            reason: "over-ceiling".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let verdict = client.escalate(esc, 5).await.unwrap();
        assert_eq!(verdict.escalation_id, "esc-persist-1");
        assert_eq!(verdict.decision, VerdictDecision::Continue);

        // Send Result
        client
            .send_result(Ok(serde_json::json!({"summary": "done"})), 0.12)
            .await
            .unwrap();

        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_persistent_client_concurrent_escalations() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            let (mut transport, _hello) = listener.accept_authenticated().await.unwrap();
            let mut received = Vec::new();
            for _ in 0..2 {
                let msg = transport.recv_upstream().await.unwrap().unwrap();
                if let UpstreamMsg::Escalation(esc) = msg {
                    received.push(esc);
                }
            }

            // Reply in reverse order to ensure demux by escalation_id works
            for esc in received.into_iter().rev() {
                transport
                    .send_downstream(&DownstreamMsg::Verdict(VerdictMsg {
                        escalation_id: esc.id.clone(),
                        decision: VerdictDecision::Continue,
                        added_context: Some(serde_json::json!({"id": esc.id})),
                        token: None,
                        risk_verdict: None,
                    }))
                    .await
                    .unwrap();
            }
        });

        let client = ChildEscalationClient::connect(&info, "child-conc", 1)
            .await
            .unwrap();

        let esc1 = EscalationMsg {
            id: "esc-c-1".into(),
            agent_id: "child-conc".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "t1"}),
            reason: "r1".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c1".into(),
        };

        let esc2 = EscalationMsg {
            id: "esc-c-2".into(),
            agent_id: "child-conc".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "t2"}),
            reason: "r2".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c2".into(),
        };

        let c1 = client.clone();
        let c2 = client.clone();
        let t1 = tokio::spawn(async move { c1.escalate(esc1, 5).await.unwrap() });
        let t2 = tokio::spawn(async move { c2.escalate(esc2, 5).await.unwrap() });

        let (v1, v2) = (t1.await.unwrap(), t2.await.unwrap());
        assert_eq!(v1.escalation_id, "esc-c-1");
        assert_eq!(v2.escalation_id, "esc-c-2");

        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_persistent_client_socket_drop_immediate_fail_closed() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let parent_task = tokio::spawn(async move {
            let (transport, _hello) = listener.accept_authenticated().await.unwrap();
            // Wait briefly then close connection abruptly
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            drop(transport);
        });

        let client = ChildEscalationClient::connect(&info, "child-drop", 1)
            .await
            .unwrap();

        let esc = EscalationMsg {
            id: "esc-drop".into(),
            agent_id: "child-drop".into(),
            tree_id: "tree-1".into(),
            action: serde_json::json!({"tool": "t"}),
            reason: "r".into(),
            enrichment: serde_json::json!({}),
            blast_radius: BlastRadius::Destructive,
            reversible: false,
            challenge: "c".into(),
        };

        let res = client.escalate(esc, 30).await;
        assert!(res.is_err(), "dropped socket must immediately fail closed");
        assert!(
            res.unwrap_err().to_string().contains("fail-closed"),
            "error should indicate fail-closed drop"
        );
        parent_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_event_backpressure_drop_newest() {
        let (listener, env) = spawn_parent().await;
        let info = conn_info_from(&env);

        let _parent_task = tokio::spawn(async move {
            let (_transport, _hello) = listener.accept_authenticated().await.unwrap();
            // Don't drain events immediately
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });

        let client = ChildEscalationClient::connect(&info, "child-burst", 1)
            .await
            .unwrap();

        // Burst 2000 events without blocking
        for i in 0..2000 {
            client.send_event(serde_json::json!({"seq": i}));
        }

        // Client is still active and doesn't panic or block
        assert!(client.is_connected());
    }
}
