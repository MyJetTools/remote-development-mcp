use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Row, Table},
    Frame,
};

use crate::activity::{ActivityEvent, ActivityKind};

use super::{render_elapsed, RunningJob};

pub struct ConsoleView<'s> {
    pub bind_addr: &'s str,
    pub repos: usize,
    pub running: &'s [RunningJob],
    pub history: &'s [ActivityEvent],
}

pub fn draw(frame: &mut Frame, view: &ConsoleView) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            // The running pane takes the smaller share: there are rarely many
            // jobs at once, while history is what fills a session.
            Constraint::Percentage(40),
            Constraint::Min(3),
        ])
        .split(frame.area());

    draw_header(frame, areas[0], view);
    draw_running(frame, areas[1], view.running);
    draw_history(frame, areas[2], view.history);
}

fn draw_header(frame: &mut Frame, area: ratatui::layout::Rect, view: &ConsoleView) {
    let line = Line::from(vec![
        Span::styled(
            " remote-development-mcp ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  listening on {}  ·  {} repositor{}  ·  press q to quit",
            view.bind_addr,
            view.repos,
            if view.repos == 1 { "y" } else { "ies" }
        )),
    ]);

    frame.render_widget(ratatui::widgets::Paragraph::new(line), area);
}

fn draw_running(frame: &mut Frame, area: ratatui::layout::Rect, running: &[RunningJob]) {
    let title = if running.is_empty() {
        " Running — nothing right now ".to_string()
    } else {
        format!(" Running — {} ", running.len())
    };

    let rows: Vec<Row> = running
        .iter()
        .map(|job| {
            Row::new(vec![
                Span::styled(
                    render_elapsed(job.elapsed_sec),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(job.repo.clone(), Style::default().fg(Color::Cyan)),
                Span::raw(job.job_id.clone()),
                Span::raw(job.command_line.clone()),
                Span::styled(
                    match job.remaining_sec {
                        Some(remaining) => render_elapsed(remaining),
                        None => "-".to_string(),
                    },
                    // Red once the deadline is close, so it reads at a glance.
                    Style::default().fg(match job.remaining_sec {
                        Some(remaining) if remaining < 60.0 => Color::Red,
                        _ => Color::DarkGray,
                    }),
                ),
                Span::styled(job.cwd.clone(), Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(16),
        ],
    )
    .header(
        Row::new(vec!["elapsed", "repo", "job", "command", "left", "cwd"]).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(table, area);
}

fn draw_history(frame: &mut Frame, area: ratatui::layout::Rect, history: &[ActivityEvent]) {
    let rows: Vec<Row> = history
        .iter()
        .map(|event| {
            let (marker, colour) = match event.kind {
                ActivityKind::ToolCall => ("→", Color::Green),
                ActivityKind::ToolFailed => ("✗", Color::Red),
                ActivityKind::JobFinished => ("←", Color::Blue),
            };

            Row::new(vec![
                Span::styled(event.time_of_day(), Style::default().fg(Color::DarkGray)),
                Span::styled(marker.to_string(), Style::default().fg(colour)),
                Span::styled(event.repo.clone(), Style::default().fg(Color::Cyan)),
                Span::styled(
                    event.subject.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(event.detail.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Length(18),
            Constraint::Length(20),
            Constraint::Min(20),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" History — newest first ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    use ratatui::{backend::TestBackend, Terminal};

    use crate::activity::ActivityEvent;

    /// Renders into an in-memory buffer, so the layout is exercised for real
    /// without needing a terminal.
    fn render_to_text(running: &[RunningJob], history: &[ActivityEvent]) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &ConsoleView {
                        bind_addr: "127.0.0.1:8123",
                        repos: 2,
                        running,
                        history,
                    },
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();

        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn both_panes_and_the_header_are_drawn() {
        let running = vec![RunningJob {
            repo: "mt-risks".to_string(),
            job_id: "job-000007".to_string(),
            command_line: "cargo build".to_string(),
            cwd: ".".to_string(),
            elapsed_sec: 94.0,
            remaining_sec: Some(3506.0),
        }];

        let history = vec![ActivityEvent::tool_call(
            "mt-risks".to_string(),
            "search".to_string(),
            r#"{"pattern":"needle"}"#.to_string(),
        )];

        let rendered = render_to_text(&running, &history);

        // Header.
        assert!(rendered.contains("remote-development-mcp"));
        assert!(rendered.contains("127.0.0.1:8123"));
        assert!(rendered.contains("2 repositories"));

        // Running pane: the job, and its elapsed time formatted.
        assert!(rendered.contains("Running"));
        assert!(rendered.contains("job-000007"));
        assert!(rendered.contains("cargo build"));
        assert!(rendered.contains("1m 34s"));

        // History pane.
        assert!(rendered.contains("History"));
        assert!(rendered.contains("search"));
    }

    #[test]
    fn an_idle_server_says_so_rather_than_showing_an_empty_box() {
        let rendered = render_to_text(&[], &[]);

        assert!(rendered.contains("nothing right now"));
    }

    #[test]
    fn one_repository_is_not_called_repositories() {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &ConsoleView {
                        bind_addr: "127.0.0.1:8123",
                        repos: 1,
                        running: &[],
                        history: &[],
                    },
                )
            })
            .unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered.contains("1 repository"));
        assert!(!rendered.contains("1 repositories"));
    }
}
