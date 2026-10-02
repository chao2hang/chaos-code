//! Which platform a binary artifact was built for, read from its own file header.
//!
//! `chaos-remote install` hands a build of *this program* to a machine and points
//! `current` at it. The digest says the bytes arrived intact, the signature says who
//! signed them, and [`super::install`] asks the filesystem whether the file may be
//! executed from where it sits — but none of the three notices that an x86-64 Linux
//! build was uploaded to an arm64 host. `access(X_OK)` answers "are you allowed to
//! exec this", which a 0755 file of the wrong architecture passes; the question that
//! actually fails later is "does this kernel know how to load it".
//!
//! So the header is read instead. Every native binary format this project ships
//! states its target in the first few bytes, before any code runs, which means the
//! refusal can happen before a single file is moved:
//!
//! | format | where the target is |
//! |---|---|
//! | ELF (Linux, and anything else Unix-like) | `e_machine` at offset 18, class at 4, byte order at 5 |
//! | Mach-O (macOS) | `cputype` at offset 4, byte order from the magic |
//! | PE (Windows) | `Machine` four bytes past the `PE\0\0` signature, whose offset is the little-endian u32 at 0x3C |
//!
//! The direction of the error is chosen deliberately: a file this module cannot
//! classify — a shell wrapper, a truncated upload, an architecture it has no entry
//! for — is *not* refused. Refusing on a guess would block the deployments that work
//! today in order to catch one that the next `execve` would have rejected on its own
//! anyway. Only a header that positively names a different platform stops an install.

/// The platform a file declares itself built for, or the platform this program is
/// running on. Both use the [`std::env::consts`] vocabulary (`"linux"`, `"macos"`,
/// `"windows"`, `"x86_64"`, `"aarch64"`), so the comparison is a string match rather
/// than a table of names for the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactTarget {
    pub os: &'static str,
    pub arch: &'static str,
}

impl ArtifactTarget {
    /// `linux-x86_64`, the shape a release asset name uses.
    pub fn describe(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }
}

