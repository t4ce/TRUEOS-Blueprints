use std::{env, fs, path::PathBuf};

fn main() {
    // Compare the small probe transport with the authoritative sibling kernel,
    // including when the Blueprint builder stages this package under target/.
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let protocol = manifest.ancestors().map(|root| root.join("TRUEOS/crates/trueos-vm/src/vmcall.rs"))
        .find(|path| path.is_file()).expect("sibling TRUEOS guest protocol source is required");
    println!("cargo:rerun-if-changed={}", protocol.display());
    let source = fs::read_to_string(protocol).unwrap();
    for expected in [
        "pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E;",
        "pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F;",
        "pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31;",
        "pub const STATUS_OK: u32 = 0;",
        "pub const COMM_PAGE_BYTES: usize = 160 * 1024;",
        "pub const PAYLOAD_CAP: usize = COMM_PAGE_BYTES - 56;",
        "const COMM_PAGE_VA: u64 = 0x0000_0000_2040_0000;",
    ] {
        assert!(source.contains(expected), "Hull probe protocol changed: {expected}");
    }
    let structure = source.split("struct CommPage {").nth(1).unwrap().split('}').next().unwrap();
    let expected = "request_op:u32,request_seq:u32,request_arg0:u64,request_arg1:u64,request_len:u32,request_pad:u32,response_seq:u32,response_status:u32,response_data:u64,response_len:u32,response_pad:u32,payload:[u8;PAYLOAD_CAP],";
    let actual: String = structure.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(actual, expected, "Hull CommPage layout changed");
}
