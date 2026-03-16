use crate::user::{DragPreview, User, UserId};

/// Ephemeral messages that are NOT persisted — used for cursor sync, presence, etc.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EphemeralMessage {
    CursorMove {
        user_id: UserId,
        position: [f32; 2],
    },
    DragUpdate {
        user_id: UserId,
        preview: DragPreview,
    },
    DragEnd {
        user_id: UserId,
    },
    UserJoined {
        user: User,
    },
    UserLeft {
        user_id: UserId,
    },
}
