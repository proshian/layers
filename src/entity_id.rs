pub type EntityId = uuid::Uuid;

pub fn new_id() -> EntityId {
    uuid::Uuid::new_v4()
}
