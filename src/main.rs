use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
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
use macaddr::MacAddr6;
use neli_wifi::{AsyncSocket, Interface};
use tui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
};

static LINUX_CONFIG: LazyLock<String> = LazyLock::new(|| {
    std::env::var("HOME").expect("HOME var not exists") + "/.config/wifi-check-tui"
});

enum AppState<'a> {
    Monitoring,
    Main,
    Error { h: &'a str, d: &'a str },
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
    let mut hide_info = true;

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
            AppState::Error { h, d } => {
                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
                        .split(f.size());
                    let header_chunk = chunks[0];
                    let description_chunk = chunks[1];

                    let header_paragraph =
                        Paragraph::new(h).block(Block::default().borders(Borders::ALL));
                    let description_paragraph =
                        Paragraph::new(Span::styled(d, Style::default().fg(Color::Red)))
                            .block(Block::default().borders(Borders::ALL));

                    f.render_widget(header_paragraph, header_chunk);
                    f.render_widget(description_paragraph, description_chunk);
                })?;
            }
            AppState::Monitoring => {
                let wifi_interface = socket.get_interfaces_info().await.unwrap();
                if wifi_interface.len() == 1 {
                    state = AppState::Error {
                        h: "wifi interface error",
                        d: "wifi interface is not existed",
                    };
                }
                let widget = match create_device(&wifi_interface, &mut socket, hide_info).await {
                    Ok(t) => t,
                    Err(e) => {
                        state = e;
                        continue;
                    }
                };
                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [Constraint::Percentage(80), Constraint::Percentage(20)].as_ref(),
                        )
                        .split(f.size());

                    let block: Block = Block::default().title("Monitoring").borders(Borders::ALL);
                    let hide_text = Paragraph::new("For hide mac address press 'h'")
                        .block(Block::default().borders(Borders::ALL));

                    f.render_widget(block, chunks[0]);
                    f.render_widget(widget, chunks[0]);
                    f.render_widget(hide_text, chunks[1]);
                })?;
            }
        }

        if let Some(key) = event::read()?.as_key_press_event() {
            if key.code == KeyCode::Esc {
                _running = false;
                info!("exiting..");
                break;
            }
            if key.code == KeyCode::Char('q') {
                _running = false;
                info!("exiting..");
                break;
            }
            if key.code == KeyCode::Char('m') {
                info!("chagning state to Monitoring..");
                state = AppState::Monitoring;
            }
            if key.code == KeyCode::Char('h') {
                info!("changed hide boolean");
                hide_info = !hide_info;
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
    let _ = terminal.clear();

    Ok(())
}

async fn create_device<'a>(
    intf: &[Interface],
    sock: &mut AsyncSocket,
    hide_info: bool,
) -> Result<Paragraph<'a>, AppState<'a>> {
    let mut text: Vec<Spans> = Vec::with_capacity(intf.len() + 2);
    for interface in intf {
        if let Some(indx) = interface.name.as_ref()
            && let Some(bss) = sock
                .get_bss_info(interface.index.unwrap())
                .await
                .unwrap()
                .first_mut()
        {
            let status = match bss.status {
                Some(stat) => stat,
                None => {
                    return Err(AppState::Error {
                        h: "Internet connection failed",
                        d: "Internet connection do not exists",
                    });
                }
            };
            let span = Spans::from(vec![Span::styled(
                String::from_utf8(indx.to_vec()).unwrap(),
                Style::default().add_modifier(if status == 1 {
                    Modifier::BOLD
                } else {
                    Modifier::DIM
                }),
            )]);

            let mut signal: i32 = 0;
            if let Some(sig) = bss.signal {
                signal = sig / 100;
            }

            info!(
                "frequency {} beacon_interval {} seen_ms_ago {}",
                bss.frequency.unwrap(),
                bss.beacon_interval.unwrap(),
                bss.seen_ms_ago.unwrap()
            );
            if let Some(m) = interface.mac.as_ref() {
                let addr: [u8; 6] = m.as_slice().try_into().unwrap();
                let mac = get_security_info(&MacAddr6::from(addr).to_string(), hide_info);

                info!(
                    "mac {} channel {} power {} phy {} device {}",
                    mac,
                    interface.channel.unwrap(),
                    interface.power.unwrap(),
                    interface.phy.unwrap(),
                    interface.device.unwrap()
                );

                let signal_span = Spans::from(vec![
                    Span::raw("Connection"),
                    Span::styled(
                        format!(" {} ", signal),
                        Style::default().fg(get_color_for_signal(signal.abs())),
                    ),
                    Span::styled("dBm", Style::default().add_modifier(Modifier::ITALIC)),
                ]);

                let mac_span = Spans::from(vec![
                    Span::raw("Mac address"),
                    Span::styled(
                        format!(" {} ", get_security_info(&mac.to_string(), hide_info)),
                        Style::default().fg(Color::Green),
                    ),
                ]);
                text.extend([span, signal_span, mac_span]);
            }
        }
    }
    Ok(Paragraph::new(text).block(Block::default().borders(Borders::ALL)))
}

fn get_color_for_signal(signal: i32) -> Color {
    match signal {
        0..=60 => Color::Green,
        61..=100 => Color::Yellow,
        _ => Color::Red,
    }
}

fn get_security_info(inf: &str, sec: bool) -> String {
    let mut s = DefaultHasher::new();
    if sec {
        inf.hash(&mut s);
        let res: String = s.finish().to_string();
        return res;
    }
    inf.to_string()
}
