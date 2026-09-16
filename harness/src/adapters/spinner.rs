//! An animated "working" indicator for interactive terminals: the
//! trailing dots grow and reset (`.`, `..`, `...`) on a background
//! thread until the guard drops. Off a terminal the line prints once
//! and nothing animates - piped output and CI logs stay clean.

use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::adapters::console_prompt::{GREEN, RESET, YELLOW};
use crate::ports::Working;

/// The greenfield loop's attempt announcement - the moment the model
/// starts writing code - renders dark green on a terminal.
const ATTEMPT_NOTE: &str = "Generating an implementation attempt";

fn paint(message: &str) -> String {
    message.replace(ATTEMPT_NOTE, &format!("{GREEN}{ATTEMPT_NOTE}{RESET}"))
}

/// The dot frame for an animation step: one, two, three, over again.
fn frame(step: usize) -> &'static str {
    match step % 3 {
        0 => ".",
        1 => "..",
        _ => "...",
    }
}

/// Redraw `message` with growing dots every `tick` until `stop` flips,
/// then settle the line as `message ...` and move to the next line -
/// after the animation the log reads exactly like the static form.
fn animate(message: &str, stop: &AtomicBool, out: &mut impl Write, tick: Duration) {
    let mut step = 0;
    while !stop.load(Ordering::Relaxed) {
        // Left-padding to three columns erases the previous, longer frame.
        let _ = write!(out, "\r{message} {YELLOW}{:<3}{RESET}", frame(step));
        let _ = out.flush();
        step += 1;
        std::thread::sleep(tick);
    }
    let _ = writeln!(out, "\r{message} {YELLOW}...{RESET}");
    let _ = out.flush();
}

/// The animated [`Working`] guard: dropping it stops the dots and
/// settles the line.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// Start the indicator. On a terminal the dots animate on a
    /// background thread; anywhere else the line prints once, exactly
    /// like the default prompter behavior.
    pub fn start(message: &str) -> Self {
        Self::with_animation(message, std::io::stdout().is_terminal())
    }

    fn with_animation(message: &str, animated: bool) -> Self {
        Self::animating(
            message,
            animated,
            std::io::stdout(),
            Duration::from_millis(250),
        )
    }

    /// The animation decision, the writer, and the tick are injected so
    /// tests exercise both paths deterministically on a buffer -
    /// `cargo test` on a real terminal still sees a terminal on the
    /// process's stdout, capture notwithstanding.
    fn animating<W: Write + Send + 'static>(
        message: &str,
        animated: bool,
        mut out: W,
        tick: Duration,
    ) -> Self {
        if !animated {
            let _ = writeln!(out, "{message} ...");
            return Self {
                stop: Arc::new(AtomicBool::new(true)),
                handle: None,
            };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let watch = Arc::clone(&stop);
        let message = paint(message);
        let handle = std::thread::spawn(move || {
            animate(&message, &watch, &mut out, tick);
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Working for Spinner {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attempt_announcement_is_painted_dark_green() {
        assert_eq!(
            paint("Generating an implementation attempt - working"),
            format!("{GREEN}Generating an implementation attempt{RESET} - working")
        );
        assert_eq!(
            paint("Running the tests - working"),
            "Running the tests - working"
        );
    }

    #[test]
    fn the_dots_grow_and_start_over() {
        assert_eq!(frame(0), ".");
        assert_eq!(frame(1), "..");
        assert_eq!(frame(2), "...");
        assert_eq!(frame(3), ".");
    }

    #[test]
    fn a_stopped_animation_settles_the_line_and_nothing_more() {
        let stop = AtomicBool::new(true);
        let mut out = Vec::new();
        animate("Working", &stop, &mut out, Duration::from_millis(1));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("\rWorking {YELLOW}...{RESET}\n")
        );
    }

    #[test]
    fn the_animation_redraws_yellow_dots_in_place_until_stopped() {
        let stop = Arc::new(AtomicBool::new(false));
        let flip = Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            flip.store(true, Ordering::Relaxed);
        });
        let mut out = Vec::new();
        animate("Working", &stop, &mut out, Duration::from_millis(2));
        let text = String::from_utf8(out).unwrap();
        let first = format!("\rWorking {YELLOW}.  {RESET}");
        let second = format!("\rWorking {YELLOW}.. {RESET}");
        let settled = format!("\rWorking {YELLOW}...{RESET}\n");
        assert!(text.starts_with(&first), "first frame: {text:?}");
        assert!(text.contains(&second), "second frame: {text:?}");
        assert!(text.ends_with(&settled), "settled line: {text:?}");
    }

    #[test]
    fn off_a_terminal_the_line_prints_once_and_no_thread_spawns() {
        let buf = SharedBuf::default();
        let spinner = Spinner::animating("Working", false, buf.clone(), Duration::from_millis(1));
        assert!(spinner.handle.is_none());
        assert!(spinner.stop.load(Ordering::Relaxed));
        assert_eq!(buf.text(), "Working ...\n");
    }

    #[test]
    fn on_a_terminal_the_animation_runs_on_a_thread_until_the_guard_drops() {
        let buf = SharedBuf::default();
        let spinner = Spinner::animating("Working", true, buf.clone(), Duration::from_millis(1));
        assert!(spinner.handle.is_some());
        // Wait for the first frame so the drop never races the thread's
        // first stop check.
        while buf.text().is_empty() {
            std::thread::yield_now();
        }
        drop(spinner); // flips stop and joins the animation thread
        let text = buf.text();
        assert!(
            text.starts_with(&format!("\rWorking {YELLOW}.  {RESET}")),
            "first frame: {text:?}"
        );
        assert!(
            text.ends_with(&format!("\rWorking {YELLOW}...{RESET}\n")),
            "settled line: {text:?}"
        );
    }

    /// A [`Write`] the animation thread and the test share.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<std::sync::Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
