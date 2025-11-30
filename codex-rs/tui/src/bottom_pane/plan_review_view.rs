use crate::app_event::AppEvent;
use crate::app_event::PlanReviewAction;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::bottom_pane_view::BottomPaneView;
use crate::history_cell;
use crate::history_cell::HistoryCell;
use crate::render::renderable::Renderable;
use codex_protocol::plan_tool::UpdatePlanArgs;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

pub(crate) struct PlanReviewView {
    app_event_tx: AppEventSender,
    goal_summary: Option<String>,
    feedback: Vec<String>,
    plan_cell: history_cell::PlanUpdateCell,
    complete: bool,
}

impl PlanReviewView {
    pub(crate) fn new(
        app_event_tx: AppEventSender,
        goal_summary: Option<String>,
        feedback: Vec<String>,
        plan: UpdatePlanArgs,
    ) -> Self {
        Self {
            app_event_tx,
            goal_summary,
            feedback,
            plan_cell: history_cell::new_plan_update(plan),
            complete: false,
        }
    }

    fn finish(&mut self, action: PlanReviewAction) {
        if !self.complete {
            self.complete = true;
            self.app_event_tx.send(AppEvent::PlanReviewAction(action));
        }
    }

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(vec!["Plan mode".bold(), " — review Codex's proposal".into()].into());
        if let Some(goal) = &self.goal_summary {
            lines.push(vec!["Goal: ".dim(), goal.clone().into()].into());
        }
        lines.push(Line::from(""));

        let plan_lines = self.plan_cell.display_lines(width);
        lines.extend(plan_lines);

        if !self.feedback.is_empty() {
            lines.push(Line::from(""));
            lines.push("Feedback provided:".bold().into());
            for entry in &self.feedback {
                lines.push(vec!["  • ".into(), entry.clone().into()].into());
            }
        }

        lines.push(Line::from(""));
        lines.push(
            vec![
                "[Enter] run plan".bold(),
                "   ".into(),
                "[F] refine plan".into(),
                "   ".into(),
                "[Esc] cancel".into(),
            ]
            .into(),
        );
        lines
    }
}

impl Renderable for PlanReviewView {
    fn desired_height(&self, width: u16) -> u16 {
        let content_height = self.render_lines(width).len() as u16;
        // Add top/bottom borders.
        content_height.saturating_add(2)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let block = Block::default().borders(Borders::ALL).title("Plan review");
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }

        let lines = self.render_lines(inner.width);

        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(inner, buf);
    }
}

impl BottomPaneView for PlanReviewView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Enter => self.finish(PlanReviewAction::Execute),
            KeyCode::Char('f') | KeyCode::Char('F') => self.finish(PlanReviewAction::Feedback),
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => {
                self.finish(PlanReviewAction::Cancel)
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.finish(PlanReviewAction::Cancel);
        CancellationEvent::Handled
    }
}
