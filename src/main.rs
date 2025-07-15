mod fastforward;
mod ident;
mod push;

use std::{path::PathBuf, sync::mpsc, time::Duration};

use clap::Parser;
use git2::{FetchOptions, PushOptions, RemoteCallbacks, Repository};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

use crate::{fastforward::fast_forward, ident::credentials_callback, push::push_worktree};

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[arg(short, long)]
    path: PathBuf,
}

#[derive(Debug)]
enum EventKind {
    Tick(()),
    Fs(()),
}

fn main() {
    let args = Args::parse();

    let (tx, rx) = mpsc::channel();
    let repo = Repository::open(&args.path).expect("Failed to open repository");

    let mut push_options = PushOptions::new();
    let mut push_callbacks = RemoteCallbacks::default();
    push_callbacks.credentials(credentials_callback);
    push_options.remote_callbacks(push_callbacks);

    let mut fetch_options = FetchOptions::new();
    let mut fetch_callbacks = RemoteCallbacks::default();
    fetch_callbacks.credentials(credentials_callback);
    fetch_options.remote_callbacks(fetch_callbacks);

    start_pull_interval(tx.clone());
    start_fs_watch(args.path, tx.clone());

    for e in rx {
        tracing::trace!("Received event: {:?}", e);
        match e {
            EventKind::Tick(_) => {
                tracing::info!("Pulling changes");
                fast_forward(&repo, &mut fetch_options);
            }
            EventKind::Fs(_) => {
                tracing::info!("Committing changes");
                if let Err(err) = push_worktree(&repo, &mut push_options) {
                    tracing::error!("Error during commit: {:?}", err);
                }
            }
        }
    }
}

fn start_pull_interval(tx: mpsc::Sender<EventKind>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            tx.send(EventKind::Tick(()))
                .expect("Failed to send interval tick");
        }
    });
}

fn start_fs_watch(path: PathBuf, tx: mpsc::Sender<EventKind>) {
    std::thread::spawn(move || {
        let (dtx, rx) = mpsc::channel();

        let mut debouncer =
            new_debouncer(Duration::from_secs(10), None, dtx).expect("Failed to create debouncer");

        debouncer
            .watch(&path, RecursiveMode::Recursive)
            .expect("failed to watch");

        for result in rx {
            if result
                .expect("Failed to read events")
                .iter()
                .filter(|e| e.kind.is_remove() || e.kind.is_create() || e.kind.is_modify())
                .count()
                == 0
            {
                tracing::debug!("No relevant file system changes detected, skipping commit.");
                continue;
            }

            tracing::debug!("Fs change, broadcasting event");

            tx.send(EventKind::Fs(()))
                .expect("Failed to send file system event");
        }
    });
}
