//! The OSC 8 hyperlink table: interning, and dropping links once the table is full. Ids are never
//! reused, so a cell that held a dropped link has no link and never shows a different one.

use super::{Hyperlink, Terminal};
use std::collections::HashSet;

impl Terminal {
    /// The number of hyperlinks stored.
    pub fn hyperlink_count(&self) -> usize {
        self.hyperlinks.len()
    }

    /// Stores `link` and returns its id, or the id it already has. At `max_hyperlinks` links the
    /// table first drops what it can.
    pub(super) fn intern_hyperlink(&mut self, link: Hyperlink) -> u32 {
        if let Some(&id) = self.hyperlink_index.get(&link) {
            return id;
        }
        if self.hyperlinks.len() >= self.config.max_hyperlinks.max(1) {
            self.make_hyperlink_room();
        }
        let id = self.hyperlink_next;
        self.hyperlink_next = self.hyperlink_next.wrapping_add(1);
        self.hyperlinks.insert(id, link.clone());
        self.hyperlink_index.insert(link, id);
        id
    }

    /// Drops the links no cell refers to. When that leaves the table still full, also drops the
    /// oldest eighth of the limit, so the next sweep (which reads every page) is that many new
    /// links away.
    fn make_hyperlink_room(&mut self) {
        self.sweep_hyperlinks();
        let max = self.config.max_hyperlinks.max(1);
        if self.hyperlinks.len() < max {
            return;
        }
        let evict = (max / 8).max(1);
        let oldest: Vec<u32> = self.hyperlinks.keys().take(evict).copied().collect();
        for id in oldest {
            self.drop_hyperlink(id);
        }
    }

    /// Removes the hyperlink `id` from the table and the index.
    fn drop_hyperlink(&mut self, id: u32) {
        if let Some(link) = self.hyperlinks.remove(&id) {
            self.hyperlink_index.remove(&link);
        }
    }

    /// Drops every hyperlink that no cell on either screen and no cursor refers to.
    pub(super) fn sweep_hyperlinks(&mut self) {
        let mut used: HashSet<u32> = HashSet::new();
        let screens = [Some(&self.primary), self.alt.as_ref()];
        for screen in screens.into_iter().flatten() {
            used.extend(screen.cursor.hyperlink);
            screen.pages.collect_hyperlink_ids(&mut used);
        }
        let stale: Vec<u32> = self.hyperlinks.keys().filter(|id| !used.contains(id)).copied().collect();
        for id in stale {
            self.drop_hyperlink(id);
        }
    }
}
