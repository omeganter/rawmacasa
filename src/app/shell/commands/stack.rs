//! Stacks by command: Group into Stack for the photos named, as the Library's
//! own command makes it ([`crate::app::library::Library::group_into_stacks`]),
//! so an import can stack what belongs together (an original and its edited
//! version, a burst). Several stacks are one undoable step, all or none.
//! Files on disk are never touched.
use super::reply::Stacked;
use super::{Error, Outcome, Result};
use crate::app::Editor;
use crate::catalog::PhotoId;
use std::collections::{HashMap, HashSet};

impl Editor {
    /// Makes each of `stacks` a collapsed stack with its first photo on top.
    /// Photos already in a stack leave it, as Group into Stack has it.
    pub(super) fn command_stack(&mut self, stacks: Vec<Vec<PhotoId>>) -> Result<Outcome> {
        if stacks.is_empty() || stacks.iter().any(|s| s.len() < 2) {
            return Err(Error::new(
                "invalid_request",
                "A stack takes two photos or more",
            ));
        }
        let named: Vec<PhotoId> = stacks.concat();
        let photos = self.catalog_change_allowed(named.clone())?;
        if photos.len() != named.len() {
            let mut seen = HashSet::new();
            let twice = named.iter().find(|p| !seen.insert(**p)).expect("one is");
            return Err(Error::new(
                "invalid_request",
                format!(
                    "A photo can be in one stack only: {} is named twice",
                    twice.0
                ),
            ));
        }
        let library = self.library.as_mut().expect("checked above");
        // The catalog refuses a stack across folders too; this says which.
        let folders: HashMap<PhotoId, _> = library
            .session
            .photos()
            .iter()
            .map(|p| (p.id, p.folder))
            .collect();
        for stack in &stacks {
            if let Some(other) = stack.iter().find(|p| folders[p] != folders[&stack[0]]) {
                return Err(Error::new(
                    "invalid_request",
                    format!(
                        "Only photos of one folder can be stacked: {} and {} are in different folders",
                        stack[0].0, other.0
                    ),
                ));
            }
        }
        let made = stacks.len();
        library
            .group_into_stacks(stacks)
            .map_err(|e| Error::new("not_applied", format!("{e:#}")))?;
        Ok(Outcome::Stacked {
            stacked: Stacked {
                stacks: made,
                photos: named.len(),
            },
        })
    }
}
