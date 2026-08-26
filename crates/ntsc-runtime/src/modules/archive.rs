//! NTSC standard library: `archive` module.
//! Minimal tar.gz and zip extraction for package manager use.
//!
//! Archive functions accept file paths (not raw data) since binary content
//! cannot travel through the string registry.

use std::io::Read;

use crate::registry;

fn fail(fn_name: &str, msg: impl std::fmt::Display) -> i64 {
    super::throw_str(format!("archive.{fn_name}: {msg}"))
}

/// `archive.extract_tar_gz(path, dest)` — extract a tar.gz file at `path`
/// into the destination directory. Returns the number of files extracted.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_archive_extract_tar_gz(path: i64, dest: i64) -> i64 {
    let path_str = registry::get_string(path).unwrap_or_default();
    let dest_str = registry::get_string(dest).unwrap_or_default();
    let dest_path = std::path::Path::new(&dest_str);

    let file = match std::fs::File::open(&path_str) {
        Ok(f) => f,
        Err(e) => return fail("extract_tar_gz", format!("cannot open '{path_str}': {e}")),
    };
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    match archive.unpack(dest_path) {
        Ok(()) => {
            let count = count_files(dest_path);
            registry::put_string(count.to_string())
        }
        Err(e) => fail(
            "extract_tar_gz",
            format!("failed to extract '{path_str}': {e}"),
        ),
    }
}

/// `archive.extract_tar(path, dest)` — extract an uncompressed tar file.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_archive_extract_tar(path: i64, dest: i64) -> i64 {
    let path_str = registry::get_string(path).unwrap_or_default();
    let dest_str = registry::get_string(dest).unwrap_or_default();
    let dest_path = std::path::Path::new(&dest_str);

    let file = match std::fs::File::open(&path_str) {
        Ok(f) => f,
        Err(e) => return fail("extract_tar", format!("cannot open '{path_str}': {e}")),
    };
    let mut archive = tar::Archive::new(file);

    match archive.unpack(dest_path) {
        Ok(()) => {
            let count = count_files(dest_path);
            registry::put_string(count.to_string())
        }
        Err(e) => fail(
            "extract_tar",
            format!("failed to extract '{path_str}': {e}"),
        ),
    }
}

/// `archive.extract_zip(path, dest)` — extract a zip file at `path`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_archive_extract_zip(path: i64, dest: i64) -> i64 {
    let path_str = registry::get_string(path).unwrap_or_default();
    let dest_str = registry::get_string(dest).unwrap_or_default();
    let dest_path = std::path::Path::new(&dest_str);

    let data = match std::fs::read(&path_str) {
        Ok(d) => d,
        Err(e) => return fail("extract_zip", format!("cannot read '{path_str}': {e}")),
    };

    match extract_zip_bytes(&data, dest_path) {
        Ok(count) => registry::put_string(count.to_string()),
        Err(e) => fail(
            "extract_zip",
            format!("failed to extract '{path_str}': {e}"),
        ),
    }
}

/// `archive.list_tar(path)` — list entry names in a tar file,
/// newline-separated.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_archive_list_tar(path: i64) -> i64 {
    let path_str = registry::get_string(path).unwrap_or_default();

    let file = match std::fs::File::open(&path_str) {
        Ok(f) => f,
        Err(e) => return fail("list_tar", format!("cannot open '{path_str}': {e}")),
    };
    let mut archive = tar::Archive::new(file);

    let mut entries = Vec::new();
    match archive.entries() {
        Ok(entry_iter) => {
            for entry in entry_iter.flatten() {
                let entry_path = entry.path().unwrap_or_default();
                entries.push(entry_path.to_string_lossy().to_string());
            }
        }
        Err(e) => return fail("list_tar", format!("failed to list '{path_str}': {e}")),
    }
    registry::put_string(entries.join("\n"))
}

