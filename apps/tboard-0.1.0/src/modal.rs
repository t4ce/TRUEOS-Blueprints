use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::Lane;

#[derive(Clone, Debug)]
pub enum InputPurpose {
    NewCard { lane: Lane },
    EditTitle { id: u64 },
    EditDetail { id: u64 },
}

#[derive(Clone, Debug)]
pub enum ConfirmPurpose {
    DeleteCard { id: u64 },
    Reset { demo: bool },
}

#[derive(Clone, Debug)]
pub struct InputModal {
    pub title: String,
    pub prompt: String,
    pub text: String,
    pub cursor: usize,
    pub purpose: InputPurpose,
}

impl InputModal {
    pub fn new(
        title: impl Into<String>,
        prompt: impl Into<String>,
        text: impl Into<String>,
        purpose: InputPurpose,
    ) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self {
            title: title.into(),
            prompt: prompt.into(),
            text,
            cursor,
            purpose,
        }
    }

    pub fn insert_text(&mut self, text: &str) {
        let byte = byte_index(&self.text, self.cursor);
        self.text.insert_str(byte, text);
        self.cursor += text.chars().count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = byte_index(&self.text, self.cursor - 1);
        let end = byte_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let start = byte_index(&self.text, self.cursor);
        let end = byte_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
    }
}

#[derive(Clone, Debug)]
pub struct ConfirmModal {
    pub title: String,
    pub question: String,
    pub purpose: ConfirmPurpose,
}

#[derive(Clone, Debug)]
pub enum Modal {
    Input(InputModal),
    Confirm(ConfirmModal),
    Help,
}

#[derive(Clone, Debug)]
pub enum ModalOutcome {
    None,
    Dirty,
    Cancel,
    InputSubmitted { purpose: InputPurpose, text: String },
    Confirmed(ConfirmPurpose),
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        match self {
            Modal::Input(input) => handle_input_key(input, key),
            Modal::Confirm(confirm) => match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                    ModalOutcome::Confirmed(confirm.purpose.clone())
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => ModalOutcome::Cancel,
                _ => ModalOutcome::None,
            },
            Modal::Help => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'Q') => ModalOutcome::Cancel,
                _ => ModalOutcome::None,
            },
        }
    }

    pub fn insert_paste(&mut self, text: &str) -> bool {
        if let Modal::Input(input) = self {
            let single_line = text.replace('\r', " ").replace('\n', " ");
            input.insert_text(&single_line);
            true
        } else {
            false
        }
    }
}

fn handle_input_key(input: &mut InputModal, key: KeyEvent) -> ModalOutcome {
    match key.code {
        KeyCode::Esc => ModalOutcome::Cancel,
        KeyCode::Enter => ModalOutcome::InputSubmitted {
            purpose: input.purpose.clone(),
            text: input.text.clone(),
        },
        KeyCode::Left => {
            input.cursor = input.cursor.saturating_sub(1);
            ModalOutcome::Dirty
        }
        KeyCode::Right => {
            input.cursor = (input.cursor + 1).min(input.text.chars().count());
            ModalOutcome::Dirty
        }
        KeyCode::Home => {
            input.cursor = 0;
            ModalOutcome::Dirty
        }
        KeyCode::End => {
            input.cursor = input.text.chars().count();
            ModalOutcome::Dirty
        }
        KeyCode::Backspace => {
            input.backspace();
            ModalOutcome::Dirty
        }
        KeyCode::Delete => {
            input.delete();
            ModalOutcome::Dirty
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            input.insert_text(&ch.to_string());
            ModalOutcome::Dirty
        }
        _ => ModalOutcome::None,
    }
}

fn byte_index(text: &str, character_index: usize) -> usize {
    text.char_indices()
        .nth(character_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}
