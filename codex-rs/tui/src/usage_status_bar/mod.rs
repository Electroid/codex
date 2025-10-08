use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;

use crate::chatwidget::get_limits_duration;
use crate::status::RateLimitSnapshotDisplay;

const THRESHOLD_PERCENT: f64 = 70.0;
const SIGNIFICANT_CHANGE_THRESHOLD: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrgencyLevel {
    Warning,
    Elevated,
    Critical,
}

impl UrgencyLevel {
    fn from_percent(percent: f64) -> Self {
        if percent >= 90.0 {
            UrgencyLevel::Critical
        } else if percent >= 80.0 {
            UrgencyLevel::Elevated
        } else {
            UrgencyLevel::Warning
        }
    }

    fn label(self) -> &'static str {
        match self {
            UrgencyLevel::Warning => "WARNING",
            UrgencyLevel::Elevated => "ALERT",
            UrgencyLevel::Critical => "CRITICAL",
        }
    }

    fn color(self) -> ratatui::style::Color {
        match self {
            UrgencyLevel::Critical => ratatui::style::Color::Red,
            UrgencyLevel::Warning | UrgencyLevel::Elevated => ratatui::style::Color::Yellow,
        }
    }
}

struct UrgentWindowInfo {
    percent: f64,
    reset_at: Option<String>,
    window_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UsageStatusText {
    pub(crate) text: String,
    pub(crate) at_limit: bool,
}

pub(crate) struct UsageStatusBar {
    snapshot: Option<RateLimitSnapshotDisplay>,
    dismissed: bool,
}

impl Default for UsageStatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageStatusBar {
    pub(crate) fn new() -> Self {
        Self {
            snapshot: None,
            dismissed: false,
        }
    }

    pub(crate) fn update_snapshot(&mut self, snapshot: Option<RateLimitSnapshotDisplay>) {
        let should_reset_dismiss = match (&self.snapshot, &snapshot) {
            (Some(old), Some(new)) => self.has_significant_change(old, new),
            (None, Some(_)) => true,
            _ => false,
        };

        self.snapshot = snapshot;
        if should_reset_dismiss {
            self.dismissed = false;
        }
    }

    fn has_significant_change(
        &self,
        old: &RateLimitSnapshotDisplay,
        new: &RateLimitSnapshotDisplay,
    ) -> bool {
        let old_max = self.max_percent_from_snapshot(old);
        let new_max = self.max_percent_from_snapshot(new);

        match (old_max, new_max) {
            (Some(old_pct), Some(new_pct)) => {
                (new_pct - old_pct).abs() >= SIGNIFICANT_CHANGE_THRESHOLD
            }
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        }
    }

