//! Small, design-time registry definitions for values whose behavior is part
//! of the XP personality rather than an accident of an imported `.reg` file.

use crate::process::GuestMemory;

pub const HKEY_CURRENT_USER: u32 = 0x8000_0001;
pub const HKEY_LOCAL_MACHINE: u32 = 0x8000_0002;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;

pub struct OpenKeyExAFrame {
    pub caller_ret: u32,
    pub hkey: u32,
    pub subkey: Option<String>,
    pub options: u32,
    pub sam: u32,
    pub result_ptr: u32,
}

pub struct QueryValueExAFrame {
    pub caller_ret: u32,
    pub hkey: u32,
    pub value_name: Option<String>,
    pub reserved: u32,
    pub type_ptr: u32,
    pub data_ptr: u32,
    pub size_ptr: u32,
}

pub fn format_root_name(hkey: u32) -> String {
    match hkey {
        0x8000_0000 => "HKEY_CLASSES_ROOT".into(),
        HKEY_CURRENT_USER => "HKEY_CURRENT_USER".into(),
        HKEY_LOCAL_MACHINE => "HKEY_LOCAL_MACHINE".into(),
        0x8000_0003 => "HKEY_USERS".into(),
        0x8000_0005 => "HKEY_CURRENT_CONFIG".into(),
        _ => format!("0x{hkey:08x}"),
    }
}

pub const fn is_predefined_root(hkey: u32) -> bool {
    matches!(
        hkey,
        0x8000_0000 | HKEY_CURRENT_USER | HKEY_LOCAL_MACHINE | 0x8000_0003 | 0x8000_0005
    )
}

pub fn decode_open_key_ex_a(
    memory: &impl GuestMemory,
    esp: u32,
) -> Result<OpenKeyExAFrame, String> {
    let frame = read_words(memory, esp, 6)?;
    let subkey = (frame[2] != 0)
        .then(|| read_ansi_string(memory, frame[2]))
        .transpose()?;
    Ok(OpenKeyExAFrame {
        caller_ret: frame[0],
        hkey: frame[1],
        subkey,
        options: frame[3],
        sam: frame[4],
        result_ptr: frame[5],
    })
}

pub fn decode_query_value_ex_a(
    memory: &impl GuestMemory,
    esp: u32,
) -> Result<QueryValueExAFrame, String> {
    let frame = read_words(memory, esp, 7)?;
    let value_name = (frame[2] != 0)
        .then(|| read_ansi_string(memory, frame[2]))
        .transpose()?;
    Ok(QueryValueExAFrame {
        caller_ret: frame[0],
        hkey: frame[1],
        value_name,
        reserved: frame[3],
        type_ptr: frame[4],
        data_ptr: frame[5],
        size_ptr: frame[6],
    })
}

fn read_words(memory: &impl GuestMemory, esp: u32, count: usize) -> Result<Vec<u32>, String> {
    (0..count)
        .map(|index| {
            let address = esp
                .checked_add((index as u32) * 4)
                .ok_or_else(|| "guest stack address overflow".to_owned())?;
            let mut bytes = [0; 4];
            memory.read(address, &mut bytes).map_err(str::to_owned)?;
            Ok(u32::from_le_bytes(bytes))
        })
        .collect()
}

fn read_ansi_string(memory: &impl GuestMemory, address: u32) -> Result<String, String> {
    let mut bytes = Vec::new();
    for offset in 0..256u32 {
        let current = address
            .checked_add(offset)
            .ok_or_else(|| format!("address overflow at +0x{offset:x}"))?;
        let mut byte = [0];
        memory.read(current, &mut byte).map_err(str::to_owned)?;
        if byte[0] == 0 {
            break;
        }
        bytes.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Definition {
    pub root: u32,
    pub key_path: &'static str,
    pub value_name: &'static str,
    pub ty: u32,
    pub bytes: &'static [u8],
}

const ALLOW_LOCAL_FILES: Definition = Definition {
    root: HKEY_LOCAL_MACHINE,
    key_path: "Software\\Blizzard Entertainment\\Warcraft III",
    value_name: "Allow Local Files",
    ty: REG_DWORD,
    // Deliberately explicit for the first design-time registry probe.
    bytes: &[0, 0, 0, 0],
};

const BATTLE_NET_GATEWAYS: Definition = Definition {
    root: HKEY_CURRENT_USER,
    key_path: "Software\\Blizzard Entertainment\\Warcraft III",
    value_name: "Battle.net Gateways",
    ty: REG_MULTI_SZ,
    // Warcraft's gateway list begins with the version and entry count, then
    // stores address, zone, and display name for each configured realm.
    bytes: b"1001\0\
        01\0\
        192.168.178.111\0\
        0\0\
        RoC 1.21b Realm\0\0",
};

const DEFINITIONS: &[Definition] = &[ALLOW_LOCAL_FILES, BATTLE_NET_GATEWAYS];

pub fn lookup(root: u32, key_path: &str, value_name: &str) -> Option<Definition> {
    DEFINITIONS.iter().copied().find(|definition| {
        definition.root == root
            && definition.key_path.eq_ignore_ascii_case(key_path.trim_matches('\\'))
            && definition.value_name.eq_ignore_ascii_case(value_name)
    })
}