/// The platform this process is running on, which is what an artifact has to agree
/// with: `chaos-remote-server` installs a build of itself, not of something else.
pub fn this_host() -> ArtifactTarget {
    ArtifactTarget {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// `e_machine` / Mach-O `cputype` / PE `Machine` values for the platforms this
/// project ships. Anything else is unknown territory and gets no verdict.
const ELF_X86_64: u16 = 0x3e;
const ELF_AARCH64: u16 = 0xb7;
const ELF_I386: u16 = 0x03;
const ELF_ARM: u16 = 0x28;
/// CPU_TYPE_X86 and CPU_TYPE_ARM with the CPU_ARCH_ABI64 bit set.
const MACHO_X86_64: u32 = 0x0100_0007;
const MACHO_ARM64: u32 = 0x0100_000c;
const PE_AMD64: u16 = 0x8664;
const PE_ARM64: u16 = 0xaa64;
const PE_I386: u16 = 0x014c;

/// How many bytes of the file are needed to classify it. The header readers below
/// look at the first 64 bytes plus the PE header offset, so this is generous.
pub const HEADER_BYTES: u64 = 512;

/// The platform named by the leading bytes of a native binary, or `None` when those
/// bytes do not say. `None` covers scripts, unknown formats and unknown
/// architectures, and means "no opinion", not "wrong".
pub fn target_of(head: &[u8]) -> Option<ArtifactTarget> {
    if head.len() < 4 {
        return None;
    }
    if head.starts_with(b"\x7fELF") {
        return elf_target(head);
    }
    if let Some(target) = macho_target(head) {
        return Some(target);
    }
    pe_target(head)
}

/// Refuse an artifact whose header names a platform other than the one it is being
/// installed on. The reason is phrased for whoever is reading the deploy output,
/// which is the first and only place this message appears.
pub fn refusal(head: &[u8], host: ArtifactTarget) -> Option<String> {
    let target = target_of(head)?;
    if target.os == host.os && target.arch == host.arch {
        return None;
    }
    // Two different failures get two different sentences, because the fix differs:
    // a build for another CPU is the wrong file, a build for another OS usually
    // means the wrong release channel or a copied asset.
    let problem = if target.os != host.os {
        format!(
            "it is a {} build and this host is {}",
            target.describe(),
            host.describe()
        )
    } else {
        format!(
            "it is a {} binary and this host is {}",
            target.describe(),
            host.describe()
        )
    };
    Some(format!(
        "wrong_platform: the artifact could not start here — {problem}. Build it on \
         this host, or deploy the release asset named chaos-{}",
        host.describe(),
    ))
}

/// Read `path`'s header and apply [`refusal`]. Only the first [`HEADER_BYTES`] are
/// read, so this stays cheap next to a checksum over the whole file.
pub fn check_file(path: &std::path::Path, host: ArtifactTarget) -> Result<(), String> {
    use std::io::Read as _;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let max = HEADER_BYTES as usize;
    let mut head = vec![0u8; max];
    let read = file
        .read(&mut head)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    head.truncate(read);
    match refusal(&head, host) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// ELF: class and byte order are one-byte fields, `e_machine` is a 16-bit field at
/// offset 18 in the file's own byte order.
fn elf_target(head: &[u8]) -> Option<ArtifactTarget> {
    // 20 bytes is what it takes to reach the end of e_machine; a shorter file is a
    // truncated upload, which the empty-file and digest checks already handle.
    if head.len() < 20 {
        return None;
    }
    // EI_DATA: 1 = little-endian, 2 = big-endian. Anything else is not a file this
    // reader can trust, including a corrupt header.
    let little_endian = match head[5] {
        1 => true,
        2 => false,
        _ => return None,
    };
    let machine = u16_at(head, 18, little_endian)?;
    // An ELF is Unix-ish at best; the host check below does the rest. Naming the OS
    // from the class alone would be a guess for the BSDs, so the OS comparison is
    // done against "linux", which is where the project's Linux assets are built.
    let arch = match machine {
        ELF_X86_64 if head[4] == 2 => "x86_64",
        ELF_AARCH64 if head[4] == 2 => "aarch64",
        ELF_I386 if head[4] == 1 => "x86",
        ELF_ARM if head[4] == 1 => "arm",
        _ => return None,
    };
    Some(ArtifactTarget { os: "linux", arch })
}

/// Mach-O. The magic says both the word size and the byte order of the file, which is
/// why there are four of them and why `cputype` is read back in the file's order.
fn macho_target(head: &[u8]) -> Option<ArtifactTarget> {
    if head.len() < 8 {
        return None;
    }
    // The magic says both the word size and the byte order of the file, which is why
    // there are four of them. `cputype` is then read back in the file's own order.
    // The 0xce pair is a 32-bit Mach-O: still a verdict about the CPU, and no host
    // this project ships would run it.
    let cpu = match head[0..4] {
        [0xcf, 0xfa, 0xed, 0xfe] => u32_at(head, 4, true)?,
        [0xfe, 0xed, 0xfa, 0xcf] => u32_at(head, 4, false)?,
        [0xce, 0xfa, 0xed, 0xfe] => u32_at(head, 4, true)?,
        [0xfe, 0xed, 0xfa, 0xce] => u32_at(head, 4, false)?,
        _ => return None,
    };
    let arch = match cpu {
        MACHO_X86_64 => "x86_64",
        MACHO_ARM64 => "aarch64",
        _ => return None,
    };
    Some(ArtifactTarget { os: "macos", arch })
}

/// PE: `MZ`, then a little-endian offset to the `PE\0\0` signature at 0x3C, then the
/// 16-bit `Machine` four bytes later. Every one of those hops is bounds-checked,
/// because the offset comes from the file.
fn pe_target(head: &[u8]) -> Option<ArtifactTarget> {
    if !head.starts_with(b"MZ") || head.len() < 0x40 {
        return None;
    }
    let lfanew = u32_at(head, 0x3c, true)? as usize;
    if head.len() < lfanew + 6 || !head[lfanew..].starts_with(b"PE\0\0") {
        return None;
    }
    let machine = u16_at(head, lfanew + 4, true)?;
    let arch = match machine {
        PE_AMD64 => "x86_64",
        PE_ARM64 => "aarch64",
        PE_I386 => "x86",
        _ => return None,
    };
    Some(ArtifactTarget {
        os: "windows",
        arch,
    })
}

fn u16_at(bytes: &[u8], at: usize, little_endian: bool) -> Option<u16> {
    let field = bytes.get(at..at + 2)?;
    let raw = [field[0], field[1]];
    Some(if little_endian {
        u16::from_le_bytes(raw)
    } else {
        u16::from_be_bytes(raw)
    })
}

fn u32_at(bytes: &[u8], at: usize, little_endian: bool) -> Option<u32> {
    let field = bytes.get(at..at + 4)?;
    let raw = [field[0], field[1], field[2], field[3]];
    Some(if little_endian {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

/// A header for `target`, enough like a real binary for the reader above to classify
/// it. Test-only: a caller that wanted to *build* an artifact would be building it
/// with a linker, not with this.
#[cfg(test)]
pub fn header_for(target: ArtifactTarget) -> Vec<u8> {
    let mut head = vec![0u8; 0x100];
    match target.os {
        "linux" => {
            head[0..4].copy_from_slice(b"\x7fELF");
            head[4] = 2; // ELFCLASS64
            head[5] = 1; // little-endian
            let machine: u16 = match target.arch {
                "x86_64" => ELF_X86_64,
                "aarch64" => ELF_AARCH64,
                other => panic!("no ELF machine for {other}"),
            };
            head[18..20].copy_from_slice(&machine.to_le_bytes());
        }
        "macos" => {
            head[0..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            let cpu: u32 = match target.arch {
                "x86_64" => MACHO_X86_64,
                "aarch64" => MACHO_ARM64,
                other => panic!("no Mach-O cputype for {other}"),
            };
            head[4..8].copy_from_slice(&cpu.to_le_bytes());
        }
        "windows" => {
            head[0..2].copy_from_slice(b"MZ");
            head[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
            head[0x80..0x84].copy_from_slice(b"PE\0\0");
            let machine: u16 = match target.arch {
                "x86_64" => PE_AMD64,
                "aarch64" => PE_ARM64,
                other => panic!("no PE machine for {other}"),
            };
            head[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        }
        other => panic!("no header for {other}"),
    }
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of the module: this host's own build is accepted, and the same
    /// container's format for a different CPU is not.
    #[test]
    fn a_header_for_this_host_is_accepted() {
        let host = this_host();
        let head = header_for(host);
        assert_eq!(
            target_of(&head),
            Some(host),
            "the reader cannot read back what header_for wrote for {host:?}"
        );
        assert!(refusal(&head, host).is_none());
    }

    #[test]
    fn a_header_for_another_architecture_is_refused_by_name() {
        let host = this_host();
        let other_arch = if host.arch == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let head = header_for(ArtifactTarget {
            os: host.os,
            arch: other_arch,
        });
        let reason = refusal(&head, host).expect("a binary for the wrong CPU is refused");
        assert!(reason.starts_with("wrong_platform"), "{reason}");
        assert!(reason.contains(other_arch), "{reason}");
        assert!(reason.contains(host.arch), "{reason}");
    }

    #[test]
    fn a_header_for_another_operating_system_names_both() {
        let host = this_host();
        let foreign_os = match host.os {
            "linux" => "windows",
            _ => "linux",
        };
        let head = header_for(ArtifactTarget {
            os: foreign_os,
            arch: host.arch,
        });
        let reason = refusal(&head, host).expect("a build for another OS is refused");
        assert!(reason.contains(foreign_os), "{reason}");
        assert!(reason.contains(&host.describe()), "{reason}");
    }

    #[test]
    fn a_big_endian_elf_is_read_in_its_own_byte_order() {
        let mut head = header_for(ArtifactTarget {
            os: "linux",
            arch: "aarch64",
        });
        // Same file, big-endian: flipping EI_DATA has to move e_machine's bytes too,
        // or the reader would be reading garbage and agreeing by luck.
        head[5] = 2;
        head[18..20].copy_from_slice(&ELF_AARCH64.to_be_bytes());
        assert_eq!(
            target_of(&head).map(|t| t.arch),
            Some("aarch64"),
            "the machine field was read in the wrong order"
        );
    }

    #[test]
    fn a_byte_swapped_mach_o_is_read_in_its_own_byte_order() {
        let mut head = header_for(ArtifactTarget {
            os: "macos",
            arch: "aarch64",
        });
        head[0..4].copy_from_slice(&[0xfe, 0xed, 0xfa, 0xcf]);
        head[4..8].copy_from_slice(&MACHO_ARM64.to_be_bytes());
        assert_eq!(target_of(&head).map(|t| t.arch), Some("aarch64"));
    }

    /// A shell wrapper is a legitimate thing to deploy — the install path has always
    /// accepted one — and a file this short is somebody else's problem (the digest
    /// and empty-file checks run first). None of these may be refused.
    #[test]
    fn a_file_that_does_not_state_a_platform_gets_no_verdict() {
        let host = this_host();
        for head in [
            b"#!/bin/sh\nexec chaos-remote-server \"$@\"\n".as_slice(),
            b"MZ".as_slice(),
            b"\x7fELF".as_slice(),
            b"random bytes".as_slice(),
            [].as_slice(),
        ] {
            assert_eq!(target_of(head), None, "{head:?} should not be classified");
            assert!(
                refusal(head, host).is_none(),
                "{head:?} was refused: {:?}",
                refusal(head, host)
            );
        }
    }

    /// An architecture this module has no entry for is unknown, not wrong: refusing
    /// it would block a host this list simply has not caught up with.
    #[test]
    fn an_unknown_machine_is_no_verdict() {
        let mut head = header_for(this_host());
        head[18..20].copy_from_slice(&0xf3u16.to_le_bytes()); // EM_RISCV
        assert_eq!(target_of(&head), None);
    }

    /// A bogus `e_lfanew` must not send the reader outside the buffer.
    #[test]
    fn a_pe_header_pointing_past_the_file_is_no_verdict() {
        let mut head = header_for(ArtifactTarget {
            os: "windows",
            arch: "x86_64",
        });
        head[0x3c..0x40].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
        assert_eq!(target_of(&head), None);
    }

    #[test]
    fn check_file_agrees_with_the_byte_level_decision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("chaos-remote-server");
        let host = this_host();
        std::fs::write(&path, header_for(host)).expect("write the header");
        check_file(&path, host).expect("this host's own build is fine");
        let other_arch = if host.arch == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        std::fs::write(
            &path,
            header_for(ArtifactTarget {
                os: host.os,
                arch: other_arch,
            }),
        )
        .expect("write the other header");
        let error = check_file(&path, host).expect_err("the wrong CPU is refused");
        assert!(error.starts_with("wrong_platform"), "{error}");
    }

    /// A file shorter than the read buffer still gets a verdict, which is the path a
    /// test fixture or a wrapper takes.
    #[test]
    fn check_file_reads_less_than_its_buffer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tiny");
        std::fs::write(&path, b"#!/bin/sh\n").expect("write");
        check_file(&path, this_host()).expect("a script is not a platform claim");
    }
}