    fn max_percent_from_snapshot(&self, snapshot: &RateLimitSnapshotDisplay) -> Option<f64> {
        let primary_pct = snapshot.primary.as_ref().map(|w| w.used_percent);
        let secondary_pct = snapshot.secondary.as_ref().map(|w| w.used_percent);

        match (primary_pct, secondary_pct) {
            (Some(p), Some(s)) => Some(p.max(s)),
            (Some(p), None) => Some(p),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }

    pub(crate) fn dismiss(&mut self) {
        self.dismissed = true;
    }

    #[cfg(test)]
    pub(crate) fn is_dismissed(&self) -> bool {
        self.dismissed
    }

    pub(crate) fn should_show(&self) -> bool {
        if self.dismissed {
            return false;
        }

        let Some(snapshot) = &self.snapshot else {
            return false;
        };

        let primary_exceeds = snapshot
            .primary
            .as_ref()
            .is_some_and(|w| w.used_percent >= THRESHOLD_PERCENT);

        let secondary_exceeds = snapshot
            .secondary
            .as_ref()
            .is_some_and(|w| w.used_percent >= THRESHOLD_PERCENT);

        primary_exceeds || secondary_exceeds
    }

    pub(crate) fn get_footer_text(&self) -> Option<UsageStatusText> {
        if !self.should_show() {
            return None;
        }

        let UrgentWindowInfo {
            percent,
            reset_at,
            window_label,
        } = self.get_urgent_window_info()?;

        let reset_text = reset_at
            .map(|r| format!(" (resets {r})"))
            .unwrap_or_default();

        let at_limit = percent >= 100.0;

        let label_text = match window_label.as_str() {
            "weekly" | "monthly" | "annual" => format!("{window_label} limit"),
            _ => "limit".to_string(),
        };

        let text = if at_limit {
            format!("{label_text} reached{reset_text}")
        } else {
            format!("{percent:.0}% of {label_text}{reset_text}")
        };

        Some(UsageStatusText { text, at_limit })
    }

    fn get_urgent_window_info(&self) -> Option<UrgentWindowInfo> {
        let snapshot = self.snapshot.as_ref()?;

        let primary_data = snapshot.primary.as_ref().and_then(|w| {
            (w.used_percent >= THRESHOLD_PERCENT).then(|| {
                let label = format_window_duration(w.window_minutes, "5h");
                UrgentWindowInfo {
                    percent: w.used_percent,
                    reset_at: w.resets_at.clone(),
                    window_label: label,
                }
            })
        });

        let secondary_data = snapshot.secondary.as_ref().and_then(|w| {
            (w.used_percent >= THRESHOLD_PERCENT).then(|| {
                let label = format_window_duration(w.window_minutes, "weekly");
                UrgentWindowInfo {
                    percent: w.used_percent,
                    reset_at: w.resets_at.clone(),
                    window_label: label,
                }
            })
        });

        match (primary_data, secondary_data) {
            (Some(p), Some(s)) => Some(if p.percent >= s.percent { p } else { s }),
            (Some(p), None) => Some(p),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }
}

fn format_window_duration(window_minutes: Option<u64>, default: &str) -> String {
    window_minutes
        .map(get_limits_duration)
        .unwrap_or_else(|| default.to_string())
}

impl WidgetRef for &UsageStatusBar {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        if !self.should_show() {
            return;
        }

        let Some(info) = self.get_urgent_window_info() else {
            return;
        };

        let urgency = UrgencyLevel::from_percent(info.percent);
        let label = urgency.label();
        let UrgentWindowInfo {
            percent,
            reset_at,
            window_label,
        } = info;

        let mut text = format!("{label}: {percent:.0}% of {window_label} limit used");

        if let Some(reset) = reset_at {
            text.push_str(&format!(" (resets {reset})"));
        }

        text.push_str(" [Press Esc to dismiss]");

        let line = Line::from(text).fg(urgency.color());

        line.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::RateLimitWindowDisplay;

    fn create_test_snapshot(
        primary_percent: Option<f64>,
        secondary_percent: Option<f64>,
    ) -> RateLimitSnapshotDisplay {
        RateLimitSnapshotDisplay {
            primary: primary_percent.map(|used_percent| RateLimitWindowDisplay {
                used_percent,
                resets_at: Some("in 30m".to_string()),
                window_minutes: Some(300),
            }),
            secondary: secondary_percent.map(|used_percent| RateLimitWindowDisplay {
                used_percent,
                resets_at: Some("in 2h".to_string()),
                window_minutes: Some(10080),
            }),
        }
    }

    // Core visibility tests
    #[test]
    fn test_should_show_below_threshold() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(50.0), Some(60.0))));
        assert!(!bar.should_show());
    }

    #[test]
    fn test_should_show_above_threshold() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(75.0), Some(60.0))));
        assert!(bar.should_show());
    }

    #[test]
    fn test_should_show_dismissed() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(85.0), Some(75.0))));
        assert!(bar.should_show());

        bar.dismiss();
        assert!(!bar.should_show());
    }

    // Window selection tests
    #[test]
    fn test_most_urgent_window_selects_highest() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(75.0), Some(85.0))));

        let info = bar.get_urgent_window_info().unwrap();
        assert_eq!(info.percent, 85.0);
    }

    #[test]
    fn test_selects_secondary_when_higher() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(75.0), Some(95.0))));

        let info = bar.get_urgent_window_info().unwrap();
        assert_eq!(info.percent, 95.0);
    }

    // Dismiss behavior tests
    #[test]
    fn test_dismiss_resets_on_new_snapshot() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(80.0), None)));
        bar.dismiss();
        assert!(bar.is_dismissed());

        bar.update_snapshot(Some(create_test_snapshot(Some(85.0), None)));
        assert!(!bar.is_dismissed());
    }

    #[test]
    fn test_dismiss_persists_on_insignificant_change() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(80.0), None)));
        bar.dismiss();
        assert!(bar.is_dismissed());

        bar.update_snapshot(Some(create_test_snapshot(Some(82.0), None)));
        assert!(bar.is_dismissed());
    }

    // Boundary value tests
    #[test]
    fn test_boundary_at_exactly_70_percent() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(70.0), None)));
        assert!(bar.should_show());

        bar.update_snapshot(Some(create_test_snapshot(Some(69.99), None)));
        assert!(!bar.should_show());
    }

    #[test]
    fn test_urgency_boundary_values() {
        assert_eq!(UrgencyLevel::from_percent(69.9), UrgencyLevel::Warning);
        assert_eq!(UrgencyLevel::from_percent(79.9), UrgencyLevel::Warning);
        assert_eq!(UrgencyLevel::from_percent(80.0), UrgencyLevel::Elevated);
        assert_eq!(UrgencyLevel::from_percent(89.9), UrgencyLevel::Elevated);
        assert_eq!(UrgencyLevel::from_percent(90.0), UrgencyLevel::Critical);
        assert_eq!(UrgencyLevel::from_percent(100.0), UrgencyLevel::Critical);
    }

    // Urgency level tests
    #[test]
    fn test_urgency_levels() {
        assert_eq!(UrgencyLevel::from_percent(75.0), UrgencyLevel::Warning);
        assert_eq!(UrgencyLevel::from_percent(85.0), UrgencyLevel::Elevated);
        assert_eq!(UrgencyLevel::from_percent(95.0), UrgencyLevel::Critical);
    }

    #[test]
    fn test_urgency_level_labels() {
        assert_eq!(UrgencyLevel::Warning.label(), "WARNING");
        assert_eq!(UrgencyLevel::Elevated.label(), "ALERT");
        assert_eq!(UrgencyLevel::Critical.label(), "CRITICAL");
    }

    // Edge case tests
    #[test]
    fn test_both_windows_none() {
        let mut bar = UsageStatusBar::new();
        let snapshot = RateLimitSnapshotDisplay {
            primary: None,
            secondary: None,
        };
        bar.update_snapshot(Some(snapshot));
        assert!(!bar.should_show());
    }

    #[test]
    fn test_default_trait() {
        let bar = UsageStatusBar::default();
        assert!(!bar.is_dismissed());
        assert!(bar.snapshot.is_none());
    }

    // Helper method tests
    #[test]
    fn test_significant_change_detection() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(70.0), None)));

        let old = bar.snapshot.clone().unwrap();
        let new = create_test_snapshot(Some(71.0), None);
        assert!(!bar.has_significant_change(&old, &new));

        let new = create_test_snapshot(Some(76.0), None);
        assert!(bar.has_significant_change(&old, &new));
    }

    #[test]
    fn test_max_percent_from_snapshot() {
        let bar = UsageStatusBar::new();

        let snapshot = create_test_snapshot(Some(80.0), Some(60.0));
        assert_eq!(bar.max_percent_from_snapshot(&snapshot), Some(80.0));

        let snapshot = create_test_snapshot(Some(60.0), Some(90.0));
        assert_eq!(bar.max_percent_from_snapshot(&snapshot), Some(90.0));

        let snapshot = create_test_snapshot(None, Some(75.0));
        assert_eq!(bar.max_percent_from_snapshot(&snapshot), Some(75.0));

        let snapshot = create_test_snapshot(None, None);
        assert_eq!(bar.max_percent_from_snapshot(&snapshot), None);
    }

    #[test]
    fn test_format_window_duration_with_value() {
        let result = format_window_duration(Some(300), "default");
        assert_eq!(result, "5h");
    }

    #[test]
    fn test_format_window_duration_none() {
        let result = format_window_duration(None, "weekly");
        assert_eq!(result, "weekly");
    }

    #[test]
    fn test_get_urgent_window_info_returns_none_when_below_threshold() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(50.0), Some(60.0))));
        assert!(bar.get_urgent_window_info().is_none());
    }

    #[test]
    fn test_get_urgent_window_info_contains_correct_fields() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(80.0), None)));

        let info = bar.get_urgent_window_info().unwrap();
        assert_eq!(info.percent, 80.0);
        assert_eq!(info.reset_at, Some("in 30m".to_string()));
        assert_eq!(info.window_label, "5h");
    }

    #[test]
    fn test_get_footer_text_format() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(85.0), None)));

        let text = bar.get_footer_text().unwrap();
        assert_eq!(text.text, "85% of limit (resets in 30m)");
        assert!(!text.at_limit);
    }

    #[test]
    fn test_get_footer_text_weekly_format() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(None, Some(95.0))));

        let text = bar.get_footer_text().unwrap();
        assert_eq!(text.text, "95% of weekly limit (resets in 2h)");
        assert!(!text.at_limit);
    }

    #[test]
    fn test_get_footer_text_at_100_percent() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(100.0), None)));

        let text = bar.get_footer_text().unwrap();
        assert_eq!(text.text, "limit reached (resets in 30m)");
        assert!(text.at_limit);
    }

    #[test]
    fn test_get_footer_text_above_100_percent() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(120.0), None)));

        let text = bar.get_footer_text().unwrap();
        assert_eq!(text.text, "limit reached (resets in 30m)");
        assert!(text.at_limit);
    }

    #[test]
    fn test_get_footer_text_weekly_at_100_percent() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(None, Some(100.0))));

        let text = bar.get_footer_text().unwrap();
        assert_eq!(text.text, "weekly limit reached (resets in 2h)");
        assert!(text.at_limit);
    }

    #[test]
    fn test_dismiss_clears_footer_text() {
        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(85.0), None)));

        // Verify banner is shown
        assert!(bar.should_show());
        assert!(bar.get_footer_text().is_some());

        // Dismiss the banner
        bar.dismiss();

        // Verify banner is cleared
        assert!(!bar.should_show());
        assert!(bar.get_footer_text().is_none());
    }

    // Widget rendering tests
    #[test]
    fn test_widget_render_when_hidden() {
        use ratatui::widgets::WidgetRef;

        let bar = UsageStatusBar::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        (&bar).render_ref(Rect::new(0, 0, 80, 1), &mut buf);

        let rendered: String = buf
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.trim().is_empty() || rendered.chars().all(|c| c == ' '));
    }

    #[test]
    fn test_widget_render_when_shown() {
        use ratatui::widgets::WidgetRef;

        let mut bar = UsageStatusBar::new();
        bar.update_snapshot(Some(create_test_snapshot(Some(85.0), None)));

        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        (&bar).render_ref(Rect::new(0, 0, 80, 1), &mut buf);

        let rendered: String = buf
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();

        assert!(rendered.contains("85%"));
        assert!(rendered.contains("5h"));
        assert!(rendered.contains("Press Esc"));
    }
}
