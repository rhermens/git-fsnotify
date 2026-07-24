mod fastforward;
mod ident;
mod push;

use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use git2::{FetchOptions, PushOptions, RemoteCallbacks, Repository};
use tracing::Level;

use crate::{fastforward::fast_forward, ident::credentials_callback, push::push_worktree};

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[arg(short, long, value_parser=path_expand)]
    path: PathBuf,

    #[arg(long, default_value_t = Level::INFO)]
    log_level: Level,

    #[arg(long, default_value_t = 60)]
    interval: u64,

    #[arg(long, default_value_t = 30)]
    timeout: u64,

    #[arg(long, hide = true)]
    once: bool,
}

fn path_expand(value: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(
        shellexpand::full(value)
            .map_err(|_| "Invalid path")?
            .into_owned(),
    ))
}

#[derive(Debug, Eq, PartialEq)]
enum ChildOutcome {
    Exited(ExitStatus),
    TimedOut,
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> io::Result<ChildOutcome> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(ChildOutcome::Exited(status));
        }

        if Instant::now() >= deadline {
            match child.kill() {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
                Err(error) => return Err(error),
            }
            child.wait()?;
            return Ok(ChildOutcome::TimedOut);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn sync_child_command(args: &Args, executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--once")
        .arg("--path")
        .arg(&args.path)
        .arg("--log-level")
        .arg(args.log_level.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

fn sync_once(path: &Path) -> Result<(), git2::Error> {
    let repo = Repository::open(path)?;

    let mut push_options = PushOptions::new();
    let mut push_callbacks = RemoteCallbacks::default();
    push_callbacks.credentials(credentials_callback);
    push_options.remote_callbacks(push_callbacks);

    let mut fetch_options = FetchOptions::new();
    let mut fetch_callbacks = RemoteCallbacks::default();
    fetch_callbacks.credentials(credentials_callback);
    fetch_options.remote_callbacks(fetch_callbacks);

    if let Err(error) = fast_forward(&repo, &mut fetch_options) {
        tracing::error!("error fast-forwarding: {error}");
    }

    push_worktree(&repo, &mut push_options)
}

fn supervise(args: &Args) -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let timeout = Duration::from_secs(args.timeout);
    let interval = Duration::from_secs(args.interval);

    loop {
        let mut child = sync_child_command(args, &executable).spawn()?;
        match wait_with_timeout(&mut child, timeout)? {
            ChildOutcome::Exited(status) if status.success() => {}
            ChildOutcome::Exited(status) => {
                tracing::error!("sync child exited with {status}");
            }
            ChildOutcome::TimedOut => {
                tracing::error!("sync child exceeded {timeout:?} and was killed");
            }
        }
        thread::sleep(interval);
    }
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(args.log_level)
        .init();

    if args.once {
        if let Err(error) = sync_once(&args.path) {
            tracing::error!("sync failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = supervise(&args) {
        tracing::error!("supervisor failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
        time::{Duration, SystemTime},
    };

    use super::{Args, ChildOutcome, sync_child_command, sync_once, wait_with_timeout};

    fn git(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output was not UTF-8")
            .trim()
            .to_owned()
    }

    #[test]
    fn sync_once_commits_and_pushes_worktree_changes() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock predates Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("git-watch-test-{unique}"));
        let origin = root.join("origin.git");
        let worktree = root.join("worktree");
        fs::create_dir_all(&root).expect("failed to create test directory");

        git(&root, &["init", "--bare", origin.to_str().unwrap()]);
        git(&root, &["init", "-b", "master", worktree.to_str().unwrap()]);
        git(&worktree, &["config", "user.name", "Git Watch Test"]);
        git(
            &worktree,
            &["config", "user.email", "git-watch@example.test"],
        );
        fs::write(worktree.join("note.txt"), "before\n").expect("failed to write fixture");
        git(&worktree, &["add", "note.txt"]);
        git(&worktree, &["commit", "-m", "Initial"]);
        git(
            &worktree,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&worktree, &["push", "-u", "origin", "master"]);
        fs::write(worktree.join("note.txt"), "after\n").expect("failed to change fixture");

        sync_once(&worktree).expect("one-shot sync failed");

        assert_eq!(git(&worktree, &["status", "--porcelain"]), "");
        assert_eq!(
            git(&worktree, &["rev-parse", "HEAD"]),
            git(&origin, &["rev-parse", "refs/heads/master"])
        );
        fs::remove_dir_all(root).expect("failed to clean test directory");
    }

    #[test]
    fn child_command_runs_one_sync_with_expanded_arguments() {
        let args =
            Args::try_parse_from(["git-watch", "--path", "/tmp/repo", "--log-level", "debug"])
                .expect("failed to parse arguments");

        let command = sync_child_command(&args, "/bin/git-watch".as_ref());
        let command_args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), "/bin/git-watch");
        assert_eq!(
            command_args,
            ["--once", "--path", "/tmp/repo", "--log-level", "DEBUG"]
        );
    }

    #[test]
    fn parses_hidden_once_mode_and_operation_timeout() {
        let args = Args::try_parse_from([
            "git-watch",
            "--path",
            "/tmp/repo",
            "--timeout",
            "7",
            "--once",
        ])
        .expect("failed to parse arguments");

        assert!(args.once);
        assert_eq!(args.timeout, 7);
    }

    #[test]
    fn wait_with_timeout_kills_and_reaps_slow_child() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn test child");

        let outcome = wait_with_timeout(&mut child, Duration::from_millis(50))
            .expect("failed to wait for test child");

        assert_eq!(outcome, ChildOutcome::TimedOut);
        assert!(
            child
                .try_wait()
                .expect("failed to inspect test child")
                .is_some(),
            "timed-out child was not reaped"
        );
    }
}
