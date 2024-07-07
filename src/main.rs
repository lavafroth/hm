use anyhow::Result;
use portable_pty::unix::UnixPtySystem;
use ratatui::layout::Constraint;
use ratatui_explorer::{FileExplorer, Theme};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};
use std::{
    io::{self, BufWriter},
    sync::{Arc, RwLock},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify::{EventHandler, RecursiveMode, Watcher};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::Alignment,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use tui_term::vt100;
use tui_term::vt100::Screen;
use tui_term::widget::PseudoTerminal;

#[derive(Debug, Clone)]
struct Size {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chord {
    FilePicker,
    ReRender,
    SetQuality,
}

#[derive(Debug, Default)]
enum Quality {
    #[default]
    L,
    M,
    H,
    P,
    K,
}

impl ToString for Quality {
    fn to_string(&self) -> String {
        match self {
            Quality::L => "480p",
            Quality::M => "720p",
            Quality::H => "1080p",
            Quality::P => "1440p",
            Quality::K => "4K",
        }
        .to_string()
    }
}

impl Quality {
    fn symbol(&self) -> &'static str {
        match self {
            Quality::L => "l",
            Quality::M => "m",
            Quality::H => "h",
            Quality::P => "p",
            Quality::K => "k",
        }
    }
}

struct Model {
    prev_was_space: Option<()>,
    chord: Option<Chord>,
    quality: Quality,
    directory: PathBuf,
    pty_system: NativePtySystem,
    file_picker: Option<FileExplorer>,
    last_file: String,
    last_time: Instant,
    size: Size,
}

impl Model {
    fn pty_size(&self) -> Size {
        Size {
            rows: self.size.rows - 6,
            cols: self.size.cols - 4,
        }
    }

    fn ingest_chord(&mut self, keycode: KeyCode) {
        self.chord = match keycode {
            KeyCode::Char('f') => {
                let fp = FileExplorer::new().unwrap();
                self.file_picker = Some(fp);
                Some(Chord::FilePicker)
            }
            KeyCode::Char('q') => Some(Chord::SetQuality),
            KeyCode::Char('r') => Some(Chord::ReRender),
            _ => None,
        };
    }

    fn set_quality(&mut self, keycode: KeyCode) {
        self.quality = match keycode {
            KeyCode::Char('m') => Quality::M,
            KeyCode::Char('h') => Quality::H,
            KeyCode::Char('p') => Quality::P,
            KeyCode::Char('k') => Quality::K,
            _ => Quality::L,
        };
    }

    fn render(&mut self, parser: Arc<RwLock<vt100::Parser>>, name: &str) -> Result<()> {
        let mut cmd = CommandBuilder::new("manim");

        cmd.args([
            "render",
            "--preview",
            "--preview_command",
            "mpv",
            "--quality",
            self.quality.symbol(),
            &name,
        ]);
        cmd.cwd(&self.directory);

        let Size { rows, cols } = self.pty_size();

        let pair = self.pty_system.openpty(PtySize {
            rows,
            cols,
            ..Default::default()
        })?;

        {
            let parser = parser.clone();
            let mut child = pair.slave.spawn_command(cmd)?;
            let mut reader = pair.master.try_clone_reader()?;
            std::thread::spawn(move || {
                drop(pair.slave);

                // Consume the output from the child
                let mut buf = [0u8; 8192];

                loop {
                    match reader.read(&mut buf).unwrap() {
                        0 => break,
                        size => parser.write().unwrap().process(&buf[..size]),
                    }
                }
                // Drop writer on purpose
                pair.master.take_writer().unwrap();

                let exit = child.wait();
                if let Ok(exit) = exit {
                    parser.write().unwrap().process(
                        format!("\r\nmanim exited with code: {}\r\n", exit.exit_code()).as_bytes(),
                    );
                }

                drop(pair.master);
            });
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let (mut terminal, size) = setup_terminal()?;

    let mut model = Model {
        directory: std::env::current_dir()?,
        last_time: Instant::now(),
        prev_was_space: None,
        chord: None,
        quality: Quality::L,
        pty_system: UnixPtySystem::default(),
        file_picker: None,
        last_file: String::default(),
        size,
    };

    run(&mut terminal, &mut model)?;

    cleanup_terminal(&mut terminal)?;
    Ok(())
}

pub struct EventSender {
    tx: Sender<String>,
}

impl EventHandler for EventSender {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        match event {
            Ok(event) => {
                if let notify::EventKind::Modify(notify::event::ModifyKind::Data(_)) = event.kind {
                    for path in event
                        .paths
                        .into_iter()
                        .filter(|p| p.extension().is_some_and(|ext| ext == "py"))
                    {
                        // re-render them
                        self.tx
                            .send(path.to_string_lossy().to_string())
                            .expect("welp that shouldn't have happened");
                    }
                }
            }
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn run<B: Backend>(terminal: &mut Terminal<B>, model: &mut Model) -> Result<()> {
    let Size { cols, rows } = model.pty_size();
    let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, 0)));

    let (tx, rx) = channel();
    let es = EventSender { tx };
    // Automatically select the best implementation for your platform.
    let mut watcher = notify::recommended_watcher(es)?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(&model.directory, RecursiveMode::Recursive)?;
    let tick_rate = Duration::from_millis(10);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, parser.read().unwrap().screen(), model))?;
        let t_size = terminal.get_frame().size();
        model.size = Size {
            cols: t_size.width,
            rows: t_size.height,
        };
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if let Ok(name) = rx.try_recv() {
            if model.last_file == name && model.last_time.elapsed() < Duration::from_millis(500) {
                continue;
            }
            model.render(parser.clone(), &name)?;
            model.last_file = name;
            model.last_time = Instant::now();
        }

        if event::poll(timeout)? {
            let read_event = event::read()?;

            // Send keystrokes to file picker if we are in a file picker view
            if let Some(picker) = model.file_picker.as_mut() {
                if let Event::Key(key) = read_event {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char(' ') => {
                                watcher.unwatch(&model.directory)?;
                                model.directory = picker.cwd().to_owned();
                                watcher.watch(&model.directory, RecursiveMode::Recursive)?;
                                model.file_picker = None;
                                continue;
                            }
                            KeyCode::Esc => {
                                model.file_picker = None;
                                continue;
                            }
                            _ => (),
                        }
                    }
                }
                picker.handle(&read_event)?;
                continue;
            }

            if let Event::Key(key) = read_event {
                if key.kind == KeyEventKind::Press {
                    // taking the inner value makes it None again
                    if model.prev_was_space.take().is_some() {
                        model.ingest_chord(key.code);
                    } else if model.chord.take() == Some(Chord::SetQuality) {
                        model.set_quality(key.code);
                    } else {
                        match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char(' ') => model.prev_was_space = Some(()),
                            _ => (),
                        }
                    }
                }
            }

            // Run the rendering command
            if let Some(Chord::ReRender) = model.chord {
                let name = model.last_file.clone();
                if name.is_empty() {
                    continue;
                }
                model.render(parser.clone(), &name)?;
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

fn truncate_string_by(s: &str, by: usize) -> String {
    if by == 0 {
        return s.to_string();
    }
    let cut_len = by / 2;
    let center = s.len() / 2;
    let left = center.saturating_sub(cut_len + 4);
    let right = center.saturating_add(cut_len + 4);
    let left = left.min(center);
    let right = right.max(center);
    format!("{}[...]{}", &s[..left], &s[right..])
}

fn truncate_string_to(s: &str, to: usize) -> String {
    let by = s.len().saturating_sub(to);
    truncate_string_by(s, by)
}

fn main_ui(f: &mut Frame, screen: &Screen, model: &mut Model) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(2),
            Constraint::Percentage(100),
            Constraint::Min(1),
        ])
        .split(f.size());
    let title = Line::from(" manim output ");
    let block = Block::default().borders(Borders::ALL).title(title);
    let pseudo_term = PseudoTerminal::new(screen).block(block);
    f.render_widget(pseudo_term, chunks[1]);

    let dir = model.directory.to_string_lossy().into_owned();
    let status_line_len = format!(
        "rendering files in {} at {} quality",
        dir,
        model.quality.to_string()
    )
    .len();
    let truncate = status_line_len.saturating_sub(model.size.cols.into());
    let dir = truncate_string_by(&dir, truncate);

    // top status line
    let status_line = Line::from(vec![
        Span::raw("Rendering files in "),
        Span::styled(dir, Style::new().fg(Color::Blue).italic()),
        Span::raw(" at "),
        Span::styled(
            model.quality.to_string(),
            Style::new().fg(Color::Cyan).italic(),
        ),
        Span::raw(" quality"),
    ]);
    f.render_widget(status_line, chunks[0]);

    // bottom keymap legend
    let mut legend = vec![
        LegendElement {
            name: "q",
            desc: "quit",
        },
        LegendElement {
            name: "space",
            desc: "begin chord",
        },
    ];

    if model.prev_was_space.is_some() {
        legend = vec![
            LegendElement {
                name: "q",
                desc: "set quality",
            },
            LegendElement {
                name: "r",
                desc: "render last file",
            },
            LegendElement {
                name: "f",
                desc: "change working directory",
            },
        ];
    }
    if let Some(key) = model.chord {
        match key {
            Chord::FilePicker => {}
            Chord::ReRender => {}
            Chord::SetQuality => {
                legend = vec![
                    LegendElement {
                        name: "l",
                        desc: "480p",
                    },
                    LegendElement {
                        name: "m",
                        desc: "720p",
                    },
                    LegendElement {
                        name: "h",
                        desc: "1080p",
                    },
                    LegendElement {
                        name: "p",
                        desc: "1440p",
                    },
                    LegendElement {
                        name: "k",
                        desc: "4K",
                    },
                ];
            }
        }
    }
    f.render_widget(explanation(&legend), chunks[2]);
}

