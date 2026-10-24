//! Log destinations: stderr plus an optional append-only file.
//!
//! The binary is often started from a task runner or a shortcut whose console
//! disappears together with the process, so a stderr-only diagnosis leaves
//! nothing behind. `--log-file` tees every event into a file, and the panic
//! hook writes there too, so the reason for an exit survives.
//!
//! ```text
//! remaprs --config remaprs.yaml --log-file remaprs.log
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Where log lines go: always stderr, and optionally a file.
///
/// Cloning shares the same file handle, so every clone appends to one file.
#[derive(Debug, Clone)]
pub struct LogSink {
    /// Path being appended to, kept for the startup message.
    path: Option<PathBuf>,
    /// Empty for a stderr-only sink.
    ///
    /// `OnceLock` rather than a lock: the file is opened once at startup and
    /// then only written to, and `&File` implements `Write`, so no exclusive
    /// access is ever needed.
    file: Arc<OnceLock<File>>,
}

impl LogSink {
    /// Writes to stderr only.
    ///
    /// This is the default, and it is what a build without `--log-file` uses.
    #[must_use]
    pub fn stderr() -> Self {
        Self {
            path: None,
            file: Arc::new(OnceLock::new()),
        }
    }

    /// Writes to stderr and appends to `path`, creating the file if needed.
    ///
    /// Appending rather than truncating keeps the history of previous runs,
    /// which is exactly what is needed after a silent exit.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be opened for appending.
    pub fn to_file(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let cell = OnceLock::new();
        // The cell is created here and never populated again, so this cannot
        // fail; an already-set cell would simply keep the first handle.
        let _ = cell.set(file);

        Ok(Self {
            path: Some(path),
            file: Arc::new(cell),
        })
    }

    /// The file being appended to, when one is configured.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Appends one line to the file, if any.
    ///
    /// Used for messages that were already printed to stderr by the caller,
    /// such as the final `error:` line.
    pub fn append_line(&self, line: &str) {
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        self.write_to_file(text.as_bytes());
    }

    fn write_to_file(&self, bytes: &[u8]) {
        let Some(file) = self.file.get() else {
            return;
        };
        // `&File` is `Write`, so the shared handle is enough.
        let mut handle = file;
        // A log file that cannot be written must not crash the program that is
        // being diagnosed.
        let _ = handle.write_all(bytes);
        let _ = handle.flush();
    }
}

impl Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // stderr first: when it is a closed console the write fails, and the
        // file must still receive the line.
        let _ = io::stderr().write_all(buf);
        self.write_to_file(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Some(file) = self.file.get() {
            let mut handle = file;
            let _ = handle.flush();
        }
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogSink {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Installs the tracing subscriber for `level`.
///
/// `RUST_LOG` overrides `level`, and an unparseable value falls back to `info`
/// rather than failing to start.
pub fn init(level: &str, sink: &LogSink) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(sink.clone())
        .try_init();
}

/// Records panics in the log file before the default hook prints them.
///
/// A panic in a release build otherwise vanishes with the console that started
/// the process.
pub fn install_panic_hook(sink: &LogSink) {
    let sink = sink.clone();
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        sink.append_line(&format!("panic: {}", describe_panic(info)));
        previous(info);
    }));
}

/// Renders a panic payload and location as one line.
fn describe_panic(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_owned());

    match info.location() {
        Some(location) => format!("at {location}: {message}"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::LogSink;
    use std::io::Write;

    /// A unique path in the temp directory, since these tests run in parallel.
    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = format!("remaprs-{}-{name}.log", std::process::id());
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn a_file_sink_appends_instead_of_truncating() {
        let path = temp_path("append");
        std::fs::write(&path, "first run\n").expect("the temp file is writable");

        let mut sink = LogSink::to_file(&path).expect("the file opens for append");
        sink.write_all(b"second run\n").expect("the write succeeds");

        let text = std::fs::read_to_string(&path).expect("the file is readable");
        assert!(text.contains("first run"), "the old run was kept: {text}");
        assert!(text.contains("second run"), "the new run was written");
        assert_eq!(sink.path(), Some(path.as_path()));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_stderr_sink_has_no_path_and_ignores_file_writes() {
        let sink = LogSink::stderr();
        assert!(sink.path().is_none());

        // Appending without a file must be a no-op rather than a panic.
        sink.append_line("nothing to write to");
    }
}
