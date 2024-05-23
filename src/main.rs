use anyhow::Result;
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
    style::{Color, Modifier, Style, Stylize},
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

#[derive(Default)]
struct Model {
    prev_was_space: Option<()>,
    chord: Option<Chord>,
    quality: Quality,
    directory: PathBuf,
    pty_system: NativePtySystem,
    file_picker: Option<FileExplorer>,
}

impl Model {
    fn ingest_chord(&mut self, keycode: KeyCode) {
        self.chord = match keycode {
            KeyCode::Char('f') => {
                let theme = Theme::default().add_default_title();
                let fp = FileExplorer::with_theme(theme).unwrap();
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
}

fn main() -> Result<()> {
    let (mut terminal, size) = setup_terminal()?;

    let mut model = Model {
        directory: std::env::current_dir()?,
        ..Default::default()
    };

    run(&mut terminal, size, &mut model)?;

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
                        .iter()
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

fn run<B: Backend>(terminal: &mut Terminal<B>, size: Size, model: &mut Model) -> Result<()> {
    let parser = Arc::new(RwLock::new(vt100::Parser::new(
        size.rows - 1,
        size.cols - 1,
        0,
    )));

    let (tx, rx) = channel();
    let es = EventSender { tx };
    // Automatically select the best implementation for your platform.
    let mut watcher = notify::recommended_watcher(es)?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(&model.directory, RecursiveMode::Recursive)?;
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, parser.read().unwrap().screen(), model))?;
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if let Ok(name) = rx.try_recv() {
            let cwd = std::env::current_dir()?;
            let mut cmd = CommandBuilder::new("manim");
            cmd.args(["render", "--preview", "--quality", "l", &name]);
            cmd.cwd(cwd);

            let pair = model.pty_system.openpty(PtySize {
                rows: size.rows - 4,
                cols: size.cols,
                ..Default::default()
            })?;

            let mut child = pair.slave.spawn_command(cmd)?;
            drop(pair.slave);

            let mut reader = pair.master.try_clone_reader()?;

            {
                let parser = parser.clone();
                std::thread::spawn(move || {
                    // Consume the output from the child
                    let mut s = String::default();
                    reader.read_to_string(&mut s).unwrap();
                    if !s.is_empty() {
                        let mut parser = parser.write().unwrap();
                        parser.process(s.as_bytes());
                    }
                });
            }

            // Drop writer on purpose
            pair.master.take_writer()?;

            // Wait for the child to complete
            let _child_exit_status = child.wait()?;

            drop(pair.master);
        }

        if crossterm::event::poll(timeout)? {
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
                let cwd = std::env::current_dir()?;
                let mut cmd = CommandBuilder::new("manim");
                cmd.args(["render", "--help"]);
                cmd.cwd(cwd);

                let pair = model.pty_system.openpty(PtySize {
                    rows: size.rows,
                    cols: size.cols,
                    ..Default::default()
                })?;

                let mut child = pair.slave.spawn_command(cmd)?;
                drop(pair.slave);

                let mut reader = pair.master.try_clone_reader()?;

                {
                    let parser = parser.clone();
                    std::thread::spawn(move || {
                        // Consume the output from the child
                        let mut s = String::new();
                        reader.read_to_string(&mut s).unwrap();
                        if !s.is_empty() {
                            let mut parser = parser.write().unwrap();
                            parser.process(s.as_bytes());
                        }
                    });
                }

                // Drop writer on purpose
                pair.master.take_writer()?;

                // Wait for the child to complete
                let _child_exit_status = child.wait()?;

                drop(pair.master);
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

fn main_ui(f: &mut Frame, screen: &Screen, model: &mut Model) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .margin(1)
        .constraints(
            [
                ratatui::layout::Constraint::Min(2),
                ratatui::layout::Constraint::Percentage(100),
                ratatui::layout::Constraint::Min(1),
            ]
            .as_ref(),
        )
        .split(f.size());
    let title = Line::from(" manim output ");
    let block = Block::default().borders(Borders::ALL).title(title);
    let pseudo_term = PseudoTerminal::new(screen).block(block);
    f.render_widget(pseudo_term, chunks[1]);

    // top status line
    let status_line = Line::from(vec![
        Span::raw("Rendering files in "),
        Span::styled(
            model.directory.display().to_string(),
            Style::new().fg(Color::Blue).italic(),
        ),
        Span::raw(" at "),
        Span::styled(
            model.quality.to_string(),
            Style::new().fg(Color::Cyan).italic(),
        ),
        Span::raw(" quality"),
    ]);
    f.render_widget(status_line, chunks[0]);

    // bottom keymap legend
    let mut explanation = "q -> exit | space -> begin chord";

    if model.prev_was_space.is_some() {
        explanation =
            "q -> set quality | r -> trigger manual rerender | f -> choose working directory";
    }
    if let Some(key) = model.chord {
        match key {
            Chord::FilePicker => {
                explanation = "file picker";
            }
            Chord::ReRender => {
                explanation = "triggered render";
            }
            Chord::SetQuality => {
                explanation = "l -> 480p | m -> 720p | h -> 1080p | p -> 1440p | k -> 4K";
            }
        }
    }
    let explanation = Paragraph::new(explanation)
        .style(Style::default().add_modifier(Modifier::BOLD).dark_gray())
        .alignment(Alignment::Center);
    f.render_widget(explanation, chunks[2]);
}

fn ui(f: &mut Frame, screen: &Screen, model: &mut Model) {
    if let Some(fp) = &model.file_picker {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    ratatui::layout::Constraint::Min(2),
                    ratatui::layout::Constraint::Percentage(100),
                    ratatui::layout::Constraint::Min(1),
                ]
                .as_ref(),
            )
            .split(f.size());

        let header = "choose a directory to monitor for file changes";
        let header = Paragraph::new(header).style(Style::new().italic());

        let explanation = "hjkl / arrow keys -> move | space -> pick | enter -> enter a directory | q -> quit | esc -> cancel";
        let explanation = Paragraph::new(explanation)
            .style(Style::default().add_modifier(Modifier::BOLD).dark_gray())
            .alignment(Alignment::Center);

        f.render_widget(header, chunks[0]);
        f.render_widget(&fp.widget(), chunks[1]);
        f.render_widget(explanation, chunks[2]);
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
