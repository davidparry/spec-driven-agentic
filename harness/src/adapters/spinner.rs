//! An animated "working" indicator for interactive terminals: the
//! trailing dots grow and reset (`.`, `..`, `...`) on a background
//! thread until the guard drops. Off a terminal the line prints once
//! and nothing animates - piped output and CI logs stay clean.
//!
//! Every animation registers itself here, because they all redraw on
//! the one terminal the process owns. [`hush`] silences the registered
//! animations and wipes the frame off the line, so whoever asked for
//! the terminal has it to themselves; [`HushingPrompter`] wraps the
//! console prompters in that guard. Without it the dots redraw over a
//! question every tick and the command looks hung: `spec implement`
//! printed its `command_run` confirmation and then spent the whole
//! wait scribbling "working ..." over it.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::adapters::console_prompt::{GREEN, RESET, YELLOW};
use crate::ports::{PromptError, Prompter, Working};

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

/// One running indicator: the state the animation thread shares with
/// anyone who needs the terminal quiet.
struct Animation {
    /// Painted for the terminal; `columns` is what it occupies on it.
    message: String,
    columns: usize,
    stop: AtomicBool,
    /// Nonzero while something else owns the terminal.
    hushed: AtomicUsize,
    /// Whether a frame is on screen and would need wiping.
    drawn: AtomicBool,
    /// Held for exactly one write, so wiping a frame can never land in
    /// the middle of drawing one.
    out: Mutex<Box<dyn Write + Send>>,
}

impl Animation {
    fn new(message: &str, out: Box<dyn Write + Send>) -> Self {
        Self {
            message: paint(message),
            // One space, then the dots padded to three columns.
            columns: message.chars().count() + 4,
            stop: AtomicBool::new(false),
            hushed: AtomicUsize::new(0),
            drawn: AtomicBool::new(false),
            out: Mutex::new(out),
        }
    }

    /// A poisoned writer is still a writer: a panic elsewhere must not
    /// turn the indicator into a second panic.
    fn writer(&self) -> MutexGuard<'_, Box<dyn Write + Send>> {
        self.out.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn draw(&self, step: usize) {
        let mut out = self.writer();
        if self.hushed.load(Ordering::Acquire) > 0 {
            return;
        }
        // Left-padding to three columns erases the previous, longer frame.
        let _ = write!(out, "\r{} {YELLOW}{:<3}{RESET}", self.message, frame(step));
        let _ = out.flush();
        self.drawn.store(true, Ordering::Release);
    }

    /// Settle the line as `message ...` and move to the next line -
    /// after the animation the log reads exactly like the static form.
    fn settle(&self) {
        let mut out = self.writer();
        let _ = writeln!(out, "\r{} {YELLOW}...{RESET}", self.message);
        let _ = out.flush();
        self.drawn.store(false, Ordering::Release);
    }

    /// Stop drawing and wipe the frame off the line, waiting out a
    /// frame already being written.
    fn hush(&self) {
        self.hushed.fetch_add(1, Ordering::AcqRel);
        let mut out = self.writer();
        if self.drawn.swap(false, Ordering::AcqRel) {
            let _ = write!(out, "\r{:columns$}\r", "", columns = self.columns);
            let _ = out.flush();
        }
    }

    fn resume(&self) {
        self.hushed.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Every animation still running. They all draw on the one terminal
/// the process owns, so silencing "the spinner" has to mean all of
/// them: `spec implement` holds one in `main.rs` across the whole
/// model call while the tool loop several layers down starts others.
static RUNNING: LazyLock<Mutex<Vec<Weak<Animation>>>> = LazyLock::new(Mutex::default);

/// The live animations, with the finished ones swept out on the way.
fn registered() -> Vec<Arc<Animation>> {
    let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    running.retain(|animation| animation.strong_count() > 0);
    running.iter().filter_map(Weak::upgrade).collect()
}

fn register(animation: &Arc<Animation>) {
    let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    running.retain(|animation| animation.strong_count() > 0);
    running.push(Arc::downgrade(animation));
}

/// Silence every running animation until the guard drops, wiping the
/// frame off the line first. Whatever is written next starts on a
/// clean line and stays on screen.
pub fn hush() -> Hush {
    let animations = registered();
    for animation in &animations {
        animation.hush();
    }
    Hush(animations)
}

/// Lets the dots start again. Dropped on every path out, including a
/// panic or an aborted read with the question still on screen.
pub struct Hush(Vec<Arc<Animation>>);

impl Drop for Hush {
    fn drop(&mut self) {
        for animation in &self.0 {
            animation.resume();
        }
    }
}

/// A [`Prompter`] that hands the terminal to whatever it is about to
/// say. Narration and questions alike silence the running animations
/// (see [`hush`]) and let them start again afterwards, so a line is
/// never scribbled over and a question is still on screen when the
/// answer is typed.
///
/// Wrapping at the composition root rather than inside one prompter is
/// the point: the spinner `spec implement` holds knows nothing about
/// the confirmation asked several layers below it, and a prompter
/// written later inherits the guard without knowing the spinner
/// exists.
pub struct HushingPrompter<P: Prompter>(P);

impl<P: Prompter> HushingPrompter<P> {
    pub fn new(inner: P) -> Self {
        Self(inner)
    }
}

impl<P: Prompter> Prompter for HushingPrompter<P> {
    fn tell(&mut self, message: &str) {
        let _hush = hush();
        self.0.tell(message);
    }

    fn warn(&mut self, message: &str) {
        let _hush = hush();
        self.0.warn(message);
    }

    /// Not hushed: this *starts* an animation rather than competing
    /// with one, and the guard it returns outlives the call.
    fn working(&mut self, message: &str) -> Box<dyn Working> {
        self.0.working(message)
    }

    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        let _hush = hush();
        self.0.ask(question)
    }

    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        let _hush = hush();
        self.0.confirm(question)
    }
}

