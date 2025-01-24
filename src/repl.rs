use crossterm::{
    cursor::{position, MoveTo, MoveToColumn, MoveToNextLine, MoveToRow, MoveUp},
    event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Print, PrintStyledContent, Stylize},
    terminal::{self, disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use std::{
    collections::VecDeque,
    io::{self, stdout, Write},
    time::Duration,
};

const POLL_DUR_MS: u64 = 10;

#[derive(Default)]
pub struct Repl {
    buffer_history: VecDeque<String>,
    buffer_idx: usize,
    cursor_pos: u16,
    cur_input_row: u16,
    quit: bool,
}

impl Drop for Repl {
    fn drop(&mut self) {
        disable_raw_mode().unwrap();
        println!("\nBye.");
    }
}

impl Repl {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self {
            buffer_history: VecDeque::new(),
            buffer_idx: 0,
            cursor_pos: 0,
            cur_input_row: 0,
            quit: false,
        })
    }

    fn inc_cursor_pos(&mut self, n: u16) {
        if (self.cursor_pos as usize) < self.get_buffer().len() {
            self.cursor_pos = self.cursor_pos.saturating_add(n);
        }
    }

    fn dec_cursor_pos(&mut self, n: u16) {
        self.cursor_pos = self.cursor_pos.saturating_sub(n);
    }

    fn set_cur_input_row(&mut self) -> io::Result<()> {
        self.cur_input_row = position()?.1;
        Ok(())
    }

    fn get_output_rows(&self, output: &str) -> io::Result<u16> {
        Ok(output.len() as u16 / terminal::size()?.1
            + u16::from(output.len() as u16 % terminal::size()?.1 != 0))
    }

    fn get_buffer(&self) -> &String {
        self.buffer_history.get(self.buffer_idx).unwrap()
    }

    fn get_mut_buffer(&mut self) -> &mut String {
        self.buffer_history.get_mut(self.buffer_idx).unwrap()
    }

    fn inc_buffer_idx(&mut self) {
        if self.buffer_idx < self.buffer_history.len() - 1 {
            self.buffer_idx += 1;
            self.cursor_pos = self.get_buffer().len() as u16;
        }
    }

    fn dec_buffer_idx(&mut self) {
        self.buffer_idx = self.buffer_idx.saturating_sub(1);
        self.cursor_pos = self.get_buffer().len() as u16;
    }

    fn clone_buffer(&mut self) {
        // Clones the current buffer history being viewed into
        // the current active buffer for editing

        self.buffer_history[0] = self.get_buffer().clone();
        self.buffer_idx = 0;
    }

    pub fn run(&mut self) -> io::Result<()> {
        execute!(stdout(), PrintStyledContent("> ".bold().blue()))?;
        self.buffer_history.push_front(String::new());
        self.set_cur_input_row()?;

        loop {
            if !poll(Duration::from_millis(POLL_DUR_MS))? {
                continue;
            }

            match read()? {
                Event::Key(event) => {
                    self.handle_key_event(event)?;
                }
                _ => continue,
            };

            stdout().flush()?;

            if self.quit {
                break;
            }
        }

        Ok(())
    }

    pub fn handle_key_event(&mut self, event: KeyEvent) -> io::Result<()> {
        match (event.code, event.modifiers) {
            // Keyboard shortcuts
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.quit = true,
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                queue!(stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                self.cur_input_row = 0;
            }

            // Input
            (KeyCode::Char(_) | KeyCode::Backspace, _) => self.handle_input(event)?,

            // Process the contents of the buffer and clear when enter is hit
            (KeyCode::Enter, _) => self.handle_enter()?,

            // Traverse history
            (KeyCode::Up, _) => self.inc_buffer_idx(),
            (KeyCode::Down, _) => self.dec_buffer_idx(),

            // Handle arrow keys
            (KeyCode::Left, _) => self.dec_cursor_pos(1),
            (KeyCode::Right, _) => self.inc_cursor_pos(1),
            _ => {}
        }

        self.update_text()?;
        Ok(())
    }

    fn handle_input(&mut self, event: KeyEvent) -> io::Result<()> {
        if self.buffer_idx != 0 {
            self.clone_buffer();
        }

        match (event.code, event.modifiers) {
            (KeyCode::Char(ch), _) => {
                // Adjust terminal size if we're on the last row
                if terminal::size()?.1 - 1 == position()?.1 {
                    queue!(stdout(), Print("\n"), MoveUp(1))?;
                    self.cur_input_row -= 1;
                }

                let pos = self.cursor_pos.into();
                self.get_mut_buffer().insert(pos, ch);
                self.inc_cursor_pos(1);
            }

            (KeyCode::Backspace, _) => {
                if !self.get_buffer().is_empty() && self.cursor_pos != 0 {
                    let pos = self.cursor_pos as usize - 1;
                    self.get_mut_buffer().remove(pos);
                    self.dec_cursor_pos(1);
                }
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn handle_enter(&mut self) -> io::Result<()> {
        let output = self.process_buffer();

        // Check if we're on the last line, extend by two
        if terminal::size()?.1 - 2 >= position()?.1 {
            // This is horrible code, please forgive me until I figure something out
            let clear_height = self.get_output_rows(&output.clone().unwrap_or_else(|x| x))? + 1;

            queue!(
                stdout(),
                Print("\n".repeat(clear_height.into())),
                MoveUp(clear_height)
            )?;
        }

        if self.get_buffer().trim().is_empty() {
            queue!(stdout(), MoveToNextLine(1))?;
        } else {
            match output {
                Ok(output) => queue!(
                    stdout(),
                    MoveToNextLine(1),
                    Print(output),
                    MoveToNextLine(1)
                )?,
                Err(err) => queue!(
                    stdout(),
                    MoveToNextLine(1),
                    PrintStyledContent("ERROR".bold().red()),
                    Print(format!(": {}", err)),
                    MoveToNextLine(1),
                )?,
            };

            if self.buffer_idx > 1 {
                self.clone_buffer();
            }
        }

        self.buffer_history.push_front(String::new());
        self.cursor_pos = 0;
        self.buffer_idx = 0;
        self.set_cur_input_row()?;

        Ok(())
    }

    fn update_text(&mut self) -> io::Result<()> {
        let (cols, _) = terminal::size()?;
        queue!(
            stdout(),
            MoveToRow(self.cur_input_row),
            Clear(ClearType::CurrentLine),
            Clear(ClearType::FromCursorDown),
            MoveToColumn(0),
            PrintStyledContent("> ".bold().blue()),
            Print(&self.get_buffer()),
            MoveToColumn((2 + self.cursor_pos) % cols),
            MoveToRow(self.cur_input_row + (2 + self.cursor_pos) / cols),
        )?;
        Ok(())
    }

    fn process_buffer(&self) -> Result<String, String> {
        match self.get_buffer().clone().trim() {
            "error" => Err("This is a big bad error!".into()),
            _ => Ok(self.get_buffer().clone()),
        }
    }
}
