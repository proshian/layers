use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, broadcast};
use tokio_tungstenite::tungstenite::Message;

const MAX_OP_LOG: usize = 10_000;

#[derive(Clone)]
struct UserInfo {
    id: String,
    name: String,
    color: [f32; 4],
}

const USER_COLORS: &[[f32; 4]] = &[
    [0.35, 0.78, 0.98, 1.0],
    [0.30, 0.85, 0.39, 1.0],
    [1.00, 0.58, 0.00, 1.0],
    [0.88, 0.25, 0.63, 1.0],
    [0.69, 0.32, 0.87, 1.0],
    [1.00, 0.84, 0.00, 1.0],
    [0.19, 0.84, 0.55, 1.0],
    [1.00, 0.24, 0.19, 1.0],
];

struct Room {
    /// Raw JSON strings of RemoteOp messages for replay
    op_log: VecDeque<String>,
    users: HashMap<String, UserInfo>,
    user_count: usize,
    broadcast_tx: broadcast::Sender<(String, String)>,
}

impl Room {
    fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(4096);
        Self {
            op_log: VecDeque::new(),
            users: HashMap::new(),
            user_count: 0,
            broadcast_tx,
        }
    }
}

type SharedRoom = Arc<Mutex<Room>>;

#[tokio::main]
async fn main() {
    env_logger::init();

    let port: u16 = std::env::args()
        .position(|a| a == "--port")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(8765);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    println!("Relay server listening on ws://{}", addr);

    let room: SharedRoom = Arc::new(Mutex::new(Room::new()));

    while let Ok((stream, peer)) = listener.accept().await {
        let room = room.clone();
        tokio::spawn(handle_connection(stream, peer, room));
    }
}

async fn handle_connection(stream: TcpStream, peer: SocketAddr, room: SharedRoom) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("WebSocket handshake failed for {}: {}", peer, e);
            return;
        }
    };

    let (mut ws_sink, mut ws_source) = ws_stream.split();

    // Assign user
    let (user, snapshot_ops, mut broadcast_rx) = {
        let mut room = room.lock().await;
        let idx = room.user_count;
        room.user_count += 1;
        let user_id = uuid::Uuid::new_v4().to_string();
        let user = UserInfo {
            id: user_id.clone(),
            name: format!("User {}", idx + 1),
            color: USER_COLORS[idx % USER_COLORS.len()],
        };
        room.users.insert(user_id, user.clone());
        let snapshot: Vec<String> = room.op_log.iter().cloned().collect();
        let rx = room.broadcast_tx.subscribe();
        (user, snapshot, rx)
    };

    let user_id = user.id.clone();
    println!("{} connected as {} ({})", peer, user.name, user_id);

    // Send Welcome
    let welcome_json = serde_json::json!({
        "Welcome": {
            "user": {
                "id": user.id,
                "name": user.name,
                "color": user.color,
            },
            "room_id": "default"
        }
    });
    let _ = ws_sink.send(Message::Text(welcome_json.to_string().into())).await;

    // Send state snapshot
    if !snapshot_ops.is_empty() {
        let ops: Vec<serde_json::Value> = snapshot_ops.iter()
            .filter_map(|json_str| {
                serde_json::from_str::<serde_json::Value>(json_str).ok()
                    .and_then(|v| v.get("RemoteOp").cloned())
            })
            .collect();
        let snap = serde_json::json!({
            "StateSnapshot": { "ops": ops }
        });
        let _ = ws_sink.send(Message::Text(snap.to_string().into())).await;
    }

    // Send existing users to the new client so it knows about everyone already connected
    {
        let room = room.lock().await;
        for existing in room.users.values() {
            if existing.id != user.id {
                let msg = serde_json::json!({
                    "UserJoined": {
                        "id": existing.id,
                        "name": existing.name,
                        "color": existing.color,
                    }
                });
                let _ = ws_sink.send(Message::Text(msg.to_string().into())).await;
            }
        }
        // Broadcast our own join to everyone else
        let msg = serde_json::json!({
            "UserJoined": {
                "id": user.id,
                "name": user.name,
                "color": user.color,
            }
        });
        let _ = room.broadcast_tx.send((user_id.clone(), msg.to_string()));
    }

    // Main loop
    loop {
        tokio::select! {
            msg = ws_source.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text_str: &str = &text;
                        // Parse once, branch on keys
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(text_str) {
                            if let Some(inner) = val.get("Op") {
                                // Log the operation type for debugging
                                let op_type = inner.get("op")
                                    .and_then(|o| o.as_object())
                                    .and_then(|o| o.keys().next())
                                    .map(|s| s.as_str())
                                    .unwrap_or("unknown");
                                let seq = inner.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
                                println!("[SYNC] relay received Op: {} (seq={}) from {}", op_type, seq, &user_id[..8]);
                                let remote_op = serde_json::json!({"RemoteOp": inner});
                                let json = remote_op.to_string();
                                let mut room = room.lock().await;
                                let num_subscribers = room.broadcast_tx.receiver_count();
                                room.op_log.push_back(json.clone());
                                if room.op_log.len() > MAX_OP_LOG {
                                    let excess = room.op_log.len() - MAX_OP_LOG;
                                    room.op_log.drain(..excess);
                                }
                                let send_result = room.broadcast_tx.send((user_id.clone(), json));
                                println!("[SYNC] relay broadcast: {} subscribers, result={:?}", num_subscribers, send_result.is_ok());
                            } else if let Some(inner) = val.get("Ephemeral") {
                                let remote_eph = serde_json::json!({"RemoteEphemeral": inner});
                                let json = remote_eph.to_string();
                                let room = room.lock().await;
                                let _ = room.broadcast_tx.send((user_id.clone(), json));
                            } else if val.get("RequestSync").is_some() {
                                let room = room.lock().await;
                                let ops: Vec<serde_json::Value> = room.op_log.iter()
                                    .filter_map(|s| {
                                        serde_json::from_str::<serde_json::Value>(s).ok()
                                            .and_then(|v| v.get("RemoteOp").cloned())
                                    })
                                    .collect();
                                let snap = serde_json::json!({
                                    "StateSnapshot": { "ops": ops }
                                });
                                let _ = ws_sink.send(Message::Text(snap.to_string().into())).await;
                            } else {
                                eprintln!("Unknown message from {}: {}", peer, &text[..text.len().min(100)]);
                            }
                        } else {
                            eprintln!("Invalid JSON from {}: {}", peer, &text[..text.len().min(100)]);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        eprintln!("WS error from {}: {}", peer, e);
                        break;
                    }
                    _ => {}
                }
            }
            Ok((sender_id, json)) = broadcast_rx.recv() => {
                if sender_id != user_id {
                    if ws_sink.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    // Cleanup
    {
        let mut room = room.lock().await;
        room.users.remove(&user_id);
        let msg = serde_json::json!({
            "UserLeft": user_id
        });
        let _ = room.broadcast_tx.send((user_id.clone(), msg.to_string()));
    }

    println!("{} disconnected", peer);
}
