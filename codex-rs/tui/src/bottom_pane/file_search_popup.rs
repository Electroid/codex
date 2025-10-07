use codex_file_search::FileMatch;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use crate::render::Insets;
use crate::render::RectExt;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;

/// Visual state for the file-search popup.
pub(crate) struct FileSearchPopup {
    /// Query corresponding to the `matches` currently shown.
    display_query: String,
    /// Latest query typed by the user. May differ from `display_query` when
    /// a search is still in-flight.
    pending_query: String,
    /// When `true` we are still waiting for results for `pending_query`.
    waiting: bool,
    /// All fetched matches from the search (up to 1000); paths relative to the search dir.
    matches: Vec<FileMatch>,
    /// Number of matches available for display. Set to matches.len() when results arrive.
    /// Used for bounds checking during scrolling.
    displayed_count: usize,
    /// Shared selection/scroll state.
    state: ScrollState,
}

fn to_display_row(m: &FileMatch) -> GenericDisplayRow {
    GenericDisplayRow {
        name: m.path.clone(),
        match_indices: m
            .indices
            .as_ref()
            .map(|v| v.iter().map(|&i| i as usize).collect()),
        is_current: false,
        display_shortcut: None,
        description: None,
    }
}

impl FileSearchPopup {
    pub(crate) fn new() -> Self {
        Self {
            display_query: String::new(),
            pending_query: String::new(),
            waiting: false,
            matches: Vec::new(),
            displayed_count: 0,
            state: ScrollState::new(),
        }
    }

    /// Update the query and reset state to *waiting*.
    pub(crate) fn set_query(&mut self, query: &str) {
        // Determine if current matches are still relevant.
        let keep_existing = query.starts_with(&self.display_query);

        self.pending_query = query.to_string();

        self.waiting = true;

        if !keep_existing {
            self.matches.clear();
            self.displayed_count = 0;
            self.state.reset();
        }
    }

    /// Replace matches when a `FileSearchResult` arrives. Only applied when `query` matches `pending_query`.
    /// All results are immediately available for scrolling.
    pub(crate) fn set_matches(&mut self, query: &str, matches: Vec<FileMatch>) {
        if query != self.pending_query && !(query.is_empty() && self.pending_query.is_empty()) {
            return; // stale
        }

        self.display_query = query.to_string();
        self.matches = matches;
        self.waiting = false;

        // Show all results immediately (up to 1000 from backend)
        self.displayed_count = self.matches.len();

        // Ensure selection stays within bounds
        self.state.clamp_selection(self.displayed_count);
    }

    /// Maintains invariant: selected_idx is always in [scroll_top, scroll_top + VISIBLE_COUNT)
    fn ensure_selection_visible(&mut self) {
        const VISIBLE_COUNT: usize = MAX_POPUP_ROWS;

        if let Some(sel) = self.state.selected_idx {
            if sel < self.state.scroll_top {
                self.state.scroll_top = sel;
            } else if sel >= self.state.scroll_top + VISIBLE_COUNT {
                self.state.scroll_top = sel + 1 - VISIBLE_COUNT;
            }
        } else {
            self.state.scroll_top = 0;
        }
    }

    pub(crate) fn move_up(&mut self) {
        if self.displayed_count == 0 {
            return;
        }
        self.state.move_up(self.displayed_count);
        self.ensure_selection_visible();
    }

    pub(crate) fn move_down(&mut self) {
        if self.displayed_count == 0 {
            return;
        }
        self.state.move_down(self.displayed_count);
        self.ensure_selection_visible();
    }

    pub(crate) fn selected_match(&self) -> Option<&str> {
        self.state
            .selected_idx
            .and_then(|idx| self.matches.get(idx))
            .map(|file_match| file_match.path.as_str())
    }

    pub(crate) fn calculate_required_height(&self) -> u16 {
        // File paths don't wrap (they're truncated with ellipsis), so each match
        // is exactly one row. Return the number of DISPLAYED matches clamped to MAX_POPUP_ROWS,
        // or 1 if empty to keep the popup visible.
        self.displayed_count.clamp(1, MAX_POPUP_ROWS) as u16
    }
}

