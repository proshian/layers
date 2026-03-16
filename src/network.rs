use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::sync::mpsc;

use crate::ephemeral::EphemeralMessage;
use crate::operations::CommittedOp;

/// Network mode — offline (local-only) or connected to a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NetworkMode {
    Offline = 0,
    Connecting = 1,
    Connected = 2,
    Disconnected = 3,
}

impl NetworkMode {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Connecting,
            2 => Self::Connected,
            3 => Self::Disconnected,
            _ => Self::Offline,
        }
    }
}

/// Shared connection state that can be updated from the async ws_client task.
#[derive(Clone)]
pub struct SharedConnectionState(pub Arc<AtomicU8>);

impl SharedConnectionState {
    pub fn new(mode: NetworkMode) -> Self {
        Self(Arc::new(AtomicU8::new(mode as u8)))
    }

    pub fn get(&self) -> NetworkMode {
        NetworkMode::from_u8(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, mode: NetworkMode) {
        self.0.store(mode as u8, Ordering::Relaxed);
    }
}

/// Manages the network connection for realtime collaboration.
/// In Offline mode, all methods are no-ops.
/// In Connected mode, operations and ephemeral messages are sent/received
/// via channels that interface with the network transport.
pub struct NetworkManager {
    pub connection_state: SharedConnectionState,
    // Outbound: local ops to send to server
    op_sender: Option<mpsc::UnboundedSender<CommittedOp>>,
    // Inbound: remote ops from server
    op_receiver: Option<mpsc::UnboundedReceiver<CommittedOp>>,
    // Outbound: local ephemeral messages (cursors, drag previews)
    ephemeral_sender: Option<mpsc::UnboundedSender<EphemeralMessage>>,
    // Inbound: remote ephemeral messages
    ephemeral_receiver: Option<mpsc::UnboundedReceiver<EphemeralMessage>>,
}

impl NetworkManager {
    /// Create a new offline (disconnected) network manager.
    pub fn new_offline() -> Self {
        Self {
            connection_state: SharedConnectionState::new(NetworkMode::Offline),
            op_sender: None,
            op_receiver: None,
            ephemeral_sender: None,
            ephemeral_receiver: None,
        }
    }

    /// Create a connected network manager with channels.
    /// Returns (manager, remote_op_tx, remote_op_rx, remote_eph_tx, remote_eph_rx)
    /// where the "remote" endpoints are for the WebSocket bridge to use.
    pub fn new_connected() -> (
        Self,
        mpsc::UnboundedSender<CommittedOp>,
        mpsc::UnboundedReceiver<CommittedOp>,
        mpsc::UnboundedSender<EphemeralMessage>,
        mpsc::UnboundedReceiver<EphemeralMessage>,
    ) {
        let (local_op_tx, remote_op_rx) = mpsc::unbounded_channel();
        let (remote_op_tx, local_op_rx) = mpsc::unbounded_channel();
        let (local_eph_tx, remote_eph_rx) = mpsc::unbounded_channel();
        let (remote_eph_tx, local_eph_rx) = mpsc::unbounded_channel();

        let mgr = Self {
            connection_state: SharedConnectionState::new(NetworkMode::Connecting),
            op_sender: Some(local_op_tx),
            op_receiver: Some(local_op_rx),
            ephemeral_sender: Some(local_eph_tx),
            ephemeral_receiver: Some(local_eph_rx),
        };

        (mgr, remote_op_tx, remote_op_rx, remote_eph_tx, remote_eph_rx)
    }

    pub fn mode(&self) -> NetworkMode {
        self.connection_state.get()
    }

    /// Send a committed operation to the server.
    pub fn send_op(&self, op: CommittedOp) {
        if let Some(tx) = &self.op_sender {
            log::info!("[SYNC] network.send_op: {} (seq={}, connected={})", op.op.variant_name(), op.seq, self.is_connected());
            if let Err(e) = tx.send(op) {
                log::warn!("[SYNC] FAILED to send op to channel: {}", e);
            }
        } else {
            log::debug!("[SYNC] network.send_op: no sender (offline mode)");
        }
    }

    /// Send an ephemeral message (cursor move, etc.) to the server.
    pub fn send_ephemeral(&self, msg: EphemeralMessage) {
        if let Some(tx) = &self.ephemeral_sender {
            if let Err(e) = tx.send(msg) {
                log::warn!("Failed to send ephemeral to network: {}", e);
            }
        }
    }

    /// Poll for incoming remote operations (non-blocking).
    pub fn poll_ops(&mut self) -> Vec<CommittedOp> {
        let mut ops = Vec::new();
        if let Some(rx) = &mut self.op_receiver {
            while let Ok(op) = rx.try_recv() {
                ops.push(op);
            }
        }
        ops
    }

    /// Poll for incoming ephemeral messages (non-blocking).
    pub fn poll_ephemeral(&mut self) -> Vec<EphemeralMessage> {
        let mut msgs = Vec::new();
        if let Some(rx) = &mut self.ephemeral_receiver {
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
        }
        msgs
    }

    pub fn is_connected(&self) -> bool {
        self.mode() == NetworkMode::Connected
    }
}
