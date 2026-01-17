use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::fs::File;
use std::io;
use std::process::{Child, Command, Stdio};

const SAMPLE_RATE: &str = "48000";
const SAMPLE_FORMAT: &str = "s32le";
const CHANNELS: &str = "mono";

#[derive(Clone)]
struct Effect {
    name: &'static str,
    defaults: [f32; 4],
    pots: [&'static str; 4],
    desc: &'static str,
}

const EFFECTS: &[Effect] = &[
    Effect {
        name: "flanger",
        defaults: [0.6, 0.6, 0.6, 0.6],
        pots: ["Depth", "Rate", "Feedback", "Mix"],
        desc: "Modulated delay - jet-plane swoosh",
    },
    Effect {
        name: "echo",
        defaults: [0.3, 0.3, 0.3, 0.3],
        pots: ["Delay", "Feedback", "Mix", "Tone"],
        desc: "Delay loop up to 1.25 seconds",
    },
    Effect {
        name: "fm",
        defaults: [0.25, 0.25, 0.5, 0.5],
        pots: ["Mod Depth", "Mod Rate", "Carrier", "Mix"],
        desc: "Frequency modulation synthesis",
    },
    Effect {
        name: "am",
        defaults: [0.5, 0.5, 0.5, 0.5],
        pots: ["Depth", "Rate", "Shape", "Mix"],
        desc: "Amplitude modulation",
    },
    Effect {
        name: "phaser",
        defaults: [0.3, 0.3, 0.5, 0.5],
        pots: ["Depth", "Rate", "Stages", "Feedback"],
        desc: "All-pass filter sweep",
    },
    Effect {
        name: "discont",
        defaults: [0.8, 0.1, 0.2, 0.2],
        pots: ["Pitch", "Rate", "Blend", "Mix"],
        desc: "Pitch shift via crossfade",
    },
];

struct App {
    effect_idx: usize,
    pot_idx: usize,
    pot_values: Vec<[f32; 4]>,
    status: String,
    status_ok: bool,
    list_state: ListState,
    player: Option<Child>,
}

impl App {
    fn new() -> Self {
        let pot_values = EFFECTS.iter().map(|e| e.defaults).collect();
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        let mut app = Self {
            effect_idx: 0,
            pot_idx: 0,
            pot_values,
            status: String::new(),
            status_ok: true,
            list_state,
            player: None,
        };
        app.check_environment();
        app
    }

    fn check_environment(&mut self) {
        if !std::path::Path::new("../convert").exists() 
            && !std::path::Path::new("./convert").exists() {
            self.status = "Warning: 'convert' not found. Run 'make convert' first.".to_string();
            self.status_ok = false;
        } else if !std::path::Path::new("../input.raw").exists() 
            && !std::path::Path::new("./input.raw").exists() {
            self.status = "No input.raw found - will try to convert MP3".to_string();
            self.status_ok = true;
        } else {
            self.status = "Ready - press 'p' to process, 'q' to quit".to_string();
            self.status_ok = true;
        }
    }

    fn next_effect(&mut self) {
        self.effect_idx = (self.effect_idx + 1) % EFFECTS.len();
        self.list_state.select(Some(self.effect_idx));
        self.pot_idx = 0;
    }

    fn prev_effect(&mut self) {
        self.effect_idx = if self.effect_idx == 0 {
            EFFECTS.len() - 1
        } else {
            self.effect_idx - 1
        };
        self.list_state.select(Some(self.effect_idx));
        self.pot_idx = 0;
    }

    fn next_pot(&mut self) {
        self.pot_idx = (self.pot_idx + 1) % 4;
    }

    fn increase_pot(&mut self) {
        let idx = self.pot_idx;
        let eff_idx = self.effect_idx;
        self.pot_values[eff_idx][idx] = (self.pot_values[eff_idx][idx] + 0.05).min(1.0);
    }

    fn decrease_pot(&mut self) {
        let idx = self.pot_idx;
        let eff_idx = self.effect_idx;
        self.pot_values[eff_idx][idx] = (self.pot_values[eff_idx][idx] - 0.05).max(0.0);
    }

    fn reset_pots(&mut self) {
        let defaults = EFFECTS[self.effect_idx].defaults;
        self.pot_values[self.effect_idx] = defaults;
        self.status = format!("Reset {} to defaults", EFFECTS[self.effect_idx].name);
        self.status_ok = true;
    }

    fn stop_audio(&mut self) {
        if let Some(ref mut child) = self.player {
            let _ = child.kill();
        }
        self.player = None;
    }

