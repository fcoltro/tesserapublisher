use crate::components::*;
use bevy_ecs::prelude::*;
use im::Vector;
use serde::{Deserialize, Serialize};

/// Snapshot of an entity's complete frame data for reversible history
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySnapshotData {
    pub entity_index: u32,
    pub frame: Frame,
    pub transform: Transform,
    pub size: Size,
    pub z_index: ZIndex,
    pub bounding_box: BoundingBox,
    pub style: Style,
    pub parent: Option<u32>,
    pub text_content: Option<TextContent>,
}

/// An atomic state delta action stored in the immutable history stack
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HistoryAction {
    SpawnFrame(EntitySnapshotData),
    DespawnFrame(EntitySnapshotData),
    UpdateTransform {
        entity_index: u32,
        old_transform: Transform,
        new_transform: Transform,
        old_bounding_box: BoundingBox,
        new_bounding_box: BoundingBox,
    },
    UpdateSize {
        entity_index: u32,
        old_size: Size,
        new_size: Size,
        old_bounding_box: BoundingBox,
        new_bounding_box: BoundingBox,
    },
    UpdateStyle {
        entity_index: u32,
        old_style: Style,
        new_style: Style,
    },
}

/// Immutable Undo/Redo stack powered by im::Vector
#[derive(Debug, Clone, Default)]
pub struct HistoryStack {
    pub undo_stack: Vector<HistoryAction>,
    pub redo_stack: Vector<HistoryAction>,
    pub max_depth: usize,
}

impl HistoryStack {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vector::new(),
            redo_stack: Vector::new(),
            max_depth,
        }
    }

    /// Push a new action to the undo stack and clear the redo stack
    pub fn push(&mut self, action: HistoryAction) {
        self.undo_stack.push_back(action);
        if self.undo_stack.len() > self.max_depth && self.max_depth > 0 {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn depth(&self) -> (usize, usize) {
        (self.undo_stack.len(), self.redo_stack.len())
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Undoes the last action, applying inverse changes to the Bevy ECS World
    pub fn undo(&mut self, world: &mut World) -> Option<HistoryAction> {
        let action = self.undo_stack.pop_back()?;
        self.apply_inverse_action(&action, world);
        self.redo_stack.push_back(action.clone());
        Some(action)
    }

    /// Redoes the next action, re-applying changes to the Bevy ECS World
    pub fn redo(&mut self, world: &mut World) -> Option<HistoryAction> {
        let action = self.redo_stack.pop_back()?;
        self.apply_forward_action(&action, world);
        self.undo_stack.push_back(action.clone());
        Some(action)
    }

    fn apply_inverse_action(&self, action: &HistoryAction, world: &mut World) {
        match action {
            HistoryAction::SpawnFrame(data) => {
                // Inverse of spawn is despawn
                let entity = Entity::from_raw(data.entity_index);
                if let Ok(entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.despawn();
                }
            }
            HistoryAction::DespawnFrame(data) => {
                // Inverse of despawn is re-spawn
                let mut entity_cmd = world.spawn((
                    data.frame.clone(),
                    data.transform,
                    data.size,
                    data.z_index,
                    data.bounding_box,
                    data.style.clone(),
                ));
                if let Some(parent_idx) = data.parent {
                    entity_cmd.insert(BelongsTo(Entity::from_raw(parent_idx)));
                }
                if let Some(text) = &data.text_content {
                    entity_cmd.insert(text.clone());
                }
            }
            HistoryAction::UpdateTransform {
                entity_index,
                old_transform,
                old_bounding_box,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut t) = mut_entity.get_mut::<Transform>() {
                        *t = *old_transform;
                    }
                    if let Some(mut bb) = mut_entity.get_mut::<BoundingBox>() {
                        *bb = *old_bounding_box;
                    }
                }
            }
            HistoryAction::UpdateSize {
                entity_index,
                old_size,
                old_bounding_box,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut s) = mut_entity.get_mut::<Size>() {
                        *s = *old_size;
                    }
                    if let Some(mut bb) = mut_entity.get_mut::<BoundingBox>() {
                        *bb = *old_bounding_box;
                    }
                }
            }
            HistoryAction::UpdateStyle {
                entity_index,
                old_style,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut s) = mut_entity.get_mut::<Style>() {
                        *s = old_style.clone();
                    }
                }
            }
        }
    }

    fn apply_forward_action(&self, action: &HistoryAction, world: &mut World) {
        match action {
            HistoryAction::SpawnFrame(data) => {
                let mut entity_cmd = world.spawn((
                    data.frame.clone(),
                    data.transform,
                    data.size,
                    data.z_index,
                    data.bounding_box,
                    data.style.clone(),
                ));
                if let Some(parent_idx) = data.parent {
                    entity_cmd.insert(BelongsTo(Entity::from_raw(parent_idx)));
                }
                if let Some(text) = &data.text_content {
                    entity_cmd.insert(text.clone());
                }
            }
            HistoryAction::DespawnFrame(data) => {
                let entity = Entity::from_raw(data.entity_index);
                if let Ok(entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.despawn();
                }
            }
            HistoryAction::UpdateTransform {
                entity_index,
                new_transform,
                new_bounding_box,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut t) = mut_entity.get_mut::<Transform>() {
                        *t = *new_transform;
                    }
                    if let Some(mut bb) = mut_entity.get_mut::<BoundingBox>() {
                        *bb = *new_bounding_box;
                    }
                }
            }
            HistoryAction::UpdateSize {
                entity_index,
                new_size,
                new_bounding_box,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut s) = mut_entity.get_mut::<Size>() {
                        *s = *new_size;
                    }
                    if let Some(mut bb) = mut_entity.get_mut::<BoundingBox>() {
                        *bb = *new_bounding_box;
                    }
                }
            }
            HistoryAction::UpdateStyle {
                entity_index,
                new_style,
                ..
            } => {
                let entity = Entity::from_raw(*entity_index);
                if let Ok(mut mut_entity) = world.get_entity_mut(entity) {
                    if let Some(mut s) = mut_entity.get_mut::<Style>() {
                        *s = new_style.clone();
                    }
                }
            }
        }
    }
}
