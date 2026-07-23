use std::{io::IsTerminal, sync::Arc, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::AppContext;

use super::{collect_running, draw, ConsoleView};

/// Fast enough that a build's elapsed counter ticks visibly, slow enough to cost
/// nothing.
const REDRAW_INTERVAL: Duration = Duration::from_millis(250);

/// True when there is a terminal to draw on.
///
/// Under launchd, a pipe or a redirect there is not, and taking over the screen
/// would produce a file full of escape codes — so the server logs plain lines
/// instead.
pub fn stdout_is_a_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Runs the console until the user quits, then asks the server to shut down.
///
/// Spawned as a normal task rather than a blocking one: a redraw takes
/// microseconds and input is polled with a zero timeout, so nothing here ever
/// occupies a worker thread for long.
pub async fn run(app: Arc<AppContext>) {
    if let Err(err) = enable_raw_mode() {
        eprintln!(
            "Can not start the console: {}. Falling back to plain output.",
            err
        );
        return;
    }

    let mut stdout = std::io::stdout();

    if let Err(err) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        eprintln!(
            "Can not start the console: {}. Falling back to plain output.",
            err
        );
        return;
    }

    // A panic anywhere else in the process must not leave the terminal in raw
    // mode with the alternate screen still on — that leaves an unusable shell.
    install_panic_restore();

    let backend = CrosstermBackend::new(std::io::stdout());

    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => {
            restore();
            eprintln!("Can not start the console: {}", err);
            return;
        }
    };

    loop {
        let running = collect_running(&app.repos);
        let history = app.activity.recent(200);

        let view = ConsoleView {
            bind_addr: &app.bind_addr,
            repos: app.repos.len(),
            running: &running,
            history: &history,
        };

        if terminal.draw(|frame| draw(frame, &view)).is_err() {
            break;
        }

        if should_quit() {
            break;
        }

        tokio::time::sleep(REDRAW_INTERVAL).await;
    }

    restore();

    // Quitting the console means quitting the server — it is the foreground
    // process, and leaving it running headless would surprise.
    app.app_states.set_shutting_down();
}

/// Non-blocking: returns as soon as it has drained whatever input is waiting.
fn should_quit() -> bool {
    loop {
        match event::poll(Duration::ZERO) {
            Ok(true) => {}
            _ => return false,
        }

        let key = match event::read() {
            Ok(Event::Key(key)) => key,
            Ok(_) => continue,
            Err(_) => return false,
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        let quit = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c')));

        if quit {
            return true;
        }
    }
}

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
}

fn install_panic_restore() {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
