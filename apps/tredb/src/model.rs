use std::fmt;

#[derive(Clone, Debug, Default)]
pub struct DbSnapshot {
    pub tables: Vec<TableSnapshot>,
}

impl DbSnapshot {
    pub fn total_rows(&self) -> usize {
        self.tables.iter().map(|table| table.rows.len()).sum()
    }

    pub fn table(&self, name: &str) -> Option<&TableSnapshot> {
        self.tables.iter().find(|table| table.name == name)
    }

    pub fn contains_selection(&self, selection: &Selection) -> bool {
        match selection {
            Selection::Database => true,
            Selection::Table { table } => self.table(table).is_some(),
            Selection::Row { table, key } => self
                .table(table)
                .is_some_and(|snapshot| snapshot
                    .rows
                    .iter()
                    .any(|row| row.key.as_slice() == key.as_slice())),
        }
    }

    pub fn normalize_selection(&self, selection: &mut Selection) {
        if self.contains_selection(selection) {
            return;
        }

        if let Selection::Row { table, .. } = selection {
            let table = table.clone();
            if self.table(&table).is_some() {
                *selection = Selection::Table { table };
                return;
            }
        }

        *selection = Selection::Database;
    }
}

#[derive(Clone, Debug)]
pub struct TableSnapshot {
    pub name: String,
    pub rows: Vec<RowSnapshot>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct RowSnapshot {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    Database,
    Table { table: String },
    Row { table: String, key: Vec<u8> },
}

impl Selection {
    pub fn table_name(&self) -> Option<&str> {
        match self {
            Selection::Database => None,
            Selection::Table { table } | Selection::Row { table, .. } => Some(table),
        }
    }

    pub fn row_key(&self) -> Option<&[u8]> {
        match self {
            Selection::Row { key, .. } => Some(key),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Selection::Database => "RAM database".to_owned(),
            Selection::Table { table } => format!("table ・ {table}"),
            Selection::Row { table, key } => {
                format!("{table} ・ {}", display_bytes(key))
            }
        }
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::Database
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteParseError(String);

impl fmt::Display for ByteParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ByteParseError {}

pub fn parse_user_bytes(input: &str) -> Result<Vec<u8>, ByteParseError> {
    let Some(hex) = input.strip_prefix("hex:") else {
        return Ok(input.as_bytes().to_vec());
    };

    let compact: String = hex
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '_')
        .collect();
    if compact.is_empty() {
        return Ok(Vec::new());
    }
    if compact.len() % 2 != 0 {
        return Err(ByteParseError(
            "hex input needs an even number of digits".to_owned(),
        ));
    }

    let mut bytes = Vec::with_capacity(compact.len() / 2);
    let raw = compact.as_bytes();
    for index in (0..raw.len()).step_by(2) {
        let high = hex_nibble(raw[index]).ok_or_else(|| {
            ByteParseError(format!("invalid hex digit at position {}", index + 1))
        })?;
        let low = hex_nibble(raw[index + 1]).ok_or_else(|| {
            ByteParseError(format!("invalid hex digit at position {}", index + 2))
        })?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn display_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if text.chars().all(|ch| !ch.is_control()) => text.to_owned(),
        _ => hex_bytes(bytes),
    }
}

pub fn input_bytes(bytes: &[u8]) -> String {
    display_bytes(bytes)
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    let mut result = String::from("hex:");
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            result.push(' ');
        }
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{display_bytes, parse_user_bytes};

    #[test]
    fn parses_text_and_hex() {
        assert_eq!(parse_user_bytes("hello").unwrap(), b"hello");
        assert_eq!(parse_user_bytes("hex:00 ff 7a").unwrap(), [0, 255, 122]);
        assert!(parse_user_bytes("hex:0").is_err());
    }

    #[test]
    fn renders_binary_as_hex() {
        assert_eq!(display_bytes(&[0, 255]), "hex:00 ff");
        assert_eq!(display_bytes(b"hello"), "hello");
    }
}