/// `archive.list_zip(path)` — list entry names in a zip file,
/// newline-separated.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_archive_list_zip(path: i64) -> i64 {
    let path_str = registry::get_string(path).unwrap_or_default();

    let data = match std::fs::read(&path_str) {
        Ok(d) => d,
        Err(e) => return fail("list_zip", format!("cannot read '{path_str}': {e}")),
    };

    match list_zip_entries(&data) {
        Ok(entries) => registry::put_string(entries.join("\n")),
        Err(e) => fail("list_zip", format!("failed to list '{path_str}': {e}")),
    }
}

fn count_files(dir: &std::path::Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                count += 1;
            }
            if entry.path().is_dir() {
                count += count_files(&entry.path());
            }
        }
    }
    count
}

/// Minimal zip extractor: reads the central directory and extracts
/// deflated entries.
fn extract_zip_bytes(data: &[u8], dest: &std::path::Path) -> Result<usize, String> {
    let eocd_pos = find_eocd(data)
        .ok_or_else(|| "not a valid zip archive: end of central directory not found".to_string())?;

    let num_entries = read_u16_le(data, eocd_pos + 10) as usize;
    let cd_offset = read_u32_le(data, eocd_pos + 16) as usize;

    let mut count = 0;
    let mut offset = cd_offset;

    for _ in 0..num_entries {
        if offset + 46 > data.len() {
            break;
        }
        if read_u32_le(data, offset) != 0x02014b50 {
            break;
        }
        let comp_method = read_u16_le(data, offset + 10);
        let comp_size = read_u32_le(data, offset + 20) as usize;
        let uncomp_size = read_u32_le(data, offset + 24) as usize;
        let name_len = read_u16_le(data, offset + 28) as usize;
        let extra_len = read_u16_le(data, offset + 30) as usize;
        let comment_len = read_u16_le(data, offset + 32) as usize;
        let local_offset = read_u32_le(data, offset + 42) as usize;

        if offset + 46 + name_len + extra_len + comment_len > data.len() {
            break;
        }
        let name = std::str::from_utf8(&data[offset + 46..offset + 46 + name_len]).unwrap_or("");

        if !name.is_empty() && !name.ends_with('/') && local_offset + 30 + name_len <= data.len() {
            let local_name_len = read_u16_le(data, local_offset + 26) as usize;
            let local_extra_len = read_u16_le(data, local_offset + 28) as usize;
            let data_start = local_offset + 30 + local_name_len + local_extra_len;

            if data_start + comp_size <= data.len() {
                let compressed = &data[data_start..data_start + comp_size];
                let decompressed = match comp_method {
                    0 => compressed.to_vec(),
                    8 => {
                        let mut decoder = flate2::read::DeflateDecoder::new(compressed);
                        let mut buf = Vec::with_capacity(uncomp_size);
                        decoder
                            .read_to_end(&mut buf)
                            .map_err(|e| format!("decompress failed: {e}"))?;
                        buf
                    }
                    m => return Err(format!("unsupported compression method {m}")),
                };

                let out_path = dest.join(name);
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
                }
                std::fs::write(&out_path, &decompressed).map_err(|e| format!("write file: {e}"))?;
                count += 1;
            }
        }

        offset += 46 + name_len + extra_len + comment_len;
    }

    Ok(count)
}

fn list_zip_entries(data: &[u8]) -> Result<Vec<String>, String> {
    let eocd_pos = find_eocd(data).ok_or_else(|| "not a valid zip archive".to_string())?;

    let num_entries = read_u16_le(data, eocd_pos + 10) as usize;
    let cd_offset = read_u32_le(data, eocd_pos + 16) as usize;

    let mut entries = Vec::new();
    let mut offset = cd_offset;

    for _ in 0..num_entries {
        if offset + 46 > data.len() {
            break;
        }
        if read_u32_le(data, offset) != 0x02014b50 {
            break;
        }
        let name_len = read_u16_le(data, offset + 28) as usize;
        let extra_len = read_u16_le(data, offset + 30) as usize;
        let comment_len = read_u16_le(data, offset + 32) as usize;

        if offset + 46 + name_len + extra_len + comment_len > data.len() {
            break;
        }
        let name = std::str::from_utf8(&data[offset + 46..offset + 46 + name_len])
            .unwrap_or("")
            .to_string();

        entries.push(name);
        offset += 46 + name_len + extra_len + comment_len;
    }

    Ok(entries)
}

fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    let max_comment_len = (data.len() - 22).min(65535);
    (0..=max_comment_len)
        .rev()
        .find(|&i| i + 22 <= data.len() && read_u32_le(data, i) == 0x06054b50)
}

fn read_u16_le(data: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes([data[pos], data[pos + 1]])
}

fn read_u32_le(data: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_extract_tar_gz() {
        let dir = std::env::temp_dir().join("ntsc_archive_test_tgz");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create a tar.gz file on disk.
        let tar_gz_path = dir.join("test.tar.gz");
        {
            let mut enc = flate2::write::GzEncoder::new(
                std::fs::File::create(&tar_gz_path).unwrap(),
                flate2::Compression::default(),
            );
            {
                let mut tar_builder = tar::Builder::new(&mut enc);
                let content = b"hello";
                let mut header = tar::Header::new_gnu();
                header.set_size(5);
                header.set_mode(0o644);
                header.set_cksum();
                tar_builder
                    .append_data(&mut header, "test.txt", &content[..])
                    .unwrap();
                tar_builder.finish().unwrap();
            }
            enc.finish().unwrap();
        }

        let dest = dir.join("extracted");
        std::fs::create_dir_all(&dest).unwrap();

        let count = ntsc_archive_extract_tar_gz(
            put(tar_gz_path.to_str().unwrap()),
            put(dest.to_str().unwrap()),
        );
        let count_str = read(count);
        assert_eq!(count_str, "1");
        assert!(dest.join("test.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("test.txt")).unwrap(),
            "hello"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_zip() {
        let dir = std::env::temp_dir().join("ntsc_archive_test_zip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("test.zip");
        create_test_zip(&zip_path);

        let dest = dir.join("extracted");
        std::fs::create_dir_all(&dest).unwrap();

        let count =
            ntsc_archive_extract_zip(put(zip_path.to_str().unwrap()), put(dest.to_str().unwrap()));
        let count_str = read(count);
        assert_eq!(count_str, "1");
        assert!(dest.join("hello.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "world"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_tar() {
        let dir = std::env::temp_dir().join("ntsc_archive_test_list_tar");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let tar_path = dir.join("test.tar");
        {
            let file = std::fs::File::create(&tar_path).unwrap();
            let mut tar_builder = tar::Builder::new(file);
            let content = b"test";
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            tar_builder
                .append_data(&mut header, "a.txt", &content[..])
                .unwrap();
            header.set_path("b.txt").unwrap();
            header.set_cksum();
            tar_builder
                .append_data(&mut header, "b.txt", &content[..])
                .unwrap();
            tar_builder.finish().unwrap();
        }

        let entries = read(ntsc_archive_list_tar(put(tar_path.to_str().unwrap())));
        assert!(entries.contains("a.txt"));
        assert!(entries.contains("b.txt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn create_test_zip(path: &std::path::Path) {
        let name = b"hello.txt";
        let content = b"world";

        let mut data = Vec::new();

        // Local file header
        data.extend_from_slice(&0x04034b50u32.to_le_bytes());
        data.extend_from_slice(&20u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&crc32fast::hash(content).to_le_bytes());
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(name);
        data.extend_from_slice(content);

        let cd_offset = data.len() as u32;

        // Central directory
        data.extend_from_slice(&0x02014b50u32.to_le_bytes());
        data.extend_from_slice(&20u16.to_le_bytes());
        data.extend_from_slice(&20u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&crc32fast::hash(content).to_le_bytes());
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(name);

        let cd_size = data.len() as u32 - cd_offset;

        // End of central directory
        data.extend_from_slice(&0x06054b50u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&cd_size.to_le_bytes());
        data.extend_from_slice(&cd_offset.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());

        std::fs::write(path, &data).unwrap();
    }
}
