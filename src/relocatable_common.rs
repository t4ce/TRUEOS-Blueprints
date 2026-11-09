//! Bind tentative C/assembly storage before handing a relocatable ELF to Hull.
//! Hull resolves section-backed symbols, not ELF SHN_COMMON allocation requests.
use std::{fs, path::Path};

fn common_symbols(object: &Path) -> Result<Vec<String>, String> {
    let output = crate::artifact::tool_command(&["llvm-readelf", "readelf"])?
        .arg("-Ws")
        .arg(object)
        .output()
        .map_err(|error| format!("cannot inspect relocatable COMMON storage: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect relocatable COMMON storage in {}: {}",
            object.display(),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    let symbols = String::from_utf8(output.stdout)
        .map_err(|error| format!("COMMON symbol inventory is not UTF-8: {error}"))?;
    Ok(symbols
        .lines()
        .filter_map(|line| {
            let columns: Vec<_> = line.split_whitespace().collect();
            (columns.len() >= 8 && matches!(columns[6], "COM" | "COMMON"))
                .then(|| columns[7].to_owned())
        })
        .collect())
}

pub(crate) fn materialize(object: &Path) -> Result<(), String> {
    let common = common_symbols(object)?;
    if common.is_empty() {
        return Ok(());
    }
    // Rust LLD currently accepts -d but leaves SHN_COMMON symbols unbound.
    // Keep the established LLD GC/coalescing link, then ask GNU BFD to allocate
    // surviving tentative definitions with their original size/alignment.
    let mut linker = crate::artifact::tool_command(&["ld.bfd"])
        .or_else(|_| crate::artifact::tool_command(&["ld"]))
        .map_err(|error| format!("COMMON storage requires GNU ld.bfd or ld: {error}"))?;
    let staged = object.with_extension("common.o");
    linker
        .arg("-r")
        .arg("-d")
        .arg("--no-relax")
        .arg("-o")
        .arg(&staged)
        .arg(object);
    crate::run_command(&mut linker, "GNU relocatable COMMON allocation")?;
    let remaining = common_symbols(&staged)?;
    if !remaining.is_empty() {
        return Err(format!(
            "COMMON storage remains after allocation: {}. Install GNU ld.bfd; LLVM LLD does not materialize COMMON storage under -r",
            remaining.join(", "),
        ));
    }
    // Preserve the original linked object if allocation or validation failed.
    fs::rename(&staged, object)
        .map_err(|error| format!("cannot publish allocated COMMON storage: {error}"))?;
    println!(
        "trueos-blueprint: allocated {} COMMON storage symbol(s) into ELF sections",
        common.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run(command: &mut Command) {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn real_elf_common_becomes_zeroed_aligned_storage_with_relocations_intact() {
        let work = std::env::temp_dir().join(format!("trueos-common-link-{}", std::process::id()));
        fs::create_dir_all(&work).unwrap();
        fs::write(
            work.join("input.s"),
            r#"
.comm common_capabilities,40,64
.comm common_counter,8,8
.section .text.common_entry,"ax",@progbits
.globl common_entry
.type common_entry,@function
common_entry:
    lea common_capabilities(%rip), %rax
    ret
.size common_entry,.-common_entry
.section .data.pointers,"aw",@progbits
.globl common_pointer
.type common_pointer,@object
common_pointer: .quad common_capabilities
.size common_pointer,8
.globl counter_pointer
.type counter_pointer,@object
counter_pointer: .quad common_counter
.size counter_pointer,8
.globl external_pointer
.type external_pointer,@object
external_pointer: .quad external_value
.size external_pointer,8
.section .note.GNU-stack,"",@progbits
"#,
        )
        .unwrap();
        fs::write(work.join("main.c"), r#"
#include <stdint.h>
extern unsigned char common_capabilities[40];
extern uint64_t common_counter;
extern unsigned char *common_pointer;
extern uint64_t *counter_pointer;
extern int (*external_pointer)(void);
extern void *common_entry(void);
int external_value(void) { return 37; }
int main(void) {
    if ((uintptr_t)common_capabilities % 64 || (uintptr_t)&common_counter % 8) return 1;
    for (unsigned i = 0; i < 40; ++i) if (common_capabilities[i]) return 2;
    if (common_counter || common_pointer != common_capabilities || counter_pointer != &common_counter) return 3;
    if (common_entry() != common_capabilities || external_pointer() != 37) return 4;
    common_capabilities[39] = 91;
    *counter_pointer = 0x123456789abcdef0ULL;
    if (common_pointer[39] != 91 || common_counter != 0x123456789abcdef0ULL) return 5;
    return 0;
}
"#).unwrap();
        run(Command::new("cc")
            .arg("-c")
            .arg(work.join("input.s"))
            .arg("-o")
            .arg(work.join("input.o")));
        for coalesced in [false, true] {
            let object = work.join(if coalesced {
                "coalesced.o"
            } else {
                "ordinary.o"
            });
            let mut linker = crate::artifact::tool_command(&["ld.lld", "rust-lld", "ld"]).unwrap();
            linker
                .arg("-r")
                .arg("--gc-sections")
                .arg("--undefined=common_entry")
                .arg("--undefined=common_pointer")
                .arg("--undefined=counter_pointer")
                .arg("--undefined=external_pointer");
            if coalesced {
                let script = work.join("coalesce.ld");
                fs::write(&script, crate::REL_COALESCE_LINKER_SCRIPT).unwrap();
                linker.arg("-T").arg(script);
            }
            run(linker.arg("-o").arg(&object).arg(work.join("input.o")));
            assert_eq!(common_symbols(&object).unwrap().len(), 2);
            materialize(&object).unwrap();
            assert!(common_symbols(&object).unwrap().is_empty());
            let sections = Command::new("readelf")
                .arg("-SW")
                .arg(&object)
                .output()
                .unwrap();
            let sections = String::from_utf8(sections.stdout).unwrap();
            assert!(
                sections
                    .lines()
                    .any(|line| line.contains(".bss") && line.contains("NOBITS")),
                "{sections}"
            );
            let relocs = Command::new("readelf")
                .arg("-rW")
                .arg(&object)
                .output()
                .unwrap();
            let relocs = String::from_utf8(relocs.stdout).unwrap();
            assert!(relocs.contains("external_value"), "{relocs}");
            let binary = object.with_extension("test");
            run(Command::new("cc")
                .arg(work.join("main.c"))
                .arg(&object)
                .arg("-o")
                .arg(&binary));
            run(&mut Command::new(&binary));
            // Already section-bound objects follow the no-rewrite fast path.
            let previous = fs::read(&object).unwrap();
            materialize(&object).unwrap();
            assert_eq!(fs::read(&object).unwrap(), previous);
        }
        fs::remove_dir_all(work).unwrap();
    }
}
