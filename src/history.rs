use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    entries: Vec<String>,
    max_entries: usize,
}

impl History {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    pub fn load(path: &Path, max_entries: usize) -> io::Result<Self> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::new(max_entries));
            }
            Err(err) => return Err(err),
        };

        let mut history = Self::new(max_entries);

        for line in source.lines() {
            history.add(unescape_entry(line));
        }

        Ok(history)
    }

    pub fn add(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        let entry = entry.trim_end_matches('\n').to_string();

        if entry.trim().is_empty() {
            return;
        }

        if self.entries.last() == Some(&entry) {
            return;
        }

        self.entries.push(entry);

        if self.entries.len() > self.max_entries {
            let overflow = self.entries.len() - self.max_entries;
            self.entries.drain(0..overflow);
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn recent(&self, count: usize) -> &[String] {
        let start = self.entries.len().saturating_sub(count);

        &self.entries[start..]
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = String::new();

        for entry in &self.entries {
            output.push_str(&escape_entry(entry));
            output.push('\n');
        }

        fs::write(path, output)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(1000)
    }
}

pub fn default_history_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from) {
        return Some(path.join("crbsh").join("history"));
    }

    std::env::var_os("HOME").map(PathBuf::from).map(|home| {
        home.join(".local")
            .join("state")
            .join("crbsh")
            .join("history")
    })
}

fn escape_entry(entry: &str) -> String {
    let mut escaped = String::new();

    for ch in entry.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            ch => escaped.push(ch),
        }
    }

    escaped
}

fn unescape_entry(entry: &str) -> String {
    let mut unescaped = String::new();
    let mut chars = entry.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            unescaped.push(ch);
            continue;
        }

        match chars.next() {
            Some('\\') => unescaped.push('\\'),
            Some('n') => unescaped.push('\n'),
            Some('r') => unescaped.push('\r'),
            Some(ch) => {
                unescaped.push('\\');
                unescaped.push(ch);
            }
            None => unescaped.push('\\'),
        }
    }

    unescaped
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn suppresses_consecutive_duplicates_only() {
        let mut history = History::new(10);

        history.add("cargo test");
        history.add("cargo test");
        history.add("git status");
        history.add("cargo test");

        assert_eq!(
            history.entries(),
            &[
                "cargo test".to_string(),
                "git status".to_string(),
                "cargo test".to_string()
            ]
        );
    }

    #[test]
    fn keeps_multiline_entry_as_one_logical_item() {
        let path = temp_history_path("keeps_multiline_entry_as_one_logical_item");
        let mut history = History::new(10);

        history.add("while retries < 3 {\n    print retries\n}");
        history.save(&path).unwrap();

        let loaded = History::load(&path, 10).unwrap();

        fs::remove_file(path).unwrap();

        assert_eq!(loaded.entries().len(), 1);
        assert_eq!(
            loaded.entries()[0],
            "while retries < 3 {\n    print retries\n}"
        );
    }

    #[test]
    fn returns_recent_entries() {
        let mut history = History::new(10);

        history.add("one");
        history.add("two");
        history.add("three");

        assert_eq!(history.recent(2), &["two".to_string(), "three".to_string()]);
    }

    fn temp_history_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("crbsh-{name}-{unique}.history"))
    }
}
