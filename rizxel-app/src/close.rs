//! "Save changes?" prompt before unsaved work is closed: a pure state
//! machine over document ids, driven by [`crate::AppState`] and drawn by the
//! UI as a modal.

use std::collections::VecDeque;

/// What the prompt guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// The tab's close button (one document).
    CloseTab,
    /// *File ▸ Exit*, the title bar, `Alt+F4` or the OS closing the window.
    Exit,
}

/// Answer to "Save changes to *name* before closing?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    Save,
    DontSave,
    Cancel,
}

/// What has to happen next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStep {
    /// Ask about this document.
    Ask(u64),
    /// Save this document (Save As when it has no path), then report the
    /// result with [`CloseRequest::saved`].
    Save(u64),
    /// Close this tab; the request is finished.
    Close(u64),
    /// Every document is answered for: the application may quit.
    Quit,
    /// Cancelled (or a save failed): nothing more is closed.
    Abort,
}

/// Dirty documents still to confirm, one prompt at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseRequest {
    pub action: CloseAction,
    queue: VecDeque<u64>,
}

impl CloseRequest {
    /// Closing one dirty tab.
    pub fn close_tab(doc: u64) -> Self {
        Self {
            action: CloseAction::CloseTab,
            queue: VecDeque::from([doc]),
        }
    }

    /// Exiting with these dirty documents, asked about in order.
    pub fn exit(dirty: impl IntoIterator<Item = u64>) -> Self {
        Self {
            action: CloseAction::Exit,
            queue: dirty.into_iter().collect(),
        }
    }

    /// Document the prompt is about.
    pub fn current(&self) -> Option<u64> {
        self.queue.front().copied()
    }

    /// First step: ask about the first document (or quit when none is dirty).
    pub fn start(&self) -> CloseStep {
        match self.current() {
            Some(id) => CloseStep::Ask(id),
            None if self.action == CloseAction::Exit => CloseStep::Quit,
            None => CloseStep::Abort,
        }
    }

    pub fn answer(&mut self, choice: CloseChoice) -> CloseStep {
        let Some(id) = self.current() else {
            return self.start();
        };
        match choice {
            CloseChoice::Save => CloseStep::Save(id),
            CloseChoice::DontSave => self.advance(),
            CloseChoice::Cancel => CloseStep::Abort,
        }
    }

    /// Result of the save asked for by [`CloseStep::Save`]: on success the
    /// request goes on as after "Don't Save"; a failed or cancelled save
    /// (Save As dialog closed) aborts it.
    pub fn saved(&mut self, ok: bool) -> CloseStep {
        if ok {
            self.advance()
        } else {
            CloseStep::Abort
        }
    }

    /// The current document is answered for.
    fn advance(&mut self) -> CloseStep {
        let Some(id) = self.queue.pop_front() else {
            return self.start();
        };
        match self.action {
            CloseAction::CloseTab => CloseStep::Close(id),
            CloseAction::Exit => self.start(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_close_asks_once() {
        let mut r = CloseRequest::close_tab(7);
        assert_eq!(r.start(), CloseStep::Ask(7));
        assert_eq!(r.answer(CloseChoice::DontSave), CloseStep::Close(7));

        let mut r = CloseRequest::close_tab(7);
        assert_eq!(r.answer(CloseChoice::Save), CloseStep::Save(7));
        assert_eq!(r.saved(true), CloseStep::Close(7));

        let mut r = CloseRequest::close_tab(7);
        assert_eq!(r.answer(CloseChoice::Cancel), CloseStep::Abort);
    }

    #[test]
    fn exit_walks_every_dirty_document() {
        let mut r = CloseRequest::exit([1, 2]);
        assert_eq!(r.start(), CloseStep::Ask(1));
        assert_eq!(r.answer(CloseChoice::DontSave), CloseStep::Ask(2));
        assert_eq!(r.answer(CloseChoice::Save), CloseStep::Save(2));
        assert_eq!(r.saved(true), CloseStep::Quit);
        assert_eq!(CloseRequest::exit([]).start(), CloseStep::Quit);
    }

    #[test]
    fn a_cancelled_save_aborts_the_exit() {
        let mut r = CloseRequest::exit([1, 2]);
        assert_eq!(r.answer(CloseChoice::Save), CloseStep::Save(1));
        assert_eq!(r.saved(false), CloseStep::Abort);
    }
}
