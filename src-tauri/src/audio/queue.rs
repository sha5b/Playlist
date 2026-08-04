use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTrack {
    pub id: i64,
    pub title: String,
    pub artist_name: Option<String>,
    pub album_title: Option<String>,
    pub duration_ms: Option<i64>,
    pub file_path: String,
    pub cover_art_path: Option<String>,
}

pub struct PlayQueue {
    /// Original ordered list
    tracks: Vec<QueueTrack>,
    /// Play order indices (shuffled or sequential)
    order: Vec<usize>,
    /// Current position in `order`
    position: Option<usize>,
    shuffle: bool,
    /// Set when the currently playing entry was removed from the queue while
    /// tracks remained after it: `position` already points at the FOLLOWING
    /// track, so the next `next()` must stay in place instead of advancing
    /// (otherwise one track gets silently skipped).
    stay_on_next: bool,
}

impl PlayQueue {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            order: Vec::new(),
            position: None,
            shuffle: false,
            stay_on_next: false,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<QueueTrack>, start_index: usize) {
        let len = tracks.len();
        self.tracks = tracks;
        self.stay_on_next = false;
        self.rebuild_order();
        // Clamp start_index to valid range, then find its position in play order
        let clamped = if len > 0 { start_index.min(len - 1) } else { 0 };
        self.position = if len > 0 {
            self.order.iter().position(|&i| i == clamped).or(Some(0))
        } else {
            None
        };
    }

    pub fn add_track(&mut self, track: QueueTrack) {
        let idx = self.tracks.len();
        self.tracks.push(track);
        // Insert at end of order (after current position's remaining items)
        self.order.push(idx);
    }

    pub fn add_next(&mut self, track: QueueTrack) {
        let idx = self.tracks.len();
        self.tracks.push(track);
        // Insert right after current position
        let insert_pos = self.position.map(|p| p + 1).unwrap_or(0);
        self.order.insert(insert_pos, idx);
    }

    pub fn current(&self) -> Option<&QueueTrack> {
        self.position
            .and_then(|pos| self.order.get(pos))
            .and_then(|&idx| self.tracks.get(idx))
    }

    pub fn next(&mut self) -> Option<&QueueTrack> {
        if self.tracks.is_empty() {
            return None;
        }
        // The current entry was removed while playing — `position` already
        // points at the track that should play next, so don't advance.
        if self.stay_on_next {
            self.stay_on_next = false;
            return self.current();
        }
        match self.position {
            Some(pos) if pos + 1 < self.order.len() => {
                self.position = Some(pos + 1);
            }
            _ => {
                // Reached end — caller should check repeat mode
                return None;
            }
        }
        self.current()
    }

    pub fn prev(&mut self) -> Option<&QueueTrack> {
        if self.tracks.is_empty() {
            return None;
        }
        self.stay_on_next = false;
        match self.position {
            Some(pos) if pos > 0 => {
                self.position = Some(pos - 1);
            }
            _ => return None,
        }
        self.current()
    }

    /// Skip to a specific position in the play order
    pub fn skip_to(&mut self, order_index: usize) -> Option<&QueueTrack> {
        if order_index < self.order.len() {
            self.stay_on_next = false;
            self.position = Some(order_index);
            self.current()
        } else {
            None
        }
    }

    /// Restart the queue from the beginning (used for RepeatAll when end is reached)
    pub fn restart(&mut self) -> Option<&QueueTrack> {
        if self.tracks.is_empty() {
            return None;
        }
        self.stay_on_next = false;
        self.position = Some(0);
        self.current()
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current_track_idx = self.position.and_then(|p| self.order.get(p).copied());
        self.rebuild_order();
        // Restore position to current track
        if let Some(idx) = current_track_idx {
            self.position = self.order.iter().position(|&i| i == idx);
        }
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.order.clear();
        self.position = None;
        self.stay_on_next = false;
    }

    pub fn len(&self) -> usize {
        // Removed entries stay in `tracks` but leave `order` — the play order
        // is the real queue length.
        self.order.len()
    }

    pub fn position(&self) -> Option<usize> {
        self.position
    }

    /// Get all tracks in play order
    pub fn get_ordered_tracks(&self) -> Vec<QueueTrack> {
        self.order
            .iter()
            .filter_map(|&idx| self.tracks.get(idx).cloned())
            .collect()
    }

    /// Peek at the next track without advancing the position.
    /// If `wrap` is true, wraps around to the first track when at the end (for RepeatAll).
    pub fn peek_next(&self, wrap: bool) -> Option<&QueueTrack> {
        match self.position {
            Some(pos) if pos + 1 < self.order.len() => {
                self.order.get(pos + 1).and_then(|&idx| self.tracks.get(idx))
            }
            Some(_) if wrap && !self.tracks.is_empty() => {
                self.order.first().and_then(|&idx| self.tracks.get(idx))
            }
            _ => None,
        }
    }

    pub fn move_in_queue(&mut self, from: usize, to: usize) {
        if from >= self.order.len() || to >= self.order.len() || from == to {
            return;
        }
        let item = self.order.remove(from);
        self.order.insert(to, item);
        // Adjust position to keep tracking the currently-playing item
        if let Some(pos) = self.position {
            if from == pos {
                self.position = Some(to);
            } else if from < pos && to >= pos {
                self.position = Some(pos - 1);
            } else if from > pos && to <= pos {
                self.position = Some(pos + 1);
            }
        }
    }

    pub fn remove_at_order_index(&mut self, order_idx: usize) {
        if order_idx >= self.order.len() {
            return;
        }
        self.order.remove(order_idx);
        // Adjust position
        if let Some(pos) = self.position {
            if order_idx < pos {
                self.position = Some(pos - 1);
            } else if order_idx == pos {
                if pos >= self.order.len() {
                    // Removed the last entry — step back to the new last one
                    self.position = if self.order.is_empty() { None } else { Some(self.order.len() - 1) };
                } else {
                    // Removed the CURRENT entry with tracks after it: position
                    // now points at the following track. Flag it so the next
                    // `next()` plays that track instead of skipping past it.
                    self.stay_on_next = true;
                }
            }
        }
    }

    fn rebuild_order(&mut self) {
        let len = self.tracks.len();
        self.order = (0..len).collect();
        if self.shuffle {
            let mut rng = rand::thread_rng();
            self.order.shuffle(&mut rng);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64) -> QueueTrack {
        QueueTrack {
            id,
            title: format!("Track {}", id),
            artist_name: None,
            album_title: None,
            duration_ms: None,
            file_path: String::new(),
            cover_art_path: None,
        }
    }

    fn queue_with(n: i64) -> PlayQueue {
        let mut q = PlayQueue::new();
        q.set_tracks((1..=n).map(track).collect(), 0);
        q
    }

    #[test]
    fn removing_current_does_not_skip_next_track() {
        // Queue [1, 2, 3], playing 1. Remove 1 while it plays → the next
        // natural advance must play 2, not 3.
        let mut q = queue_with(3);
        assert_eq!(q.current().unwrap().id, 1);
        q.remove_at_order_index(0);
        assert_eq!(q.next().unwrap().id, 2);
        assert_eq!(q.next().unwrap().id, 3);
    }

    #[test]
    fn removing_before_current_keeps_current() {
        let mut q = queue_with(3);
        q.skip_to(1); // playing 2
        q.remove_at_order_index(0); // remove 1
        assert_eq!(q.current().unwrap().id, 2);
        assert_eq!(q.next().unwrap().id, 3);
    }

    #[test]
    fn removing_last_while_current_steps_back() {
        let mut q = queue_with(2);
        q.skip_to(1); // playing 2 (last)
        q.remove_at_order_index(1);
        assert_eq!(q.current().unwrap().id, 1);
        assert!(q.next().is_none());
    }

    #[test]
    fn explicit_skip_clears_stay_flag() {
        let mut q = queue_with(3);
        q.remove_at_order_index(0); // stay flag set
        q.skip_to(1); // user explicitly jumps to 3 (order is now [2, 3])
        assert_eq!(q.current().unwrap().id, 3);
        assert!(q.next().is_none()); // end of queue, no phantom repeat
    }

    #[test]
    fn len_reflects_removals() {
        let mut q = queue_with(3);
        q.remove_at_order_index(2);
        assert_eq!(q.len(), 2);
    }
}
