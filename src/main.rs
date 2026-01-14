use chrono::Local;
use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Stdout},
    path::Path,
    process::exit,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::{
    sync::{Mutex, RwLock, oneshot},
    time::interval,
};

use crossterm::{
    event::{self, DisableMouseCapture, KeyCode},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(target_os = "windows")]
use directories::UserDirs;
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

#[cfg(target_os = "linux")]
static CONFIGURATION: LazyLock<String> = LazyLock::new(|| {
    std::env::var("HOME").expect("HOME var not exists") + "/.config/wifi-check-tui"
});

#[cfg(target_os = "windows")]
static CONFIGURATION: LazyLock<String> =
    LazyLock::new(|| UserDirs::home_dir() + "\\wifi-check-tui");

#[derive(Copy, Clone, Debug)]
enum AppState<'a> {
    Monitoring,
    Main,
    Error { h: &'a str, d: &'a str },
}

#[derive(Clone, Copy, Debug)]
struct ProgramState<'a> {
    pub hide_info: bool,
    pub running: bool,
    pub state: AppState<'a>,
}

impl<'a> ProgramState<'a> {
    /// Changes state for ProgramState
    pub fn change_state(&mut self, s: AppState<'a>) {
        self.state = s;
    }

    pub fn change_running(&mut self) {
        self.running = !self.running;
    }

    pub fn toggle_hide_info(&mut self) {
        self.hide_info = !self.hide_info;
    }
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let log_file: &Path = Path::new(CONFIGURATION.as_str());
    if !log_file.exists() {
        let _ = fs::create_dir(log_file);
    }

    // You can use info/debug/error loggers for logging and you're logs will be writing to file
    let _ = simple_logging::log_to_file(
        format!("{}/run-{}.log", CONFIGURATION.as_str(), Local::now()),
        log::LevelFilter::Info,
    );

    info!("createing socket");
    let mut socket: AsyncSocket = AsyncSocket::connect().expect("device not found");
    let state: Arc<Mutex<ProgramState>> = Arc::new(Mutex::new(ProgramState {
        hide_info: true,
        running: true,
        state: AppState::Main,
    }));

    info!("app started..");
    enable_raw_mode()?;
    let stdout: Stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let _ = terminal.clear();

    let mut interval = interval(Duration::from_secs(1));

    let state_clone = state.clone();

    let input_thread = tokio::task::spawn(async move {
        let state_task = state_clone.clone();
        while state_task.lock().await.running {
            if let Some(key) = event::read().unwrap().as_key_press_event() {
                info!("{}", key.code);
                let mut st = state_task.lock().await;
                if key.code == KeyCode::Esc {
                    info!("exiting..");
                    st.change_running();
                }
                if key.code == KeyCode::Char('q') {
                    info!("exiting..");
                    st.change_running();
                }
                if key.code == KeyCode::Char('m') {
                    info!("chagning state to Monitoring..");
                    st.change_state(AppState::Monitoring);
                }
                if key.code == KeyCode::Char('h') {
                    info!("changed hide boolean");
                    st.toggle_hide_info();
                }
                if key.code == KeyCode::Char('u') {
                    info!("updating screen");
                    st.change_state(AppState::Monitoring);
                }
            }
        }
    });

    #[cfg(debug_assertions)]
    let mut counter: u8 = 0;

    // need add to app fern logger in the future
    while state.clone().lock().await.running {
        interval.tick().await;

        #[cfg(debug_assertions)]
        {
            counter += 1;
            if counter == 30 {
                state.clone().lock().await.change_running();
            }
        }

        let program_state = *state.clone().lock().await;
        match program_state.state {
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
                        .constraints([
                            Constraint::Percentage(10),
                            Constraint::Percentage(80),
                            Constraint::Percentage(10),
                        ])
                        .split(f.size());
                    let header_chunk = chunks[0];
                    let description_chunk = chunks[1];
                    let keybind_chunk = chunks[2];

                    let header_paragraph =
                        Paragraph::new(h).block(Block::default().borders(Borders::ALL));
                    let keybind_paragraph = Paragraph::new("For update menu press 'u'")
                        .block(Block::default().title("hint").borders(Borders::ALL));
                    let description_paragraph =
                        Paragraph::new(Span::styled(d, Style::default().fg(Color::Red)))
                            .block(Block::default().borders(Borders::ALL));

                    f.render_widget(header_paragraph, header_chunk);
                    f.render_widget(description_paragraph, description_chunk);
                    f.render_widget(keybind_paragraph, keybind_chunk);
                })?;
            }
            AppState::Monitoring => {
                let wifi_interface = socket.get_interfaces_info().await.unwrap();
                if wifi_interface.len() == 1 {
                    state.lock().await.change_state(AppState::Error {
                        h: "wifi interface error",
                        d: "wifi interface is not existed",
                    });
                }
                let widget =
                    match create_device(&wifi_interface, &mut socket, state.lock().await.hide_info)
                        .await
                    {
                        Ok(t) => t,
                        Err(e) => {
                            state.lock().await.change_state(e);
                            continue;
                        }
                    };
                let hide_text = if program_state.hide_info {
                    "For show mac address press 'h'"
                } else {
                    "For hide mac address press 'h'"
                };
                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [Constraint::Percentage(80), Constraint::Percentage(20)].as_ref(),
                        )
                        .split(f.size());

                    let hide_paragraph = Paragraph::new(hide_text)
                        .block(Block::default().title("hint").borders(Borders::ALL));

                    f.render_widget(widget, chunks[0]);
                    f.render_widget(hide_paragraph, chunks[1]);
                })?;
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
    Ok(Paragraph::new(text).block(Block::default().title("monitoring").borders(Borders::ALL)))
}

/// Returns Color for signal level.
///
/// # Example
///
/// ```
/// use tui::style::Color;
///
/// let signal: i32 = 40;
///
/// // Returns green color for good internet connection level
/// let color_for_signal: Color = get_color_for_signal(signal);
/// ```
/// # And example for bad connection
/// ```
/// let signal: i32 = 120;
///
/// // And returns red color for bad internet connection level
/// let color_for_signal: Color = get_color_for_signal(signal);
/// ```
fn get_color_for_signal(signal: i32) -> Color {
    match signal {
        0..=60 => Color::Green,
        61..=100 => Color::Yellow,
        _ => Color::Red,
    }
}

/// Returns a information in numbers for secutiry info
/// # Example
///
/// ```
/// // second useless rgument, but if u don't need security - use false
/// let info: String = get_secutiry_info("information", true);
/// ```
fn get_security_info(inf: &str, sec: bool) -> String {
    let mut s = DefaultHasher::new();
    if sec {
        inf.hash(&mut s);
        let res: String = s.finish().to_string();
        return res;
    }
    inf.to_string()
}
