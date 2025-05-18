use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Style, Color},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::{io, time::Duration};
use crate::process::{OutputChannels, ProcessCommand};

/// Runs the TUI event loop, rendering process windows and handling user input.
/// Starts all processes, updates buffers with output, and manages scroll and process control.
/// 
/// # Arguments
/// * `channels` - The output and control channels for each process.
/// 
/// # Returns
/// * `Result<(), Box<dyn std::error::Error>>` - Ok on normal exit, Err on failure.
pub async fn run_tui(
    // config: crate::config::Config,
    mut channels: OutputChannels,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut buffers: Vec<Vec<String>> = vec![Vec::new(); channels.len()];
    let mut running: Vec<bool> = vec![true; channels.len()];
    let mut scroll_offsets: Vec<u16> = vec![0; channels.len()];

    // Start all processes initially
    for (_, _, tx) in &channels {
        let _ = tx.try_send(ProcessCommand::Start);
    }

    loop {
        let layout = get_layout(&mut terminal, channels.len());
        update_buffers_and_scroll(&mut channels, &mut buffers, &layout, &mut scroll_offsets);

        terminal.draw(|f| {
            draw_process_windows(
                f,
                &channels,
                &buffers,
                &running,
                &scroll_offsets,
            );
            draw_help_line(f);
        })?;

        if handle_input_event(&mut channels, &mut running, &mut scroll_offsets, &buffers)? {
            break;
        }
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Returns a vector of layout rectangles for each process window, splitting the terminal vertically.
/// Each window gets an equal share of the available space.
/// 
/// # Arguments
/// * `terminal` - The terminal instance to get the area from.
/// * `n` - The number of process windows to split the area into.
/// 
/// # Returns
/// * `Vec<ratatui::layout::Rect>` - The rectangles for each process window.
fn get_layout(terminal: &mut ratatui::Terminal<CrosstermBackend<std::io::Stdout>>, n: usize) -> Vec<ratatui::layout::Rect> {
    let term_area = terminal.get_frame().area();
    Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(vec![Constraint::Percentage(100 / n as u16); n])
        .split(term_area)
        .to_vec()
}

/// Updates the output buffers for each process by draining their channels.
/// Also manages autoscroll: if new lines are added, scrolls to show the latest output.
/// 
/// # Arguments
/// * `channels` - Mutable reference to the process output channels.
/// * `buffers` - Mutable reference to the output buffers for each process.
/// * `layout` - The layout rectangles for each process window.
/// * `scroll_offsets` - Mutable reference to the scroll offsets for each process window.
fn update_buffers_and_scroll(
    channels: &mut OutputChannels,
    buffers: &mut Vec<Vec<String>>,
    layout: &[ratatui::layout::Rect],
    scroll_offsets: &mut Vec<u16>,
) {
    for (i, (_, rx, _)) in channels.iter_mut().enumerate() {
        while let Ok(line) = rx.try_recv() {
            buffers[i].push(line);
            let visible_height = layout.get(i).map(|a| a.height.saturating_sub(2)).unwrap_or(0);
            let buffer_len = buffers[i].len() as u16;
            if buffer_len > visible_height && visible_height > 0 {
                scroll_offsets[i] = buffer_len - visible_height;
            }
        }
    }
}

/// Draws each process window, including its output, title, and a vertical scrollbar.
/// Each window shows the process name, a start/stop button, and the current output buffer.
/// 
/// # Arguments
/// * `f` - The ratatui frame to render into.
/// * `channels` - The process channels (names and control).
/// * `buffers` - The output buffers for each process.
/// * `running` - The running/stopped state for each process.
/// * `scroll_offsets` - The scroll offset for each process window.
fn draw_process_windows<'a>(
    f: &mut ratatui::Frame<'a>,
    channels: &OutputChannels,
    buffers: &Vec<Vec<String>>,
    running: &Vec<bool>,
    scroll_offsets: &Vec<u16>,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(vec![Constraint::Percentage(100 / channels.len() as u16); channels.len()])
        .split(f.area());

    for (i, area) in layout.iter().enumerate() {
        let name = &channels[i].0;
        let button = if running[i] { "[Stop]" } else { "[Start]" };
        let title = format!("{} {}", name, button);
        let text = buffers[i].join("\n");
        let para = Paragraph::new(text)
            .block(Block::default().title(title.as_str()).borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
            .scroll((scroll_offsets[i], 0));
        f.render_widget(para, *area);

        let content_height = buffers[i].len() as u16;
        let mut scrollbar_state = ScrollbarState::default()
            .content_length(content_height as usize)
            .position(scroll_offsets[i] as usize);
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight);
        f.render_stateful_widget(scrollbar, *area, &mut scrollbar_state);
    }
}

/// Draws a help line at the bottom of the screen with key bindings for the user.
/// 
/// # Arguments
/// * `f` - The ratatui frame to render into.
fn draw_help_line(f: &mut ratatui::Frame) {
    let help = "(q: quit, 1-9: toggle process, ↑/↓: scroll)";
    let rect = f.area();
    let help_area = ratatui::layout::Rect {
        x: rect.x,
        y: rect.y + rect.height.saturating_sub(1),
        width: rect.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(help)
            .style(Style::default().fg(Color::Yellow)),
        help_area,
    );
}

/// Handles user input events for process control and scrolling.
/// Returns Ok(true) if the user requested to quit, otherwise Ok(false).
/// 
/// # Arguments
/// * `channels` - Mutable reference to the process channels for sending control commands.
/// * `running` - Mutable reference to the running/stopped state for each process.
/// * `scroll_offsets` - Mutable reference to the scroll offsets for each process window.
/// * `buffers` - Reference to the output buffers for each process.
/// 
/// # Returns
/// * `Result<bool, Box<dyn std::error::Error>>` - Ok(true) if quit, Ok(false) otherwise.
fn handle_input_event(
    channels: &mut OutputChannels,
    running: &mut Vec<bool>,
    scroll_offsets: &mut Vec<u16>,
    buffers: &Vec<Vec<String>>,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crossterm::event::{self, Event, KeyCode};
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char(c) if c >= '1' && (c as usize - '1' as usize) < running.len() => {
                    let idx = c as usize - '1' as usize;
                    running[idx] = !running[idx];
                    let (_, _, tx) = &channels[idx];
                    let cmd = if running[idx] {
                        ProcessCommand::Start
                    } else {
                        ProcessCommand::Stop
                    };
                    let _ = tx.try_send(cmd);
                }
                KeyCode::Char(c) if c >= '1' && (c as usize - '1' as usize) < scroll_offsets.len() => {
                    // handled above for start/stop
                }
                KeyCode::Up => {
                    for offset in scroll_offsets.iter_mut() {
                        if *offset > 0 {
                            *offset -= 1;
                        }
                    }
                }
                KeyCode::Down => {
                    for (i, offset) in scroll_offsets.iter_mut().enumerate() {
                        let max_offset = buffers[i].len().saturating_sub(1) as u16;
                        if *offset < max_offset {
                            *offset += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(false)
}
