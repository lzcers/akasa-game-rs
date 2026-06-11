use std::collections::HashMap;

use bevy_ecs::{entity::Entity, resource::Resource};

#[derive(Resource, Default)]
// 用于把 Session ID 和内部的 Session Entity 关联
pub(crate) struct SessionRegistry {
    pub(crate) entities: HashMap<String, Entity>,
}
