use std::{
    fs,
    io::{self, Stdout},
    path::Path,
    sync::LazyLock,
};

use crossterm::{
    event::{self, DisableMouseCapture, KeyCode},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::info;
use neli_wifi::{AsyncSocket, Interface};
use tui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
};

static LINUX_CONFIG: LazyLock<String> = LazyLock::new(|| {
    std::env::var("HOME").expect("HOME var not exists") + "/.config/wifi-check-tui"
});

enum AppState {
    Monitoring,
    Main,
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let log_path = format!("{}/{}", LINUX_CONFIG.as_str(), "run.log");
    let log_file: &Path = Path::new(&log_path);
    if !log_file.exists() {
        let _ = fs::create_dir(log_file);
    }

    // You can use info/debug/error loggers for logging and you're logs will be writing to file
    let _ = simple_logging::log_to_file(
        format!("{}/{}", LINUX_CONFIG.as_str(), "run.log"),
        log::LevelFilter::Info,
    );

    info!("createing socket");
    let mut socket: AsyncSocket = AsyncSocket::connect().expect("device not found");
    let mut state: AppState = AppState::Main;

    info!("app started..");
    enable_raw_mode()?;
    let stdout: Stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let _ = terminal.clear();

    let mut _running: bool = true;

    // add to app fern logger in the future

    loop {
        match state {
            AppState::Main => {
                terminal.draw(|f| {
                    // Create a vertical layout with 2 sections
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [Constraint::Percentage(80), Constraint::Percentage(20)].as_ref(),
                        )
                        .split(f.size());

                    // Create a block with borders
                    let block = Block::default().title("Main").borders(Borders::ALL);

                    let paragraph =
                        Paragraph::new("Press 'esc' to quit\nPress 'm' to change state")
                            .block(Block::default().borders(Borders::ALL));

                    // Render the widgets in their respective chunks
                    f.render_widget(block, chunks[0]);
                    f.render_widget(paragraph, chunks[1]);
                })?;
            }
            AppState::Monitoring => {
                let wifi_interface = socket.get_interfaces_info().await.unwrap();
                let widget = create_device(&wifi_interface, &mut socket).await;
                terminal.draw(|f| {
                    let block: Block = Block::default().title("Monitoring").borders(Borders::ALL);

                    f.render_widget(block, f.size());
                    f.render_widget(widget, f.size());
                })?;
            }
        }

        if let Some(key) = event::read()?.as_key_press_event() {
            if key.code == KeyCode::Esc {
                _running = false;
                info!("exiting..");
                break;
            }
            if key.code == KeyCode::Char('Q') {
                _running = false;
                info!("exiting..");
                break;
            }
            if key.code == KeyCode::Char('m') {
                info!("chagning state to Monitoring..");
                state = AppState::Monitoring;
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

async fn create_device<'a>(intf: &[Interface], sock: &mut AsyncSocket) -> Paragraph<'a> {
    let mut text: Vec<Spans> = Vec::with_capacity(intf.len() + 2);
    for interface in intf {
        if let Some(indx) = interface.name.as_ref() {
            let bss = sock.get_bss_info(interface.index.unwrap()).await.unwrap();
            let span = Spans::from(vec![Span::styled(
                String::from_utf8(indx.to_vec()).unwrap(),
                Style::default().add_modifier(Modifier::BOLD),
            )]);
            let signal_span = Spans::from(vec![
                Span::raw("Connection"),
                Span::styled(
                    format!(" {} ", bss[0].signal.ok_or_else(|| 0).unwrap() / 100),
                    Style::default(),
                ),
                Span::styled("dBm", Style::default().add_modifier(Modifier::ITALIC)),
            ]);
            text.extend([span, signal_span]);
        }
    }
    Paragraph::new(text).block(Block::default().borders(Borders::ALL))
}
