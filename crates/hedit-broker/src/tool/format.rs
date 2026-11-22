//! Minimal native-header classification, not a loader or signature validator.
//! The OS performs complete executable validation after native confinement.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Read, Seek, SeekFrom},
};

pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    Aarch64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub platform: Platform,
    pub architectures: BTreeSet<Architecture>,
}
impl Header {
    pub fn supports_host(&self) -> bool {
        let platform = if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            return false;
        };
        let architecture = if cfg!(target_arch = "x86_64") {
            Architecture::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else {
            return false;
        };
        self.platform == platform && self.architectures.contains(&architecture)
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Choose a supported standalone native executable.",
    )
}
struct Reader<'a, R> {
    source: &'a mut R,
    size: u64,
    remaining: usize,
}
impl<R: Read + Seek> Reader<'_, R> {
    fn at(&mut self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        if length > self.remaining
            || offset
                .checked_add(length as u64)
                .is_none_or(|end| end > self.size)
        {
            return Err(invalid());
        }
        self.remaining -= length;
        self.source.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; length];
        self.source.read_exact(&mut bytes)?;
        Ok(bytes)
    }
    fn macho(&mut self, offset: u64, size: u64) -> io::Result<(Architecture, u32, u32)> {
        if size < 32 {
            return Err(invalid());
        }
        let header = self.at(offset, 32)?;
        if header[..4] != [0xcf, 0xfa, 0xed, 0xfe] || le32(&header, 12) != 2 {
            return Err(invalid());
        }
        let cpu = le32(&header, 4);
        let architecture = match cpu {
            0x0100_0007 => Architecture::X86_64,
            0x0100_000c => Architecture::Aarch64,
            _ => return Err(invalid()),
        };
        Ok((architecture, cpu, le32(&header, 8)))
    }
}
fn le16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn le64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn be64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// Read at most 64 KiB of headers from an already pinned regular file. Never
/// execute it, load libraries, run --version or interpret package scripts.
pub fn inspect(source: &mut (impl Read + Seek), size: u64) -> io::Result<Header> {
    if !(32..=MAX_IMAGE_BYTES).contains(&size) {
        return Err(invalid());
    }
    let mut reader = Reader {
        source,
        size,
        remaining: MAX_HEADER_BYTES,
    };
    let magic = reader.at(0, 4)?;
    if magic == b"\x7fELF" {
        let bytes = reader.at(0, 64)?;
        if bytes[4..7] != [2, 1, 1]
            || !matches!(le16(&bytes, 16), 2 | 3)
            || le32(&bytes, 20) != 1
            || le64(&bytes, 24) == 0
            || le16(&bytes, 52) != 64
        {
            return Err(invalid());
        }
        let architecture = match le16(&bytes, 18) {
            62 => Architecture::X86_64,
            183 => Architecture::Aarch64,
            _ => return Err(invalid()),
        };
        return Ok(Header {
            platform: Platform::Linux,
            architectures: BTreeSet::from([architecture]),
        });
    }
    if magic[..2] == *b"MZ" {
        let dos = reader.at(0, 64)?;
        let offset = u64::from(le32(&dos, 60));
        if !(64..=1024 * 1024).contains(&offset) {
            return Err(invalid());
        }
        let bytes = reader.at(offset, 24 + 112)?;
        if bytes[..4] != *b"PE\0\0"
            || le16(&bytes, 20) < 112
            || le16(&bytes, 24) != 0x20b
            || le16(&bytes, 22) & 0x2002 != 2
        {
            return Err(invalid());
        }
        let architecture = match le16(&bytes, 4) {
            0x8664 => Architecture::X86_64,
            0xaa64 => Architecture::Aarch64,
            _ => return Err(invalid()),
        };
        return Ok(Header {
            platform: Platform::Windows,
            architectures: BTreeSet::from([architecture]),
        });
    }
    if magic == [0xcf, 0xfa, 0xed, 0xfe] {
        return Ok(Header {
            platform: Platform::Macos,
            architectures: BTreeSet::from([reader.macho(0, size)?.0]),
        });
    }
    let fat64 = match magic.as_slice() {
        [0xca, 0xfe, 0xba, 0xbe] => false,
        [0xca, 0xfe, 0xba, 0xbf] => true,
        _ => return Err(invalid()),
    };
    let count = be32(&reader.at(4, 4)?, 0) as usize;
    if !(1..=16).contains(&count) {
        return Err(invalid());
    }
    let stride = if fat64 { 32 } else { 20 };
    let table = reader.at(8, count * stride)?;
    let mut architectures = BTreeSet::new();
    let mut cpus = BTreeSet::new();
    let mut ranges = Vec::new();
    for entry in table.chunks_exact(stride) {
        let (offset, length, align) = if fat64 {
            (be64(entry, 8), be64(entry, 16), be32(entry, 24))
        } else {
            (
                u64::from(be32(entry, 8)),
                u64::from(be32(entry, 12)),
                be32(entry, 16),
            )
        };
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= size)
            .ok_or_else(invalid)?;
        if offset < (8 + table.len()) as u64
            || align > 31
            || offset & ((1u64 << align) - 1) != 0
            || ranges
                .iter()
                .any(|(start, stop)| offset < *stop && *start < end)
        {
            return Err(invalid());
        }
        let (architecture, cpu, subtype) = reader.macho(offset, length)?;
        if cpu != be32(entry, 0) || subtype != be32(entry, 4) || !cpus.insert((cpu, subtype)) {
            return Err(invalid());
        }
        ranges.push((offset, end));
        architectures.insert(architecture);
    }
    Ok(Header {
        platform: Platform::Macos,
        architectures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn thin(cpu: u32) -> Vec<u8> {
        let mut bytes = vec![0; 64];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
        bytes
    }
    fn parse(bytes: &[u8]) -> io::Result<Header> {
        inspect(&mut Cursor::new(bytes), bytes.len() as u64)
    }
    #[test]
    fn native_image_headers_reject_scripts_dlls_architecture_and_truncation() {
        assert!(parse(&[b'#'; 64]).is_err());
        assert_eq!(
            parse(&thin(0x0100_000c)).unwrap().architectures,
            BTreeSet::from([Architecture::Aarch64])
        );
        let mut elf = vec![0; 256];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[24..32].copy_from_slice(&4096u64.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        assert_eq!(parse(&elf).unwrap().platform, Platform::Linux);
        let mut pe = vec![0; 256];
        pe[..2].copy_from_slice(b"MZ");
        pe[60..64].copy_from_slice(&64u32.to_le_bytes());
        pe[64..68].copy_from_slice(b"PE\0\0");
        pe[68..70].copy_from_slice(&0xaa64u16.to_le_bytes());
        pe[84..86].copy_from_slice(&112u16.to_le_bytes());
        pe[86..88].copy_from_slice(&2u16.to_le_bytes());
        pe[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        assert_eq!(parse(&pe).unwrap().platform, Platform::Windows);
        for bytes in [&thin(0x0100_0007), &elf, &pe] {
            for length in 0..32 {
                assert!(parse(&bytes[..length]).is_err());
            }
        }
        pe[86..88].copy_from_slice(&0x2002u16.to_le_bytes());
        assert!(parse(&pe).is_err());
        elf[18..20].copy_from_slice(&3u16.to_le_bytes());
        assert!(parse(&elf).is_err());
        let mut library = thin(0x0100_000c);
        library[12..16].copy_from_slice(&6u32.to_le_bytes());
        assert!(parse(&library).is_err());
    }
    #[test]
    fn universal_slices_are_bounded_and_cannot_overlap_or_disagree() {
        let mut bytes = vec![0; 512];
        bytes[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        bytes[4..8].copy_from_slice(&2u32.to_be_bytes());
        for (index, (cpu, offset)) in [(0x0100_0007u32, 256u32), (0x0100_000cu32, 384u32)]
            .into_iter()
            .enumerate()
        {
            let base = 8 + index * 20;
            bytes[base..base + 4].copy_from_slice(&cpu.to_be_bytes());
            bytes[base + 8..base + 12].copy_from_slice(&offset.to_be_bytes());
            bytes[base + 12..base + 16].copy_from_slice(&64u32.to_be_bytes());
            bytes[base + 16..base + 20].copy_from_slice(&4u32.to_be_bytes());
            bytes[offset as usize..offset as usize + 64].copy_from_slice(&thin(cpu));
        }
        assert_eq!(parse(&bytes).unwrap().architectures.len(), 2);
        let mut bad = bytes.clone();
        bad[36..40].copy_from_slice(&256u32.to_be_bytes());
        assert!(parse(&bad).is_err());
        let mut bad = bytes.clone();
        bad[28..32].copy_from_slice(&0x0100_0007u32.to_be_bytes());
        assert!(parse(&bad).is_err());
        bytes[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn malformed_headers_stay_within_the_read_budget() {
        struct Counted {
            inner: Cursor<Vec<u8>>,
            read: usize,
        }
        impl Read for Counted {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                let count = self.inner.read(bytes)?;
                self.read += count;
                assert!(self.read <= MAX_HEADER_BYTES);
                Ok(count)
            }
        }
        impl Seek for Counted {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                self.inner.seek(from)
            }
        }
        let mut seed = 0x42a1_0f19_2c4e_56d7u64;
        for iteration in 0..10000 {
            let mut bytes = vec![0u8; 512];
            for byte in &mut bytes {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *byte = seed as u8;
            }
            // Exercise each format's offset/count parser, not only bad magic.
            let magic: &[u8] = match iteration % 5 {
                0 => b"MZ",
                1 => b"\x7fELF",
                2 => &[0xca, 0xfe, 0xba, 0xbe],
                3 => &[0xca, 0xfe, 0xba, 0xbf],
                _ => &[0xcf, 0xfa, 0xed, 0xfe],
            };
            bytes[..magic.len()].copy_from_slice(magic);
            let mut source = Counted {
                inner: Cursor::new(bytes),
                read: 0,
            };
            let _ = inspect(
                &mut source,
                if iteration % 2 == 0 {
                    512
                } else {
                    MAX_IMAGE_BYTES
                },
            );
        }
    }
    #[test]
    fn installed_test_executable_is_classified_without_running_it() {
        let mut file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let length = file.metadata().unwrap().len();
        assert!(inspect(&mut file, length).unwrap().supports_host());
    }
}
