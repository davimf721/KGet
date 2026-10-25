//! Remote and local ZIP Central Directory previewer.
//!
//! Inspects archive contents without downloading the entire file by requesting
//! only the End of Central Directory (EOCD) and Central Directory records via
//! HTTP Range requests.

use humansize::{DECIMAL, format_size};
use std::error::Error;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CDFH_SIGNATURE: u32 = 0x0201_4b50;
const EOCD_MIN_SIZE: usize = 22;

/// Metadata for an individual entry in a ZIP archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntryInfo {
    /// File or directory path within the archive.
    pub name: String,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// True if the entry represents a directory.
    pub is_directory: bool,
    /// CRC-32 checksum.
    pub crc32: u32,
    /// Compression method ID (0 = stored, 8 = deflate).
    pub compression_method: u16,
}

impl ZipEntryInfo {
    /// Format compression method name.
    pub fn method_name(&self) -> &'static str {
        match self.compression_method {
            0 => "Stored",
            8 => "Deflate",
            12 => "BZIP2",
            14 => "LZMA",
            93 => "ZSTD",
            _ => "Other",
        }
    }
}

/// Parse Central Directory File Header entries from a byte slice.
pub fn parse_cdfh_entries(
    mut bytes: &[u8],
    expected_count: usize,
) -> Result<Vec<ZipEntryInfo>, String> {
    let mut entries = Vec::with_capacity(expected_count.min(1000));
    while bytes.len() >= 46 {
        let sig = u32::from_le_bytes(bytes[0..4].try_into().map_err(|e| format!("{e}"))?);
        if sig != CDFH_SIGNATURE {
            break;
        }

        let compression = u16::from_le_bytes(bytes[10..12].try_into().map_err(|e| format!("{e}"))?);
        let crc32 = u32::from_le_bytes(bytes[16..20].try_into().map_err(|e| format!("{e}"))?);
        let comp_size =
            u32::from_le_bytes(bytes[20..24].try_into().map_err(|e| format!("{e}"))?) as u64;
        let uncomp_size =
            u32::from_le_bytes(bytes[24..28].try_into().map_err(|e| format!("{e}"))?) as u64;
        let name_len =
            u16::from_le_bytes(bytes[28..30].try_into().map_err(|e| format!("{e}"))?) as usize;
        let extra_len =
            u16::from_le_bytes(bytes[30..32].try_into().map_err(|e| format!("{e}"))?) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[32..34].try_into().map_err(|e| format!("{e}"))?) as usize;

        let total_header_len = 46 + name_len + extra_len + comment_len;
        if bytes.len() < total_header_len {
            break;
        }

        let name_bytes = &bytes[46..46 + name_len];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        let is_directory = name.ends_with('/') || name.ends_with('\\');

        entries.push(ZipEntryInfo {
            name,
            uncompressed_size: uncomp_size,
            compressed_size: comp_size,
            is_directory,
            crc32,
            compression_method: compression,
        });

        bytes = &bytes[total_header_len..];
    }

    Ok(entries)
}

/// Parse the ZIP Central Directory from a byte buffer representing the tail of a ZIP file.
pub fn parse_central_directory(bytes: &[u8]) -> Result<Vec<ZipEntryInfo>, String> {
    if bytes.len() < EOCD_MIN_SIZE {
        return Err("Payload too small for ZIP central directory".into());
    }

    let mut eocd_pos = None;
    let max_search = bytes.len().min(65536 + EOCD_MIN_SIZE);
    let start_scan = bytes.len() - max_search;
    for i in (start_scan..=bytes.len() - EOCD_MIN_SIZE).rev() {
        if let Ok(sig_bytes) = bytes[i..i + 4].try_into()
            && u32::from_le_bytes(sig_bytes) == EOCD_SIGNATURE
        {
            eocd_pos = Some(i);
            break;
        }
    }

    let eocd_idx = eocd_pos
        .ok_or_else(|| "Could not locate ZIP End of Central Directory signature".to_string())?;
    let eocd = &bytes[eocd_idx..];
    if eocd.len() < 22 {
        return Err("Invalid End of Central Directory record size".into());
    }

    let total_entries =
        u16::from_le_bytes(eocd[10..12].try_into().map_err(|e| format!("{e}"))?) as usize;
    let cd_size = u32::from_le_bytes(eocd[12..16].try_into().map_err(|e| format!("{e}"))?) as usize;

    let cd_bytes = if eocd_idx >= cd_size {
        &bytes[eocd_idx - cd_size..eocd_idx]
    } else {
        bytes
    };

    parse_cdfh_entries(cd_bytes, total_entries)
}