    fn process_and_play(&mut self) {
        let effect_name = EFFECTS[self.effect_idx].name.to_string();
        let effect_pots = self.pot_values[self.effect_idx];
        
        self.status = format!("Processing {}...", effect_name);
        self.status_ok = true;

        let (convert_path, input_path, output_path) = 
            if std::path::Path::new("../convert").exists() {
                ("../convert", "../input.raw", "../output.raw")
            } else {
                ("./convert", "./input.raw", "./output.raw")
            };

        if !std::path::Path::new(input_path).exists() {
            let mp3_path = if std::path::Path::new("../BassForLinus.mp3").exists() {
                "../BassForLinus.mp3"
            } else if std::path::Path::new("./BassForLinus.mp3").exists() {
                "./BassForLinus.mp3"
            } else {
                self.status = "Error: No input.raw or .mp3 file found".to_string();
                self.status_ok = false;
                return;
            };

            let result = Command::new("ffmpeg")
                .args(["-y", "-v", "fatal", "-i", mp3_path,
                       "-f", SAMPLE_FORMAT, "-ar", SAMPLE_RATE, "-ac", "1", input_path])
                .status();

            if result.is_err() || !result.unwrap().success() {
                self.status = "Error: Failed to convert MP3".to_string();
                self.status_ok = false;
                return;
            }
        }

        if !std::path::Path::new(convert_path).exists() {
            self.status = "Error: 'convert' not found - run 'make convert'".to_string();
            self.status_ok = false;
            return;
        }

        let input_file = match File::open(input_path) {
            Ok(f) => f,
            Err(e) => {
                self.status = format!("Error opening input: {}", e);
                self.status_ok = false;
                return;
            }
        };

        let output_file = match File::create(output_path) {
            Ok(f) => f,
            Err(e) => {
                self.status = format!("Error creating output: {}", e);
                self.status_ok = false;
                return;
            }
        };

        let result = Command::new(convert_path)
            .arg(&effect_name)
            .args(effect_pots.iter().map(|p| format!("{:.2}", p)))
            .stdin(Stdio::from(input_file))
            .stdout(Stdio::from(output_file))
            .status();

        match result {
            Ok(status) if status.success() => {
                self.stop_audio();
                
                self.player = Command::new("ffplay")
                    .args(["-v", "fatal", "-nodisp", "-autoexit",
                           "-f", SAMPLE_FORMAT, "-ar", SAMPLE_RATE,
                           "-ch_layout", CHANNELS, "-i", output_path])
                    .spawn()
                    .ok();

                self.status = format!(
                    "Playing: {} [{:.2}, {:.2}, {:.2}, {:.2}]",
                    effect_name, effect_pots[0], effect_pots[1], effect_pots[2], effect_pots[3]
                );
                self.status_ok = true;
            }
            _ => {
                self.status = "Error: Processing failed".to_string();
                self.status_ok = false;
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            app.stop_audio();
                            break;
                        }
                        KeyCode::Up | KeyCode::Char('k') => app.prev_effect(),
                        KeyCode::Down | KeyCode::Char('j') => app.next_effect(),
                        KeyCode::Tab => app.next_pot(),
                        KeyCode::Left | KeyCode::Char('h') => app.decrease_pot(),
                        KeyCode::Right | KeyCode::Char('l') => app.increase_pot(),
                        KeyCode::Char('p') | KeyCode::Char('P') => app.process_and_play(),
                        KeyCode::Char('r') | KeyCode::Char('R') => app.reset_pots(),
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            app.stop_audio();
                            app.status = "Stopped playback".to_string();
                            app.status_ok = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(f.area());

    let title = Paragraph::new("=== AUDIONOISE TUI ===")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = EFFECTS
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let style = if i == app.effect_idx {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let marker = if i == app.effect_idx { "> " } else { "  " };
            ListItem::new(format!("{}{}", marker, e.name.to_uppercase())).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("EFFECTS"));
    f.render_stateful_widget(list, chunks[1], &mut app.list_state);

    let effect = &EFFECTS[app.effect_idx];
    let pots = &app.pot_values[app.effect_idx];
    
    let mut pot_lines: Vec<Line> = vec![
        Line::from(Span::styled(effect.desc, Style::default().fg(Color::Gray))),
        Line::from(""),
    ];

    for i in 0..4 {
        let selected = i == app.pot_idx;
        let value = pots[i];
        let name = effect.pots[i];
        
        let bar_width = 20;
        let filled = (value * bar_width as f32) as usize;
        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(bar_width - filled));

        let style = if selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        pot_lines.push(Line::from(vec![
            Span::styled(format!(" {:12}", name), style),
            Span::styled(bar, if selected { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Blue) }),
            Span::styled(format!(" {:.2}", value), style),
        ]));
    }

    let pots_widget = Paragraph::new(pot_lines)
        .block(Block::default().borders(Borders::ALL).title(format!("POTS - {}", effect.name.to_uppercase())));
    f.render_widget(pots_widget, chunks[2]);

    let controls = Paragraph::new("Up/Down: effect | Tab: pot | Left/Right: value | p: play | s: stop | r: reset | q: quit")
        .style(Style::default().fg(Color::Gray))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(controls, chunks[3]);

    let status_style = if app.status_ok {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };
    let status = Paragraph::new(app.status.as_str()).style(status_style);
    f.render_widget(status, chunks[4]);
}