struct LegendElement<'a> {
    name: &'a str,
    desc: &'a str,
}

fn explanation<'a>(legend: &[LegendElement<'a>]) -> Line<'a> {
    let mut elements = vec![];
    for entry in legend {
        elements.push(Span::styled(
            format!(" {} ", entry.name),
            Style::new().on_white().black(),
        ));
        elements.push(Span::styled(
            format!(" {} ", entry.desc),
            Style::new().dark_gray(),
        ));
    }
    Line::from(elements).alignment(Alignment::Center)
}

fn ui(f: &mut Frame, screen: &Screen, model: &mut Model) {
    if let Some(fp) = model.file_picker.as_mut() {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(2),
                Constraint::Percentage(100),
                Constraint::Min(1),
            ])
            .split(f.size());

        let header = "choose a directory to monitor for file changes";
        let header = Paragraph::new(header).style(Style::new().italic());

        let legend = vec![
            LegendElement {
                name: "q",
                desc: "quit",
            },
            LegendElement {
                name: "hjkl / ←↓↑→",
                desc: "navigate",
            },
            LegendElement {
                name: "space",
                desc: "confirm",
            },
            LegendElement {
                name: "esc",
                desc: "cancel",
            },
        ];
        let size_ref = model.size.cols.saturating_sub(4);
        let theme = Theme::default().with_title_top(move |fp| {
            Line::from(truncate_string_to(
                fp.cwd().to_string_lossy().as_ref(),
                size_ref as usize,
            ))
        });
        fp.set_theme(theme);
        f.render_widget(header, chunks[0]);
        f.render_widget(&fp.widget(), chunks[1]);
        f.render_widget(explanation(&legend), chunks[2]);
        return;
    }
    main_ui(f, screen, model)
}

fn setup_terminal() -> io::Result<(Terminal<CrosstermBackend<BufWriter<io::Stdout>>>, Size)> {
    enable_raw_mode()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(BufWriter::new(stdout));
    let mut terminal = Terminal::new(backend)?;
    let initial_size = terminal.size()?;
    let size = Size {
        rows: initial_size.height,
        cols: initial_size.width,
    };
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    Ok((terminal, size))
}

fn cleanup_terminal(
    terminal: &mut Terminal<CrosstermBackend<BufWriter<io::Stdout>>>,
) -> io::Result<()> {
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    terminal.show_cursor()?;
    terminal.clear()?;
    Ok(())
}
