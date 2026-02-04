#[derive(Copy, Clone, Debug)]
pub enum AppState<'a> {
    Monitoring,
    Main,
    Error { h: &'a str, d: &'a str },
}

impl<'a> std::fmt::Display for AppState<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppState::Monitoring => write!(f, "Monitoring"),
            AppState::Main => write!(f, "Main"),
            AppState::Error { h, d } => write!(f, "Error header {}; description {}", h, d),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProgramState<'a> {
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
