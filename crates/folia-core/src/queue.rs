// Play queue: the order of items, the current position and the repeat mode.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Queue<T> {
    items: Vec<T>,
    current: Option<usize>,
    pub repeat: Repeat,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self { items: Vec::new(), current: None, repeat: Repeat::Off }
    }
}

impl<T> Queue<T> {
    /// Replaces the queue and makes `start` current.
    pub fn replace(&mut self, items: Vec<T>, start: usize) {
        self.current = (start < items.len()).then_some(start);
        self.items = items;
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&T> {
        self.items.get(self.current?)
    }

    pub fn current_mut(&mut self) -> Option<&mut T> {
        self.items.get_mut(self.current?)
    }

    /// Moves to the next item. `finished` is true when the current item played to its end,
    /// in which case repeat-one replays it.
    pub fn advance(&mut self, finished: bool) -> Option<&T> {
        let current = self.current?;
        let next = match self.repeat {
            Repeat::One if finished => Some(current),
            Repeat::All => Some((current + 1) % self.items.len()),
            _ => (current + 1 < self.items.len()).then_some(current + 1),
        };
        self.current = next.or(self.current);
        next.and_then(|index| self.items.get(index))
    }

    /// Moves to the previous item, wrapping only when repeating the whole queue.
    pub fn back(&mut self) -> Option<&T> {
        let current = self.current?;
        let previous = match (current, self.repeat) {
            (0, Repeat::All) => self.items.len().checked_sub(1),
            (0, _) => None,
            _ => Some(current - 1),
        };
        self.current = previous.or(self.current);
        previous.and_then(|index| self.items.get(index))
    }
}