/// Redraw with growing dots every `tick` until the animation stops,
/// then settle the line.
fn animate(animation: &Animation, tick: Duration) {
    let mut step = 0;
    while !animation.stop.load(Ordering::Relaxed) {
        animation.draw(step);
        step += 1;
        std::thread::sleep(tick);
    }
    animation.settle();
}

/// The animated [`Working`] guard: dropping it stops the dots and
/// settles the line.
pub struct Spinner {
    animation: Arc<Animation>,
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
        out: W,
        tick: Duration,
    ) -> Self {
        let animation = Arc::new(Animation::new(message, Box::new(out)));
        if !animated {
            let _ = writeln!(animation.writer(), "{message} ...");
            animation.stop.store(true, Ordering::Relaxed);
            return Self {
                animation,
                handle: None,
            };
        }
        // Only an animation can be scribbled over, so only an animation
        // is worth registering.
        register(&animation);
        let watched = Arc::clone(&animation);
        let handle = std::thread::spawn(move || animate(&watched, tick));
        Self {
            animation,
            handle: Some(handle),
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.animation.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Working for Spinner {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::console_prompt::ConsolePrompter;

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
        let sink = Sink::default();
        let animation = Animation::new("Working", Box::new(sink.clone()));
        animation.stop.store(true, Ordering::Relaxed);
        animate(&animation, Duration::from_millis(1));
        assert_eq!(sink.text(), format!("\rWorking {YELLOW}...{RESET}\n"));
    }

    #[test]
    fn the_animation_redraws_yellow_dots_in_place_until_stopped() {
        let sink = Sink::default();
        let animation = Arc::new(Animation::new("Working", Box::new(sink.clone())));
        let stopper = Arc::clone(&animation);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            stopper.stop.store(true, Ordering::Relaxed);
        });
        animate(&animation, Duration::from_millis(2));
        let text = sink.text();
        let first = format!("\rWorking {YELLOW}.  {RESET}");
        let second = format!("\rWorking {YELLOW}.. {RESET}");
        let settled = format!("\rWorking {YELLOW}...{RESET}\n");
        assert!(text.starts_with(&first), "first frame: {text:?}");
        assert!(text.contains(&second), "second frame: {text:?}");
        assert!(text.ends_with(&settled), "settled line: {text:?}");
    }

    #[test]
    fn off_a_terminal_the_line_prints_once_and_no_thread_spawns() {
        let sink = Sink::default();
        let spinner = Spinner::animating("Working", false, sink.clone(), Duration::from_millis(1));
        assert!(spinner.handle.is_none());
        assert!(spinner.animation.stop.load(Ordering::Relaxed));
        assert_eq!(sink.text(), "Working ...\n");
    }

    #[test]
    fn a_line_that_never_animated_is_not_registered_and_needs_no_wiping() {
        let sink = Sink::default();
        let _spinner = Spinner::animating("Working", false, sink.clone(), Duration::from_millis(1));
        drop(hush());
        // A piped run has nothing on the line to wipe, so the guard is
        // inert and the log stays exactly as clean as before.
        assert_eq!(sink.text(), "Working ...\n");
    }

    #[test]
    fn on_a_terminal_the_animation_runs_on_a_thread_until_the_guard_drops() {
        let sink = Sink::default();
        let spinner = Spinner::animating("Working", true, sink.clone(), Duration::from_millis(1));
        assert!(spinner.handle.is_some());
        // Wait for the first frame so the drop never races the thread's
        // first stop check.
        sink.wait_for("Working");
        drop(spinner); // flips stop and joins the animation thread
        let text = sink.text();
        assert!(
            text.starts_with(&format!("\rWorking {YELLOW}.  {RESET}")),
            "first frame: {text:?}"
        );
        assert!(
            text.ends_with(&format!("\rWorking {YELLOW}...{RESET}\n")),
            "settled line: {text:?}"
        );
    }

    /// The defect this guard exists for, asserted at the seam: the
    /// ordered writes that reached the terminal. `spec implement`
    /// starts a spinner in `main.rs` and, several layers down, asks the
    /// developer to confirm `command_run`. The question has to be the
    /// last thing written before the answer is read - a frame in
    /// between is a question nobody can see, and the command looks
    /// hung.
    #[test]
    fn no_frame_is_drawn_between_asking_and_reading_the_answer() {
        let sink = Sink::default();
        let spinner = Spinner::animating(
            "Sending the sources to the model - working",
            true,
            sink.clone(),
            Duration::from_millis(1),
        );
        sink.wait_for("- working");
        let mut prompter = HushingPrompter::new(ConsolePrompter::new(
            std::io::BufReader::new(SlowReader::new("y\n", sink.clone())),
            sink.clone(),
        ));

        assert!(
            prompter
                .confirm(r#"Run command_run(command=["cat","kata/pom.xml"])?"#)
                .unwrap()
        );
        drop(prompter);
        drop(spinner);

        let writes = sink.writes();
        let asked = writes
            .iter()
            .position(|write| write.contains("[y/N]"))
            .unwrap_or_else(|| panic!("the question was never written: {writes:?}"));
        let read = writes
            .iter()
            .position(|write| write == ANSWER_READ)
            .unwrap_or_else(|| panic!("the answer was never read: {writes:?}"));
        assert!(asked < read, "asked after reading: {writes:?}");
        let waiting = &writes[asked..read];
        assert!(
            !waiting.iter().any(|write| write.contains("working")),
            "the dots drew over the question: {waiting:?}"
        );
    }

    /// The other half of the guard: it wipes the frame before the
    /// question so the question does not land on top of the dots, and
    /// the dots come back once the answer is in.
    #[test]
    fn the_line_is_wiped_before_the_question_and_the_dots_return_after_it() {
        let sink = Sink::default();
        let spinner = Spinner::animating(
            "Sending the sources to the model - working",
            true,
            sink.clone(),
            Duration::from_millis(1),
        );
        sink.wait_for("- working");
        let mut prompter = HushingPrompter::new(ConsolePrompter::new(
            std::io::BufReader::new(SlowReader::new("y\n", sink.clone())),
            sink.clone(),
        ));
        prompter.confirm("Stage this?").unwrap();
        let writes = sink.writes();
        let asked = writes
            .iter()
            .position(|write| write.contains("[y/N]"))
            .expect("the question was written");
        let wipe = &writes[asked - 1];
        assert!(
            wipe.starts_with('\r')
                && wipe.ends_with('\r')
                && wipe.trim_matches('\r').chars().all(|c| c == ' '),
            "the frame was still on the line: {writes:?}"
        );

        // Resumed: the dots pick the line back up now the answer is in.
        sink.wait_for_another("- working");
        drop(prompter);
        drop(spinner);
    }

    /// A question asked with nothing animating must not gain a stray
    /// wipe - `spec draft`'s wizard is the same prompter without a
    /// spinner, and its transcript is asserted elsewhere byte for byte.
    #[test]
    fn a_question_with_no_animation_running_writes_nothing_extra() {
        let sink = Sink::default();
        let mut prompter = HushingPrompter::new(ConsolePrompter::new(
            std::io::Cursor::new(b"a fine title\n".to_vec()),
            sink.clone(),
        ));
        assert_eq!(prompter.ask("Title?").unwrap(), "a fine title");
        prompter.tell("staged");
        assert_eq!(sink.text(), "Title? staged\n");
    }

    /// Records every write in order, so a test can ask what reached the
    /// terminal and in which order.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<String>>>);

    impl Sink {
        fn writes(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }

        fn text(&self) -> String {
            self.writes().concat()
        }

        fn record(&self, text: &str) {
            self.0.lock().unwrap().push(text.to_string());
        }

        fn wait_for(&self, needle: &str) {
            self.wait_until(|sink| sink.text().contains(needle), needle);
        }

        /// Wait for `needle` to be written again after everything
        /// already recorded.
        fn wait_for_another(&self, needle: &str) {
            let seen = self.writes().len();
            self.wait_until(
                |sink| sink.writes()[seen..].iter().any(|w| w.contains(needle)),
                needle,
            );
        }

        fn wait_until(&self, done: impl Fn(&Self) -> bool, needle: &str) {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done(self) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "{needle:?} never arrived: {:?}",
                    self.writes()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.record(&String::from_utf8_lossy(buf));
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    const ANSWER_READ: &str = "<the answer was read>";

    /// A reader that takes its time, the way a human reading a
    /// confirmation does. Anything the animation manages to write
    /// while it waits lands between the question and the marker.
    struct SlowReader {
        answer: std::io::Cursor<Vec<u8>>,
        sink: Sink,
        waited: bool,
    }

    impl SlowReader {
        fn new(answer: &str, sink: Sink) -> Self {
            Self {
                answer: std::io::Cursor::new(answer.as_bytes().to_vec()),
                sink,
                waited: false,
            }
        }
    }

    impl std::io::Read for SlowReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.waited {
                self.waited = true;
                std::thread::sleep(Duration::from_millis(50));
                self.sink.record(ANSWER_READ);
            }
            self.answer.read(buf)
        }
    }
}
