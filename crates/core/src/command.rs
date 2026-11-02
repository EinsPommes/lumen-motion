//! Undo/redo via the command pattern.
//!
//! Every mutation of a [`Project`] goes through a [`Command`] executed by a
//! [`History`]. Continuous edits (slider drags, colour pickers) are merged
//! into one undo step until the history is [sealed](History::seal), which the
//! UI does when the pointer is released.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;

use thiserror::Error;

use crate::project::{Project, ProjectError, ProjectSettings};
use crate::scene::SceneError;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Scene(#[from] SceneError),
    #[error("{0}")]
    Invalid(String),
}

/// A reversible edit of the project.
pub trait Command: Any + fmt::Debug + Send {
    /// Human-readable label for menus ("Undo: …").
    fn label(&self) -> String;

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError>;

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError>;

    /// Tries to absorb `next` (already applied) into `self`. Return `true`
    /// if merged; `next` is then discarded.
    fn merge(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any;
}

/// Undo/redo stacks.
#[derive(Debug)]
pub struct History {
    undo: VecDeque<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    limit: usize,
    sealed: bool,
}

impl Default for History {
    fn default() -> Self {
        Self::new(500)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            sealed: true,
        }
    }

    /// Applies `cmd` and records it. On error nothing is recorded.
    pub fn execute(
        &mut self,
        project: &mut Project,
        mut cmd: Box<dyn Command>,
    ) -> Result<(), CommandError> {
        cmd.apply(project)?;
        self.redo.clear();
        if !self.sealed {
            if let Some(last) = self.undo.back_mut() {
                if last.merge(cmd.as_ref()) {
                    return Ok(());
                }
            }
        }
        self.undo.push_back(cmd);
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.sealed = false;
        Ok(())
    }

    /// Ends the current gesture; the next command starts a new undo step.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn undo(&mut self, project: &mut Project) -> Result<bool, CommandError> {
        self.sealed = true;
        let Some(mut cmd) = self.undo.pop_back() else {
            return Ok(false);
        };
        if let Err(e) = cmd.revert(project) {
            self.undo.push_back(cmd);
            return Err(e);
        }
        self.redo.push(cmd);
        Ok(true)
    }

    pub fn redo(&mut self, project: &mut Project) -> Result<bool, CommandError> {
        self.sealed = true;
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(false);
        };
        if let Err(e) = cmd.apply(project) {
            self.redo.push(cmd);
            return Err(e);
        }
        self.undo.push_back(cmd);
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<String> {
        self.undo.back().map(|c| c.label())
    }

    pub fn redo_label(&self) -> Option<String> {
        self.redo.last().map(|c| c.label())
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.sealed = true;
    }
}

// ---------------------------------------------------------------------------
// Concrete commands
// ---------------------------------------------------------------------------

/// Replaces the project settings.
#[derive(Debug, Clone)]
pub struct SetProjectSettings {
    before: ProjectSettings,
    after: ProjectSettings,
}

impl SetProjectSettings {
    pub fn new(before: ProjectSettings, after: ProjectSettings) -> Self {
        Self { before, after }
    }
}

impl Command for SetProjectSettings {
    fn label(&self) -> String {
        "Change project settings".to_owned()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        self.after.validate()?;
        project.settings = self.after.clone();
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project.settings = self.before.clone();
        Ok(())
    }

    fn merge(&mut self, next: &dyn Command) -> bool {
        match next.as_any().downcast_ref::<Self>() {
            Some(n) => {
                self.after = n.after.clone();
                true
            }
            None => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_width(p: &Project, w: u32) -> Box<dyn Command> {
        let mut after = p.settings.clone();
        after.width = w;
        Box::new(SetProjectSettings::new(p.settings.clone(), after))
    }

    fn run(h: &mut History, p: &mut Project, w: u32) -> Result<(), CommandError> {
        let cmd = set_width(p, w);
        h.execute(p, cmd)
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut p = Project::default();
        let mut h = History::default();
        run(&mut h, &mut p, 1280).unwrap();
        h.seal();
        run(&mut h, &mut p, 640).unwrap();
        assert_eq!(p.settings.width, 640);
        assert_eq!(h.undo_len(), 2);

        assert!(h.undo(&mut p).unwrap());
        assert_eq!(p.settings.width, 1280);
        assert!(h.undo(&mut p).unwrap());
        assert_eq!(p.settings.width, 1920);
        assert!(!h.undo(&mut p).unwrap());

        assert!(h.redo(&mut p).unwrap());
        assert_eq!(p.settings.width, 1280);
        assert!(h.can_redo());
    }

    #[test]
    fn new_command_clears_redo() {
        let mut p = Project::default();
        let mut h = History::default();
        run(&mut h, &mut p, 1280).unwrap();
        h.undo(&mut p).unwrap();
        assert!(h.can_redo());
        run(&mut h, &mut p, 800).unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn unsealed_commands_merge_into_one_step() {
        let mut p = Project::default();
        let mut h = History::default();
        for w in [1000, 1100, 1200] {
            run(&mut h, &mut p, w).unwrap();
        }
        assert_eq!(h.undo_len(), 1);
        h.undo(&mut p).unwrap();
        assert_eq!(p.settings.width, 1920, "merged step restores original");
    }

    #[test]
    fn sealed_commands_do_not_merge() {
        let mut p = Project::default();
        let mut h = History::default();
        run(&mut h, &mut p, 1000).unwrap();
        h.seal();
        run(&mut h, &mut p, 1100).unwrap();
        assert_eq!(h.undo_len(), 2);
    }

    #[test]
    fn failing_command_is_not_recorded() {
        let mut p = Project::default();
        let mut h = History::default();
        assert!(run(&mut h, &mut p, 0).is_err());
        assert_eq!(p.settings.width, 1920);
        assert!(!h.can_undo());
    }

    #[test]
    fn limit_drops_oldest() {
        let mut p = Project::default();
        let mut h = History::new(2);
        for w in [100, 200, 300] {
            run(&mut h, &mut p, w).unwrap();
            h.seal();
        }
        assert_eq!(h.undo_len(), 2);
        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        assert_eq!(p.settings.width, 100);
    }

    #[test]
    fn labels() {
        let mut p = Project::default();
        let mut h = History::default();
        assert_eq!(h.undo_label(), None);
        run(&mut h, &mut p, 100).unwrap();
        assert_eq!(h.undo_label().as_deref(), Some("Change project settings"));
    }
}
