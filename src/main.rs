mod stdio;
mod term;
use colored::*;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use env_logger::{Builder, Target};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle, WeakProgressBar};
use log::{Level, LevelFilter, Log, Metadata, Record};

const RANDOM_VERBS: &[&str] = &[
    "building",
    "linking",
    "compiling",
    "downloading",
    "uploading",
    "generating",
    "optimizing",
    "minifying",
    "preparing",
    "checking",
    "linting",
    "formatting",
    "analyzing",
    "checking",
];

const RANDOM_OBJECT: &[&str] = &[
    "project",
    "crate",
    "package",
    "library",
    "binary",
    "artifact",
    "dependency",
    "target",
    "build",
    "file",
    "module",
    "function",
    "test",
    "example",
    "bench",
];

struct LogGenerator {
    line_idx: usize,
    min_lines: usize,
    max_lines: usize,
}

impl LogGenerator {
    fn generate(&mut self) -> String {
        let num_lines = fastrand::usize(self.min_lines..self.max_lines);
        let mut output = String::new();
        for _ in 0..num_lines {
            let line = self.generate_line(self.line_idx);
            output.push_str(&line);
            output.push('\n');
            self.line_idx += 1;
        }

        output
    }

    fn generate_line(&mut self, line_idx: usize) -> String {
        let verb = RANDOM_VERBS[fastrand::usize(0..RANDOM_VERBS.len())];
        let object = RANDOM_OBJECT[fastrand::usize(0..RANDOM_OBJECT.len())];
        let line = format!("{:<5}: {} the {}", line_idx, verb, object);
        line
    }
}

fn logthread(
    thread_: usize,
    flag: Arc<AtomicBool>,
    sender: std::sync::mpsc::Sender<(usize, String)>,
) {
    let mut lg = LogGenerator {
        line_idx: 0,
        min_lines: 0,
        max_lines: 10,
    };

    while flag.load(std::sync::atomic::Ordering::SeqCst) {
        let line = lg.generate();
        sender.send((thread_, line)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

struct MyLogger;

impl Log for MyLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let destination = stdio::get_destination();

        // Build the message string.
        use std::fmt::Write as FmtWrite;
        let log_msg = format!("{}", record.args());
        let log_string = {
            let mut log_string = String::new();

            let use_color = destination.stderr_use_color();

            let level = record.level();
            let level_marker = match level {
                _ if !use_color => format!("[{level}]").normal().clear(),
                Level::Info => format!("[{level}]").normal(),
                Level::Error => format!("[{level}]").red(),
                Level::Warn => format!("[{level}]").yellow(),
                Level::Debug => format!("[{level}]").green(),
                Level::Trace => format!("[{level}]").magenta(),
            };
            write!(log_string, " {level_marker}").unwrap();

            write!(log_string, " ({})", record.target()).unwrap();

            writeln!(log_string, " {log_msg}").unwrap();
            log_string
        };
        let log_bytes = log_string.as_bytes();

        // Attempt to write to stdio, and write to the pantsd log if we fail (either because we don't
        // have a valid stdio instance, or because of an error).
        destination.write_stderr_raw(log_bytes).unwrap();
    }

    fn flush(&self) {}
}
fn main() {
    const NUM_THREADS: usize = 5;
    const DURATION: std::time::Duration = std::time::Duration::from_secs(20);

    log::set_boxed_logger(Box::new(MyLogger)).unwrap();
    let terminal_width = 50;
    let destination = stdio::new_console_destination(0, 1, 2);
    stdio::set_thread_destination(destination);
    let stderr_dest_bar: Arc<Mutex<Option<WeakProgressBar>>> = Arc::new(Mutex::new(None));
    let mut stderr_dest_bar_guard = stderr_dest_bar.lock().unwrap();
    let (term_read, _, term_stderr_write) = {
        let stderr_dest_bar = stderr_dest_bar.clone();
        stdio::get_destination()
            .exclusive_start(Box::new(move |msg: &str| {
                let dest_bar = {
                    let stderr_dest_bar = stderr_dest_bar.lock().unwrap();

                    stderr_dest_bar.as_ref().unwrap().upgrade().ok_or(())?
                };
                dest_bar.println(msg);
                Ok(())
            }))
            .unwrap()
    };
    let flag = Arc::new(AtomicBool::new(true));
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut threads = (0..NUM_THREADS)
        .map(|thread_| {
            let flag = flag.clone();

            let sender = sender.clone();

            let thread = std::thread::spawn(move || {
                logthread(thread_, flag, sender);
            });
            thread
        })
        .collect::<Vec<_>>();

    let term = console::Term::read_write_pair_with_style(
        term_read,
        term_stderr_write,
        console::Style::new().force_styling(true),
    );

    let draw_target = ProgressDrawTarget::term(term, 30);
    let multi_progress = MultiProgress::with_draw_target(draw_target);

    let bars = (0..5)
        .map(|_n| {
            let style = ProgressStyle::default_spinner()
                .template("{spinner} {prefix:.bold.dim}{msg}")
                .expect("Valid template.");

            let pb = multi_progress.add(ProgressBar::new(terminal_width).with_style(style));
            pb.set_prefix(format!("Thread {_n}"));
            pb
        })
        .collect::<Vec<_>>();

    *stderr_dest_bar_guard = Some(bars[0].downgrade());

    let flag_ = flag.clone();
    threads.push(std::thread::spawn(move || {
        while flag_.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(fastrand::u64(100..1000)));
            log::warn!("This is a test log message");
        }
    }));

    let weak = bars[0].downgrade();
    let flag_ = flag.clone();
    let started = std::time::Instant::now();
    threads.push(std::thread::spawn(move || {
		let mut count = 0;
		let mut print = 0;
        let mut thread_states = vec![String::new(); NUM_THREADS];

        while flag_.load(std::sync::atomic::Ordering::SeqCst) {
            match receiver.try_recv() {
                Ok((thread, line)) => {
                    thread_states[thread] = line;
					count += 1;

					if count % 100 == 0 {
						print += 1;
						let pb = weak.upgrade().unwrap();
						multi_progress.println(format!("{print:} Hello, world!\nThis is a multiline message.\nIt's pretty cool.\nThis is the {print:}th time we've printed this message.\nWe have received {count:} messages so far.\n\nThis is the final line.\n"));
					}

                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    for (thread, line) in thread_states.iter().enumerate() {
                        let pb = &bars[thread];

                        if line.is_empty() {
                            pb.set_prefix(format!(
                                "Thread {} ({}s)",
                                thread,
                                started.elapsed().as_secs()
                            ));
                            pb.set_message("");
                        } else {
                            pb.set_message(format!("\n{line}"));
                        }
                        pb.tick();
                    }
                    continue;
                }
            }
        }
    }));

    std::thread::sleep(DURATION);
    flag.store(false, std::sync::atomic::Ordering::SeqCst);

    for thread in threads {
        thread.join().unwrap();
    }
}