impl WidgetRef for &FileSearchPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        // Only convert displayed items to rows (incremental loading)
        let rows_all: Vec<GenericDisplayRow> = self
            .matches
            .iter()
            .take(self.displayed_count)
            .map(to_display_row)
            .collect();

        let empty_message = if self.waiting {
            "loading..."
        } else {
            "no matches"
        };

        render_rows(
            area.inset(Insets::tlbr(0, 2, 0, 0)),
            buf,
            &rows_all,
            &self.state,
            MAX_POPUP_ROWS,
            empty_message,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_match(path: &str) -> FileMatch {
        FileMatch {
            score: 0,
            path: path.to_string(),
            indices: None,
        }
    }

    #[test]
    fn test_scroll_down_stays_within_window() {
        let mut popup = FileSearchPopup::new();
        let matches: Vec<FileMatch> = (0..20)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches);

        assert_eq!(popup.state.selected_idx, Some(0));
        assert_eq!(popup.state.scroll_top, 0);

        // MAX_POPUP_ROWS = 8, so items 0-7 should have scroll_top = 0
        for i in 1..8 {
            popup.move_down();
            assert_eq!(popup.state.selected_idx, Some(i));
            assert_eq!(
                popup.state.scroll_top, 0,
                "scroll_top should be 0 for items 0-7"
            );
        }

        popup.move_down();
        assert_eq!(popup.state.selected_idx, Some(8));
        assert_eq!(
            popup.state.scroll_top, 1,
            "scroll_top should be 1 when selection is at 8"
        );

        popup.move_down();
        assert_eq!(popup.state.selected_idx, Some(9));
        assert_eq!(
            popup.state.scroll_top, 2,
            "scroll_top should be 2 when selection is at 9"
        );
    }

    #[test]
    fn test_scroll_up_basic() {
        let mut popup = FileSearchPopup::new();
        let matches: Vec<FileMatch> = (0..20)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches);
        popup.state.selected_idx = Some(5);
        popup.state.scroll_top = 2;

        popup.move_up();
        assert_eq!(popup.state.selected_idx, Some(4));
        assert_eq!(popup.state.scroll_top, 2);

        popup.move_up();
        popup.move_up();
        assert_eq!(popup.state.selected_idx, Some(2));
        assert_eq!(popup.state.scroll_top, 2);

        popup.move_up();
        assert_eq!(popup.state.selected_idx, Some(1));
        assert_eq!(popup.state.scroll_top, 1);
    }

    #[test]
    fn test_set_matches_clamps_selection() {
        let mut popup = FileSearchPopup::new();
        let matches1: Vec<FileMatch> = (0..10)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches1);
        popup.state.selected_idx = Some(9);

        let matches2: Vec<FileMatch> = (0..5).map(|i| make_match(&format!("file{i}.rs"))).collect();
        popup.set_query("test2");
        popup.set_matches("test2", matches2);

        assert_eq!(popup.state.selected_idx, Some(4));
    }

    #[test]
    fn test_invariant_always_maintained() {
        let mut popup = FileSearchPopup::new();
        let matches: Vec<FileMatch> = (0..20)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches);

        const VISIBLE_COUNT: usize = MAX_POPUP_ROWS;

        // Test invariant after many down movements
        for _ in 0..15 {
            popup.move_down();
            if let Some(sel) = popup.state.selected_idx {
                assert!(
                    sel >= popup.state.scroll_top && sel < popup.state.scroll_top + VISIBLE_COUNT,
                    "INVARIANT VIOLATED: selection {} not in [scroll_top={}, scroll_top+{})",
                    sel,
                    popup.state.scroll_top,
                    VISIBLE_COUNT
                );
            }
        }

        // Test invariant after moving back up
        for _ in 0..10 {
            popup.move_up();
            if let Some(sel) = popup.state.selected_idx {
                assert!(
                    sel >= popup.state.scroll_top && sel < popup.state.scroll_top + VISIBLE_COUNT,
                    "INVARIANT VIOLATED: selection {} not in [scroll_top={}, scroll_top+{})",
                    sel,
                    popup.state.scroll_top,
                    VISIBLE_COUNT
                );
            }
        }
    }

    #[test]
    fn test_no_skipping_on_scroll() {
        let mut popup = FileSearchPopup::new();
        let matches: Vec<FileMatch> = (0..20)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches);

        let mut prev_sel = popup
            .state
            .selected_idx
            .expect("Expected initial selection to be set");

        for _ in 0..15 {
            popup.move_down();
            let curr_sel = popup
                .state
                .selected_idx
                .expect("Expected selection to remain set during navigation");
            assert_eq!(
                curr_sel,
                prev_sel + 1,
                "Selection skipped from {prev_sel} to {curr_sel}"
            );
            prev_sel = curr_sel;
        }
    }

    #[test]
    fn test_all_results_scrollable() {
        let mut popup = FileSearchPopup::new();
        // Create 100 matches to test scrolling through all results
        let matches: Vec<FileMatch> = (0..100)
            .map(|i| make_match(&format!("file{i}.rs")))
            .collect();

        popup.set_query("test");
        popup.set_matches("test", matches);

        // All results should be immediately available
        assert_eq!(popup.displayed_count, 100);
        assert_eq!(popup.matches.len(), 100);
        assert_eq!(popup.state.selected_idx, Some(0));

        // Should be able to scroll through all 100 items
        for i in 1..100 {
            popup.move_down();
            assert_eq!(popup.state.selected_idx, Some(i));
        }

        // Should stop at last item
        popup.move_down();
        assert_eq!(popup.state.selected_idx, Some(99));
    }
}
