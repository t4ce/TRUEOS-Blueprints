extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

type PrinterReadFn = unsafe extern "C" fn(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;

unsafe extern "C" {
    fn trueos_vlayer_printer_snapshot_read(
        offset: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Printer {
    pub name: String,
    pub uri: String,
    pub secure: bool,
    pub make_and_model: Option<String>,
    pub formats: Vec<String>,
    pub last_seen_ms: u64,
}

#[inline]
pub fn snapshot_bytes() -> Result<Vec<u8>, i32> {
    read_all(trueos_vlayer_printer_snapshot_read)
}

#[inline]
pub fn snapshot_len() -> Result<usize, i32> {
    read_len(trueos_vlayer_printer_snapshot_read)
}

#[inline]
pub fn snapshot_text() -> Result<String, i32> {
    String::from_utf8(snapshot_bytes()?).map_err(|_| -1)
}

pub fn snapshot() -> Result<Vec<Printer>, i32> {
    let text = snapshot_text()?;
    let mut printers = Vec::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        if fields.next() != Some("printer") {
            continue;
        }
        let Some(name) = fields.next() else {
            continue;
        };
        if name == "name" {
            continue;
        }
        let Some(uri) = fields.next() else {
            continue;
        };
        let secure = fields.next() == Some("1");
        let model = fields.next().unwrap_or("");
        let formats = fields.next().unwrap_or("");
        let last_seen_ms = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        printers.push(Printer {
            name: name.to_string(),
            uri: uri.to_string(),
            secure,
            make_and_model: (!model.is_empty()).then(|| model.to_string()),
            formats: formats
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
            last_seen_ms,
        });
    }
    Ok(printers)
}

fn read_all(read_fn: PrinterReadFn) -> Result<Vec<u8>, i32> {
    let len = read_len(read_fn)?;
    let mut bytes = vec![0u8; len];
    if len == 0 {
        return Ok(bytes);
    }

    let got = unsafe { read_fn(0, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 {
        return Err(got as i32);
    }

    bytes.truncate((got as usize).min(len));
    Ok(bytes)
}

fn read_len(read_fn: PrinterReadFn) -> Result<usize, i32> {
    let len = unsafe { read_fn(0, core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    Ok(len as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_record_shape_remains_stable() {
        let printer = Printer {
            name: String::from("Office"),
            uri: String::from("ipp://192.0.2.1:631/ipp/print"),
            secure: false,
            make_and_model: Some(String::from("Example")),
            formats: vec![String::from("application/pdf")],
            last_seen_ms: 15_000,
        };
        assert_eq!(printer.name, "Office");
        assert!(!printer.secure);
    }
}
