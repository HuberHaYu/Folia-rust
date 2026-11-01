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

/// Where the queue was started, for as long as every item is still one of those songs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Queue<T> {
    items: Vec<T>,
    current: Option<usize>,
    origin: Option<Origin>,
    pub repeat: Repeat,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self { items: Vec::new(), current: None, origin: None, repeat: Repeat::Off }
    }
}

impl<T> Queue<T> {
    /// Replaces the queue and makes `start` current. A replacement is not the previous source's queue.
    pub fn replace(&mut self, items: Vec<T>, start: usize) {
        self.current = (start < items.len()).then_some(start);
        self.items = items;
        self.origin = None;
    }

    /// `replace`, and records `origin` as where these songs were started.
    pub fn replace_from(&mut self, items: Vec<T>, start: usize, origin: Origin) {
        self.replace(items, start);
        self.origin = Some(origin);
    }

    pub fn origin(&self) -> Option<&Origin> {
        self.origin.as_ref()
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

    /// Makes `index` the current item.
    pub fn select(&mut self, index: usize) -> Option<&T> {
        if index >= self.items.len() {
            return None;
        }
        self.current = Some(index);
        self.items.get(index)
    }

    /// Removes `index`. The item that slides into a removed current item becomes current.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        self.current = match self.current {
            Some(current) if current > index => Some(current - 1),
            Some(current) if current == index => {
                if self.items.is_empty() {
                    None
                } else if current >= self.items.len() {
                    Some(self.items.len() - 1)
                } else {
                    Some(current)
                }
            }
            other => other,
        };
        Some(item)
    }

    /// Places `index` immediately after the current item. The current item stays current.
    pub fn move_after_current(&mut self, index: usize) -> bool {
        let Some(current) = self.current else { return false };
        if index >= self.items.len() || index == current {
            return false;
        }
        let item = self.items.remove(index);
        let current = if index < current { current - 1 } else { current };
        let insert = (current + 1).min(self.items.len());
        self.items.insert(insert, item);
        self.current = Some(current);
        true
    }

    /// Moves `index` to the end of the queue.
    pub fn move_to_end(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let item = self.items.remove(index);
        self.items.push(item);
        if let Some(current) = self.current {
            if current == index {
                self.current = Some(self.items.len() - 1);
            } else if current > index {
                self.current = Some(current - 1);
            }
        }
        true
    }

    /// Moves to the next item. `finished` is true when the current item played to its end,
    /// in which case repeat-one replays it.
    pub fn advance(&mut self, finished: bool) -> Option<&T> {
        let current = self.current?;
        let next = if finished && self.repeat == Repeat::One { Some(current) } else { self.next_index() };
        self.current = next.or(self.current);
        next.and_then(|index| self.items.get(index))
    }

    /// Moves to the previous item, wrapping only when repeating the whole queue.
    pub fn back(&mut self) -> Option<&T> {
        let previous = self.previous_index();
        self.current = previous.or(self.current);
        previous.and_then(|index| self.items.get(index))
    }

    /// The item a skip forward would play, wrapping only when repeating the whole queue.
    pub fn next_index(&self) -> Option<usize> {
        let current = self.current?;
        match self.repeat {
            Repeat::All => Some((current + 1) % self.items.len()),
            _ => (current + 1 < self.items.len()).then_some(current + 1),
        }
    }

    /// The item a skip back would play, wrapping only when repeating the whole queue.
    pub fn previous_index(&self) -> Option<usize> {
        match (self.current?, self.repeat) {
            (0, Repeat::All) => self.items.len().checked_sub(1),
            (0, _) => None,
            (current, _) => Some(current - 1),
        }
    }
}
