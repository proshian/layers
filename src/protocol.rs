use serde::{Deserialize, Serialize};

use crate::ephemeral::EphemeralMessage;
use crate::operations::CommittedOp;
use crate::user::{User, UserId};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMessage {
    Op(CommittedOp),
    Ephemeral(EphemeralMessage),
    RequestSync,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMessage {
    Welcome { user: User, room_id: String },
    RemoteOp(CommittedOp),
    RemoteEphemeral(EphemeralMessage),
    StateSnapshot { ops: Vec<CommittedOp> },
    UserJoined(User),
    UserLeft(UserId),
}
