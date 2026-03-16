use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::ephemeral::EphemeralMessage;
use crate::network::{NetworkMode, SharedConnectionState};
use crate::operations::CommittedOp;
use crate::protocol::{ClientMessage, ServerMessage};
use crate::user::User;

pub fn spawn_ws_client(
    url: String,
    op_tx: mpsc::UnboundedSender<CommittedOp>,
    mut op_rx: mpsc::UnboundedReceiver<CommittedOp>,
    eph_tx: mpsc::UnboundedSender<EphemeralMessage>,
    mut eph_rx: mpsc::UnboundedReceiver<EphemeralMessage>,
    welcome_tx: tokio::sync::oneshot::Sender<User>,
    connection_state: SharedConnectionState,
    rt: &tokio::runtime::Runtime,
) -> JoinHandle<()> {
    rt.spawn(async move {
        let (ws_stream, _) = match tokio_tungstenite::connect_async(&url).await {
            Ok(pair) => pair,
            Err(e) => {
                log::error!("WebSocket connect failed: {}", e);
                connection_state.set(NetworkMode::Disconnected);
                return;
            }
        };
        log::info!("Connected to relay server at {}", url);

        let (mut ws_sink, mut ws_source) = ws_stream.split();
        let mut welcome_tx = Some(welcome_tx);

        // Outbound task: local ops/ephemeral → server
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<String>();
        let outbound_tx2 = outbound_tx.clone();

        // Spawn outbound forwarder for ops
        let op_fwd = tokio::spawn({
            let outbound_tx = outbound_tx.clone();
            async move {
                while let Some(op) = op_rx.recv().await {
                    log::info!("[SYNC] ws_client outbound: {} (seq={}, user={})", op.op.variant_name(), op.seq, op.user_id);
                    let msg = ClientMessage::Op(op);
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            log::info!("[SYNC] ws_client serialized {} bytes", json.len());
                            let _ = outbound_tx.send(json);
                        }
                        Err(e) => log::warn!("[SYNC] ws_client FAILED to serialize op: {}", e),
                    }
                }
            }
        });

        // Spawn outbound forwarder for ephemeral
        let eph_fwd = tokio::spawn(async move {
            while let Some(eph) = eph_rx.recv().await {
                let msg = ClientMessage::Ephemeral(eph);
                match serde_json::to_string(&msg) {
                    Ok(json) => { let _ = outbound_tx2.send(json); }
                    Err(e) => log::warn!("Failed to serialize ephemeral: {}", e),
                }
            }
        });

        // Main loop: read from WS and outbound channel
        loop {
            tokio::select! {
                // Send outbound messages to WS
                Some(json) = outbound_rx.recv() => {
                    use tokio_tungstenite::tungstenite::Message;
                    if ws_sink.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                // Receive messages from WS
                msg = ws_source.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            match serde_json::from_str::<ServerMessage>(&text) {
                                Ok(server_msg) => match server_msg {
                                    ServerMessage::Welcome { user, room_id } => {
                                        log::info!("Joined room {} as {}", room_id, user.name);
                                        connection_state.set(NetworkMode::Connected);
                                        if let Some(tx) = welcome_tx.take() {
                                            let _ = tx.send(user);
                                        }
                                    }
                                    ServerMessage::RemoteOp(committed) => {
                                        log::info!("[SYNC] ws_client inbound: {} (seq={}, user={})", committed.op.variant_name(), committed.seq, committed.user_id);
                                        let _ = op_tx.send(committed);
                                    }
                                    ServerMessage::RemoteEphemeral(eph) => {
                                        let _ = eph_tx.send(eph);
                                    }
                                    ServerMessage::StateSnapshot { ops } => {
                                        for op in ops {
                                            let _ = op_tx.send(op);
                                        }
                                    }
                                    ServerMessage::UserJoined(user) => {
                                        let _ = eph_tx.send(EphemeralMessage::UserJoined { user });
                                    }
                                    ServerMessage::UserLeft(user_id) => {
                                        let _ = eph_tx.send(EphemeralMessage::UserLeft { user_id });
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse server message: {}", e);
                                }
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                            log::info!("WebSocket closed");
                            break;
                        }
                        Some(Err(e)) => {
                            log::error!("WebSocket error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        connection_state.set(NetworkMode::Disconnected);
        op_fwd.abort();
        eph_fwd.abort();
    })
}
