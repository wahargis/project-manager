//! File archive module for literature PDFs and attachments.
//!
//! Manages files in ~/.local/share/pm/files/ with sanitized filenames.

pub fn archive_file(lit_id: i64, title: &str, source_path: &str) -> Result<String, std::io::Error> {
    let dir = dirs_data_dir().join("pm/files");
    std::fs::create_dir_all(&dir)?;
    let sanitized: String = title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(50)
        .collect();
    let dest = dir.join(format!("{}_{}.pdf", lit_id, sanitized));
    std::fs::copy(source_path, &dest)?;
    Ok(dest.to_string_lossy().to_string())
}

/// Get the data directory (platform-aware fallback).
fn dirs_data_dir() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".local/share")
    } else {
        std::path::PathBuf::from(".local/share")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title() {
        let sanitized: String = "Hello World! (2024) — A Paper"
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .take(50)
            .collect();
        assert_eq!(sanitized, "HelloWorld2024APaper");
    }

    #[test]
    fn test_archive_file_nonexistent_source() {
        let result = archive_file(1, "Test Paper", "/tmp/nonexistent_file_12345.pdf");
        assert!(result.is_err());
    }
}
