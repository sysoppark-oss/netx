//! 전송 폴더. FTP·TFTP는 이 폴더 안의 파일만 올리고, 받은 파일도 이 폴더에만 둔다.

use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];

/// 전송 폴더 안의 파일 이름으로 쓸 수 있는지. 경로가 아니라 이름만 받는다.
pub fn check_name(name: &str) -> Result<(), String> {
    let stem = name.split('.').next().unwrap_or("");
    let ok = !name.is_empty()
        && name.len() <= 120
        && !name.starts_with('.')
        && !name.ends_with(['.', ' '])
        && !name.starts_with(' ')
        && !name.chars().any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
        && !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem));
    if ok { Ok(()) } else { Err(format!("'{}'은(는) 전송 폴더의 파일 이름으로 쓸 수 없습니다 (경로 없이 이름만, 특수 문자 없이)", super::plain(name))) }
}

/// 원격 경로의 마지막 이름
pub fn base_name(remote: &str) -> &str {
    remote.rsplit(['/', '\\']).next().unwrap_or(remote)
}

/// 같은 이름이 있으면 `이름-1.확장자`처럼 비어 있는 이름을 고른다(덮어쓰지 않는다).
pub fn fresh_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..10_000).map(|n| dir.join(format!("{stem}-{n}{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// 올릴 파일을 연다. 전송 폴더 바로 아래의 보통 파일만 된다.
pub fn open_upload(dir: &Path, name: &str) -> Result<(std::fs::File, u64), String> {
    check_name(name)?;
    let path = dir.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| format!("전송 폴더에 '{name}'이(가) 없습니다 (lls로 목록을 볼 수 있습니다)"))?;
    if !meta.is_file() {
        return Err(format!("'{name}'은(는) 파일이 아닙니다"));
    }
    let file = std::fs::File::open(&path).map_err(|e| format!("'{name}'을(를) 열지 못했습니다: {e}"))?;
    Ok((file, meta.len()))
}

/// 전송 폴더의 파일 목록(이름, 크기)
pub fn list(dir: &Path) -> Vec<(String, u64)> {
    let mut items: Vec<(String, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (e.file_name().to_string_lossy().into_owned(), meta.len()))
        })
        .collect();
    items.sort();
    items
}

/// `lls` 명령의 출력
pub fn list_text(dir: &Path) -> String {
    let items = list(dir);
    let mut text = format!("전송 폴더: {}\n", dir.display());
    if items.is_empty() {
        text.push_str("  (비어 있음. 올릴 파일을 이 폴더에 넣으세요)\n");
    }
    for (name, size) in items {
        text.push_str(&format!("  {:>10}  {}\n", super::console::size_text(size), super::plain(&name)));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_only() {
        for ok in ["config.txt", "startup-config", "펌웨어 v1.2.bin", "a"] {
            assert!(check_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", ".ssh", "../a", "a/b", "a\\b", "C:a", "a\nb", "con", "NUL.txt", "com1.cfg", "a.", " a", "a*", &"a".repeat(121)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert_eq!(base_name("/flash/config.text"), "config.text");
        assert_eq!(base_name("flash:\\a.bin"), "a.bin");
        assert_eq!(base_name("a.bin"), "a.bin");
    }

    #[test]
    fn never_overwrites() {
        let dir = std::env::temp_dir().join(format!("netx-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(fresh_path(&dir, "a.cfg"), dir.join("a.cfg"));
        std::fs::write(dir.join("a.cfg"), b"1").unwrap();
        std::fs::write(dir.join("a-1.cfg"), b"22").unwrap();
        std::fs::write(dir.join("noext"), b"").unwrap();
        assert_eq!(fresh_path(&dir, "a.cfg"), dir.join("a-2.cfg"));
        assert_eq!(fresh_path(&dir, "noext"), dir.join("noext-1"));
        assert_eq!(list(&dir), [("a-1.cfg".to_string(), 2), ("a.cfg".to_string(), 1), ("noext".to_string(), 0)]);
        assert_eq!(open_upload(&dir, "a-1.cfg").unwrap().1, 2);
        assert!(open_upload(&dir, "missing").is_err());
        assert!(open_upload(&dir, "../a.cfg").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
