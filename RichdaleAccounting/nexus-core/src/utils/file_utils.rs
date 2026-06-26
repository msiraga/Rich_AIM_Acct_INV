//! File Utilities Module
//!
//! Provides utility functions for working with files and directories.

use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::fmt;
use thiserror::Error;
use base64::{Engine as _, engine::general_purpose};
use path_absolutize::Absolutize;

/// File error types
#[derive(Debug, Error)]
pub enum FileError {
    /// File not found
    #[error("File not found: {0}")]
    NotFound(String),
    
    /// File already exists
    #[error("File already exists: {0}")]
    AlreadyExists(String),
    
    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    
    /// UTF-8 error
    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    
    /// Base64 error
    #[error("Base64 error: {0}")]
    Base64Error(String),
    
    /// Any other file error
    #[error("File error: {0}")]
    Other(String),
}

impl FileError {
    /// Create a new not found error
    pub fn not_found<P: AsRef<Path>>(path: P) -> Self {
        Self::NotFound(path.as_ref().display().to_string())
    }

    /// Create a new already exists error
    pub fn already_exists<P: AsRef<Path>>(path: P) -> Self {
        Self::AlreadyExists(path.as_ref().display().to_string())
    }

    /// Create a new permission denied error
    pub fn permission_denied<P: AsRef<Path>>(path: P) -> Self {
        Self::PermissionDenied(path.as_ref().display().to_string())
    }

    /// Create a new invalid path error
    pub fn invalid_path<P: AsRef<Path>>(path: P) -> Self {
        Self::InvalidPath(path.as_ref().display().to_string())
    }

    /// Create a new base64 error
    pub fn base64_error(msg: &str) -> Self {
        Self::Base64Error(msg.to_string())
    }

    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

/// Result type for file operations
pub type FileResult<T> = Result<T, FileError>;

/// File processor for common file operations
#[derive(Debug, Clone)]
pub struct FileProcessor;

impl FileProcessor {
    /// Check if a path exists
    pub fn exists<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().exists()
    }