/// Print formatted preview table of ZIP archive entries.
pub fn print_zip_preview(entries: &[ZipEntryInfo]) {
    println!(
        "{:<45} {:>12} {:>12} {:>10} {:>10}",
        "Name", "Size", "Compressed", "Method", "CRC-32"
    );
    println!("{:-<95}", "");
    let mut total_uncompressed = 0u64;
    let mut total_compressed = 0u64;
    let mut file_count = 0usize;

    for entry in entries {
        total_uncompressed += entry.uncompressed_size;
        total_compressed += entry.compressed_size;
        if !entry.is_directory {
            file_count += 1;
        }
        let size_str = format_size(entry.uncompressed_size, DECIMAL);
        let comp_str = format_size(entry.compressed_size, DECIMAL);
        let crc_str = format!("{:08X}", entry.crc32);
        println!(
            "{:<45} {:>12} {:>12} {:>10} {:>10}",
            entry.name,
            size_str,
            comp_str,
            entry.method_name(),
            crc_str
        );
    }
    println!("{:-<95}", "");
    println!(
        "Total: {} files ({} uncompressed, {} compressed)",
        file_count,
        format_size(total_uncompressed, DECIMAL),
        format_size(total_compressed, DECIMAL)
    );
}

/// Inspect remote or local ZIP contents without downloading the whole file.
pub fn inspect_zip_target(
    target: &str,
    client: &reqwest::blocking::Client,
    extra_headers: &[(String, String)],
) -> Result<Vec<ZipEntryInfo>, Box<dyn Error + Send + Sync>> {
    if !target.starts_with("http://") && !target.starts_with("https://") {
        let bytes = std::fs::read(target)?;
        return parse_central_directory(&bytes).map_err(|e| e.into());
    }

    let mut req = client
        .get(target)
        .header(reqwest::header::RANGE, "bytes=-65558");
    req = crate::http::apply_headers(req, extra_headers);
    let resp = req.send()?;
    if !resp.status().is_success() {
        return Err(format!("Server returned HTTP {} for range request", resp.status()).into());
    }
    let body = resp.bytes()?;
    parse_central_directory(&body).map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_synthetic_zip_central_directory() {
        let mut buf = Vec::new();

        // 1. Central Directory File Header 1 ("test.txt", uncomp: 12, comp: 12)
        buf.extend_from_slice(&CDFH_SIGNATURE.to_le_bytes());
        buf.extend_from_slice(&[20, 0]); // version made by
        buf.extend_from_slice(&[20, 0]); // version needed
        buf.extend_from_slice(&[0, 0]); // flags
        buf.extend_from_slice(&[0, 0]); // method (stored)
        buf.extend_from_slice(&[0, 0, 0, 0]); // time
        buf.extend_from_slice(&0x1234_5678u32.to_le_bytes()); // crc32
        buf.extend_from_slice(&12u32.to_le_bytes()); // comp size
        buf.extend_from_slice(&12u32.to_le_bytes()); // uncomp size
        let filename = b"test.txt";
        buf.extend_from_slice(&(filename.len() as u16).to_le_bytes()); // name len
        buf.extend_from_slice(&[0, 0]); // extra len
        buf.extend_from_slice(&[0, 0]); // comment len
        buf.extend_from_slice(&[0, 0]); // disk start
        buf.extend_from_slice(&[0, 0]); // int attr
        buf.extend_from_slice(&[0, 0, 0, 0]); // ext attr
        buf.extend_from_slice(&[0, 0, 0, 0]); // rel offset
        buf.extend_from_slice(filename);

        let cd_size = buf.len() as u32;

        // 2. End of Central Directory Record
        buf.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        buf.extend_from_slice(&[0, 0]); // disk number
        buf.extend_from_slice(&[0, 0]); // start disk
        buf.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
        buf.extend_from_slice(&1u16.to_le_bytes()); // total entries
        buf.extend_from_slice(&cd_size.to_le_bytes()); // cd size
        buf.extend_from_slice(&0u32.to_le_bytes()); // cd offset
        buf.extend_from_slice(&[0, 0]); // comment len

        let entries = parse_central_directory(&buf).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test.txt");
        assert_eq!(entries[0].uncompressed_size, 12);
        assert_eq!(entries[0].compressed_size, 12);
        assert_eq!(entries[0].crc32, 0x1234_5678);
        assert_eq!(entries[0].method_name(), "Stored");
    }
}
