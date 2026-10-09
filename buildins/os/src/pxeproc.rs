//! One-shot UI key scripts supplied through the ordinary Blueprint launch surface.

use crossterm::event::KeyCode;

pub fn parse_keys(script: &str) -> Result<Vec<KeyCode>, String> {
    script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line {
            "key up" => Ok(KeyCode::Up),
            "key down" => Ok(KeyCode::Down),
            "key enter" => Ok(KeyCode::Enter),
            "key left" => Ok(KeyCode::Left),
            "key right" => Ok(KeyCode::Right),
            "key escape" => Ok(KeyCode::Esc),
            _ => Err(format!("os: invalid launch directive {line:?}")),
        })
        .collect()
}

pub async fn launch_keys() -> Result<Vec<KeyCode>, String> {
    let Ok(bytes) = trueos::async_fs::read_file(b"vFile:launch").await else {
        return Ok(Vec::new());
    };
    let script =
        String::from_utf8(bytes).map_err(|_| String::from("os: launch script is not UTF-8"))?;
    parse_keys(&script)
}