    /// Check if a path is a file
    pub fn is_file<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().is_file()
    }

    /// Check if a path is a directory
    pub fn is_directory<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().is_dir()
    }

    /// Get the absolute path
    pub fn absolutize<P: AsRef<Path>>(path: P) -> FileResult<PathBuf> {
        path.as_ref()
            .absolutize()
            .map_err(|e| FileError::InvalidPath(e.to_string()))
    }

    /// Get the canonical path
    pub fn canonicalize<P: AsRef<Path>>(path: P) -> FileResult<PathBuf> {
        fs::canonicalize(path.as_ref())
            .map_err(|e| FileError::IoError(e))
    }

    /// Get the parent directory
    pub fn parent<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
        path.as_ref().parent().map(|p| p.to_path_buf())
    }

    /// Get the file name
    pub fn file_name<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref().file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// Get the file stem (name without extension)
    pub fn file_stem<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref().file_stem().map(|n| n.to_string_lossy().into_owned())
    }

    /// Get the file extension
    pub fn extension<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref().extension().map(|e| e.to_string_lossy().into_owned())
    }

    /// Create a directory (and all parent directories)
    pub fn create_dir_all<P: AsRef<Path>>(path: P) -> FileResult<()> {
        fs::create_dir_all(path.as_ref())
            .map_err(|e| FileError::IoError(e))
    }

    /// Create a directory
    pub fn create_dir<P: AsRef<Path>>(path: P) -> FileResult<()> {
        fs::create_dir(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    FileError::already_exists(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Remove a file
    pub fn remove_file<P: AsRef<Path>>(path: P) -> FileResult<()> {
        fs::remove_file(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Remove a directory
    pub fn remove_dir<P: AsRef<Path>>(path: P) -> FileResult<()> {
        fs::remove_dir(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Remove a directory and all its contents
    pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> FileResult<()> {
        fs::remove_dir_all(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Copy a file
    pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> FileResult<u64> {
        fs::copy(from.as_ref(), to.as_ref())
            .map_err(|e| FileError::IoError(e))
    }

    /// Rename a file or directory
    pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> FileResult<()> {
        fs::rename(from.as_ref(), to.as_ref())
            .map_err(|e| FileError::IoError(e))
    }

    /// Read a file to a string
    pub fn read_to_string<P: AsRef<Path>>(path: P) -> FileResult<String> {
        fs::read_to_string(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else if e.kind() == io::ErrorKind::PermissionDenied {
                    FileError::permission_denied(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Read a file to bytes
    pub fn read<P: AsRef<Path>>(path: P) -> FileResult<Vec<u8>> {
        fs::read(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else if e.kind() == io::ErrorKind::PermissionDenied {
                    FileError::permission_denied(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })
    }

    /// Write a string to a file
    pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> FileResult<()> {
        fs::write(path.as_ref(), contents)
            .map_err(|e| FileError::IoError(e))
    }

    /// Append to a file
    pub fn append<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> FileResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())
            .map_err(|e| FileError::IoError(e))?;
        
        file.write_all(contents.as_ref())
            .map_err(|e| FileError::IoError(e))
    }

    /// Read a file line by line
    pub fn read_lines<P: AsRef<Path>>(path: P) -> FileResult<Vec<String>> {
        let contents = Self::read_to_string(path)?;
        Ok(contents.lines().map(|s| s.to_string()).collect())
    }

    /// Get file size in bytes
    pub fn file_size<P: AsRef<Path>>(path: P) -> FileResult<u64> {
        let metadata = fs::metadata(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })?;
        
        Ok(metadata.len())
    }

    /// Get file modified time
    pub fn modified_time<P: AsRef<Path>>(path: P) -> FileResult<chrono::DateTime<chrono::Utc>> {
        let metadata = fs::metadata(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })?;
        
        let modified = metadata.modified()
            .map_err(|e| FileError::IoError(e))?;
        
        Ok(chrono::DateTime::<chrono::Utc>::from(modified))
    }

    /// Get file created time
    pub fn created_time<P: AsRef<Path>>(path: P) -> FileResult<chrono::DateTime<chrono::Utc>> {
        let metadata = fs::metadata(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })?;
        
        let created = metadata.created()
            .map_err(|e| FileError::IoError(e))?;
        
        Ok(chrono::DateTime::<chrono::Utc>::from(created))
    }

    /// List files in a directory
    pub fn list_files<P: AsRef<Path>>(path: P) -> FileResult<Vec<PathBuf>> {
        let entries = fs::read_dir(path.as_ref())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    FileError::not_found(path.as_ref())
                } else if e.kind() == io::ErrorKind::PermissionDenied {
                    FileError::permission_denied(path.as_ref())
                } else {
                    FileError::IoError(e)
                }
            })?;
        
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| FileError::IoError(e))?;
            files.push(entry.path());
        }
        
        Ok(files)
    }

    /// List files in a directory with filtering
    pub fn list_files_with_filter<P: AsRef<Path>, F>(path: P, filter: F) -> FileResult<Vec<PathBuf>>
    where
        F: Fn(&Path) -> bool,
    {
        let files = Self::list_files(path)?;
        Ok(files.into_iter().filter(filter).collect())
    }

    /// List only files (not directories)
    pub fn list_only_files<P: AsRef<Path>>(path: P) -> FileResult<Vec<PathBuf>> {
        Self::list_files_with_filter(path, |p| p.is_file())
    }

    /// List only directories
    pub fn list_only_dirs<P: AsRef<Path>>(path: P) -> FileResult<Vec<PathBuf>> {
        Self::list_files_with_filter(path, |p| p.is_dir())
    }

    /// List files with a specific extension
    pub fn list_files_with_extension<P: AsRef<Path>>(path: P, extension: &str) -> FileResult<Vec<PathBuf>> {
        Self::list_files_with_filter(path, |p| {
            p.extension().map_or(false, |ext| ext.to_string_lossy().eq_ignore_ascii_case(extension))
        })
    }

    /// Encode file contents to base64
    pub fn encode_to_base64<P: AsRef<Path>>(path: P) -> FileResult<String> {
        let contents = Self::read(path)?;
        Ok(general_purpose::STANDARD.encode(contents))
    }

    /// Decode base64 to a file
    pub fn decode_from_base64<P: AsRef<Path>>(base64: &str, path: P) -> FileResult<()> {
        let contents = general_purpose::STANDARD.decode(base64)
            .map_err(|e| FileError::base64_error(&e.to_string()))?;
        
        Self::write(path, contents)
    }

    /// Hash file contents using SHA-256
    pub fn hash_sha256<P: AsRef<Path>>(path: P) -> FileResult<String> {
        use sha2::{Sha256, Digest};
        
        let contents = Self::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&contents);
        let result = hasher.finalize();
        
        Ok(hex::encode(result))
    }

    /// Check if a file has a specific hash
    pub fn verify_hash<P: AsRef<Path>>(path: P, expected_hash: &str) -> FileResult<bool> {
        let actual_hash = Self::hash_sha256(path)?;
        Ok(actual_hash == expected_hash)
    }

    /// Get the MIME type of a file based on its extension
    pub fn get_mime_type<P: AsRef<Path>>(path: P) -> String {
        let ext = path.as_ref().extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());
        
        match ext.as_deref() {
            Some("html") => "text/html".to_string(),
            Some("htm") => "text/html".to_string(),
            Some("txt") => "text/plain".to_string(),
            Some("csv") => "text/csv".to_string(),
            Some("json") => "application/json".to_string(),
            Some("xml") => "application/xml".to_string(),
            Some("pdf") => "application/pdf".to_string(),
            Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
            Some("png") => "image/png".to_string(),
            Some("gif") => "image/gif".to_string(),
            Some("svg") => "image/svg+xml".to_string(),
            Some("ico") => "image/x-icon".to_string(),
            Some("js") => "application/javascript".to_string(),
            Some("css") => "text/css".to_string(),
            Some("woff") => "font/woff".to_string(),
            Some("woff2") => "font/woff2".to_string(),
            Some("ttf") => "font/ttf".to_string(),
            Some("eot") => "application/vnd.ms-fontobject".to_string(),
            Some("mp3") => "audio/mpeg".to_string(),
            Some("wav") => "audio/wav".to_string(),
            Some("mp4") => "video/mp4".to_string(),
            Some("webm") => "video/webm".to_string(),
            Some("zip") => "application/zip".to_string(),
            Some("tar") => "application/x-tar".to_string(),
            Some("gz") => "application/gzip".to_string(),
            Some("7z") => "application/x-7z-compressed".to_string(),
            Some("rar") => "application/x-rar-compressed".to_string(),
            Some("doc") => "application/msword".to_string(),
            Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
            Some("xls") => "application/vnd.ms-excel".to_string(),
            Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            Some("ppt") => "application/vnd.ms-powerpoint".to_string(),
            Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    }

    /// Get a temporary file path
    pub fn temp_file_path(prefix: &str, suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let filename = format!("{}{}{}", prefix, Uuid::new_v4(), suffix);
        path.push(filename);
        path
    }

    /// Create a temporary file
    pub fn create_temp_file(prefix: &str, suffix: &str) -> FileResult<PathBuf> {
        let path = Self::temp_file_path(prefix, suffix);
        File::create(&path)
            .map_err(|e| FileError::IoError(e))?;
        Ok(path)
    }

    /// Create a temporary directory
    pub fn create_temp_dir(prefix: &str) -> FileResult<PathBuf> {
        let mut path = std::env::temp_dir();
        let dirname = format!("{}{}", prefix, Uuid::new_v4());
        path.push(dirname);
        
        Self::create_dir(&path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_exists() {
        // Create a temporary file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();
        
        assert!(FileProcessor::exists(path));
        assert!(FileProcessor::is_file(path));
        assert!(!FileProcessor::is_directory(path));
    }

    #[test]
    fn test_create_and_remove_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test_dir");
        
        // Create directory
        FileProcessor::create_dir(&path).unwrap();
        assert!(FileProcessor::is_directory(&path));
        
        // Remove directory
        FileProcessor::remove_dir(&path).unwrap();
        assert!(!FileProcessor::exists(&path));
    }

    #[test]
    fn test_create_dir_all() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("a").join("b").join("c");
        
        FileProcessor::create_dir_all(&path).unwrap();
        assert!(FileProcessor::is_directory(&path));
    }

    #[test]
    fn test_write_and_read() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        
        // Write to file
        FileProcessor::write(&path, "Hello, World!").unwrap();
        
        // Read from file
        let contents = FileProcessor::read_to_string(&path).unwrap();
        assert_eq!(contents, "Hello, World!");
        
        // Clean up
        FileProcessor::remove_file(&path).unwrap();
    }

    #[test]
    fn test_file_size() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        
        FileProcessor::write(&path, "Hello").unwrap();
        let size = FileProcessor::file_size(&path).unwrap();
        assert_eq!(size, 5);
        
        FileProcessor::remove_file(&path).unwrap();
    }

    #[test]
    fn test_list_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();
        
        // Create some files
        FileProcessor::write(path.join("file1.txt"), "").unwrap();
        FileProcessor::write(path.join("file2.txt"), "").unwrap();
        FileProcessor::create_dir(path.join("subdir")).unwrap();
        
        let files = FileProcessor::list_files(path).unwrap();
        assert_eq!(files.len(), 3); // file1.txt, file2.txt, subdir
        
        let only_files = FileProcessor::list_only_files(path).unwrap();
        assert_eq!(only_files.len(), 2);
        
        let only_dirs = FileProcessor::list_only_dirs(path).unwrap();
        assert_eq!(only_dirs.len(), 1);
    }

    #[test]
    fn test_base64_encode_decode() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        
        // Write test data
        FileProcessor::write(&path, "Hello, Base64!").unwrap();
        
        // Encode to base64
        let encoded = FileProcessor::encode_to_base64(&path).unwrap();
        assert!(!encoded.is_empty());
        
        // Decode from base64
        let decoded_path = temp_dir.path().join("decoded.txt");
        FileProcessor::decode_from_base64(&encoded, &decoded_path).unwrap();
        
        // Verify
        let contents = FileProcessor::read_to_string(&decoded_path).unwrap();
        assert_eq!(contents, "Hello, Base64!");
    }

    #[test]
    fn test_hash_sha256() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        
        FileProcessor::write(&path, "Hello, SHA-256!").unwrap();
        
        let hash = FileProcessor::hash_sha256(&path).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex characters
    }

    #[test]
    fn test_mime_type() {
        assert_eq!(FileProcessor::get_mime_type("test.html"), "text/html");
        assert_eq!(FileProcessor::get_mime_type("test.txt"), "text/plain");
        assert_eq!(FileProcessor::get_mime_type("test.json"), "application/json");
        assert_eq!(FileProcessor::get_mime_type("test.png"), "image/png");
        assert_eq!(FileProcessor::get_mime_type("test.pdf"), "application/pdf");
        assert_eq!(FileProcessor::get_mime_type("test.unknown"), "application/octet-stream");
    }

    #[test]
    fn test_temp_file() {
        let path = FileProcessor::temp_file_path("nexus_", ".tmp");
        assert!(path.to_string_lossy().contains("nexus_"));
        assert!(path.to_string_lossy().contains(".tmp"));
        
        // Clean up (in case the file was created)
        let _ = FileProcessor::remove_file(&path);
    }
}
