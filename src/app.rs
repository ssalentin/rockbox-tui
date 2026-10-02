use crate::ipod::{self};
use crate::ui;
use crate::workflow::{self, Event as WEvent, InstallOptions};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::sync::mpsc;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct FoundIpod {
    pub path: String,
    pub ipod: ipod::Ipod,
}

enum WorkerMsg {
    Event(WEvent),
    Done(Result<(), String>),
}

pub struct App {
    pub devices: Vec<FoundIpod>,
    pub selected: usize,
    pub denied: usize,

    pub logs: Vec<String>,
    pub log_follow: bool,
    pub log_scroll: usize,
    pub log_view: std::cell::Cell<usize>,

    pub running: bool,
    pub step: Option<String>,
    pub progress: Option<(u64, Option<u64>)>,
    pub status_msg: Option<String>,

    pub frame: u64,
    pub should_quit: bool,
    pub show_help: bool,
    /// A confirmation dialog is shown before the install actually starts.
    pub confirm: bool,

    worker_rx: Option<mpsc::Receiver<WorkerMsg>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl App {
    pub fn new() -> App {
        let (devices, denied) = ipod::io::scan();
        let devices: Vec<FoundIpod> = devices
            .into_iter()
            .map(|(path, ipod)| FoundIpod { path, ipod })
            .collect();
        let mut logs = vec!["Scanning for iPods…".to_string()];
        if devices.is_empty() {
            if denied > 0 {
                logs.push(format!(
                    "{denied} disk(s) need root access — run rockbox-tui with sudo to see them."
                ));
            }
            logs.push("No iPods found. Plug one in and press r to rescan.".to_string());
        } else {
            logs.push(format!("Found {} iPod(s).", devices.len()));
        }

        App {
            devices,
            selected: 0,
            denied,
            logs,
            log_follow: true,
            log_scroll: 0,
            log_view: std::cell::Cell::new(0),
            running: false,
            step: None,
            progress: None,
            status_msg: None,
            frame: 0,
            should_quit: false,
            show_help: false,
            confirm: false,
            worker_rx: None,
            worker_handle: None,
        }
    }

    pub fn selected_device(&self) -> Option<&FoundIpod> {
        self.devices.get(self.selected)
    }

    pub fn spinner(&self) -> &'static str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        FRAMES[(self.frame as usize / 3) % FRAMES.len()]
    }

    fn drain_worker(&mut self) {
        let mut msgs = Vec::new();
        if let Some(rx) = &self.worker_rx {
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
        }
        for msg in msgs {
            match msg {
                WorkerMsg::Event(ev) => match ev {
                    WEvent::Log(s) => self.push_log(s),
                    WEvent::Step(s) => {
                        self.step = Some(s.clone());
                        self.push_log(format!("== {s}"));
                    }
                    WEvent::Progress { done, total } => self.progress = Some((done, total)),
                },
                WorkerMsg::Done(res) => {
                    self.running = false;
                    self.progress = None;
                    self.step = None;
                    match res {
                        Ok(()) => {
                            self.status_msg = Some("Rockbox installed successfully".into());
                            self.push_log("Installation complete.".into());
                        }
                        Err(e) => {
                            self.status_msg = Some("Installation failed — see logs".into());
                            self.push_log(format!("ERROR: {e}"));
                        }
                    }
                }
            }
        }
    }

    fn push_log(&mut self, s: String) {
        self.logs.push(s);
        if self.logs.len() > 2000 {
            let overflow = self.logs.len() - 2000;
            self.logs.drain(0..overflow);
        }
    }

    fn rescan(&mut self) {
        if self.running {
            return;
        }
        self.devices.clear();
        let (devices, denied) = ipod::io::scan();
        self.devices = devices
            .into_iter()
            .map(|(path, ipod)| FoundIpod { path, ipod })
            .collect();
        self.denied = denied;
        self.selected = 0;
        self.push_log(format!("Rescan: {} iPod(s) found.", self.devices.len()));
    }

    fn start_install(&mut self) {
        if self.running {
            return;
        }
        let Some(dev) = self.selected_device() else {
            self.status_msg = Some("no iPod selected".into());
            return;
        };
        let opts = InstallOptions {
            device: dev.path.clone(),
            firmware: None,
            bootloader: None,
            target: None,
            backup_dir: None,
        };

        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let handle = std::thread::spawn(move || {
            let mut on = |ev: WEvent| {
                let _ = tx2.send(WorkerMsg::Event(ev));
            };
            let res = workflow::install(&opts, &mut on);
            let _ = tx.send(WorkerMsg::Done(res));
        });

        self.running = true;
        self.step = None;
        self.progress = None;
        self.status_msg = None;
        self.worker_rx = Some(rx);
        self.worker_handle = Some(handle);
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Char('h')) {
                self.show_help = false;
            }
            return;
        }
        if self.confirm {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.confirm = false;
                    self.start_install();
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.confirm = false;
                    self.status_msg = Some("cancelled".into());
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.running {
                    self.status_msg = Some("installation in progress — cannot quit".into());
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.running {
                    self.status_msg = Some("installation in progress — cannot quit".into());
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('?') | KeyCode::Char('h') => self.show_help = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.devices.is_empty() {
                    self.selected = (self.selected + 1) % self.devices.len();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.devices.is_empty() {
                    self.selected = (self.selected + self.devices.len() - 1) % self.devices.len();
                }
            }
            KeyCode::Char('s') | KeyCode::Enter => {
                if self.selected_device().is_some() {
                    self.confirm = true;
                } else {
                    self.status_msg = Some("no iPod selected".into());
                }
            }
            KeyCode::Char('r') => self.rescan(),
            KeyCode::Char('f') => {
                self.log_follow = !self.log_follow;
                self.log_scroll = 0;
            }
            KeyCode::Char('x') => {
                self.logs.clear();
                self.log_scroll = 0;
            }
            KeyCode::Char('u') => self.scroll_logs(-10),
            KeyCode::Char('d') => self.scroll_logs(10),
            _ => {}
        }
    }

    fn scroll_logs(&mut self, delta: i32) {
        if self.log_follow {
            if delta > 0 {
                return;
            }
            let page = self.log_view.get().max(1);
            self.log_scroll = self.logs.len().saturating_sub(page);
            self.log_follow = false;
        }
        let max = self.logs.len().saturating_sub(1) as i32;
        self.log_scroll = ((self.log_scroll as i32 + delta).clamp(0, max)) as usize;
    }

    pub fn run(&mut self) -> Result<()> {
        let mut terminal = ratatui::init();
        let res = (|| -> Result<()> {
            loop {
                self.drain_worker();
                self.frame += 1;
                terminal.draw(|f| ui::draw(f, self))?;
                if self.should_quit {
                    break;
                }
                if event::poll(Duration::from_millis(40))? {
                    if let Event::Key(key) = event::read()? {
                        self.on_key(key);
                    }
                }
            }
            Ok(())
        })();
        ratatui::restore();
        // If a worker is still running on quit, join it so it isn't orphaned.
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
        res
    }
}
