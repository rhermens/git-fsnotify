use std::{
    env, path::{Path, PathBuf}, sync::mpsc, time::Duration
};

use clap::Parser;
use git2::{Cred, CredentialType, FetchOptions, ObjectType, PushOptions, RemoteCallbacks, Repository, Status, StatusOptions};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

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
        println!("Received event: {:?}", e);
        match e {
            EventKind::Tick(_) => {
                println!("Received tick event, pulling changes...");
                fast_forward(&repo, &mut fetch_options);
            }
            EventKind::Fs(_) => {
                println!("Received file system event, committing changes...");
                if let Err(err) = commit_worktree(&repo, &mut push_options) {
                    eprintln!("Error during commit: {:?}", err);
                }
            }
        }
    }
}

fn credentials_callback(_remote_url: &str, cred: Option<&str>, _allowed: CredentialType) -> Result<Cred, git2::Error> {
    Cred::ssh_key(
        cred.expect("Failed to get username from url"),
        None,
        Path::new(&format!(
            "{}/.ssh/id_ed25519",
            env::var("HOME").expect("Failed to get home directory")
        )),
        None,
    )
}

fn start_pull_interval(tx: mpsc::Sender<EventKind>) {
    let _ = std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            println!("Sending interval tick");
            tx.send(EventKind::Tick(())).expect("Failed to send interval tick");
        }
    });
}

fn fast_forward(repo: &Repository, fetch_options: &mut FetchOptions) {
    let mut remote = repo.find_remote("origin").expect("Failed to get remote");
    let refspec: &[&str; 0] = &[];
    let _ = remote.fetch(refspec, Some(fetch_options), None).expect("Failed to fetch remote");

    repo.fetchhead_foreach(|_ref_name, _remote_url, oid, _is_merge| {
        println!("Checking fast-forward for {}", oid);

        let commit = repo.find_annotated_commit(oid.to_owned()).expect("Cannot lookup commit");
        let mut current_head = repo.head().expect("Failed to get head");
        let (merge_analysis, _merge_preference) = repo.merge_analysis_for_ref(&current_head, &[&commit]).expect("failed to get merge analysis");

        if merge_analysis.is_up_to_date() || !merge_analysis.is_fast_forward() {
            return true;
        }

        let tree = repo.find_object(oid.to_owned(), Some(ObjectType::Commit)).expect("Failed to find object");
        repo.checkout_tree(&tree, None).expect("Failed to checkout tree");

        current_head.set_target(oid.to_owned(), "Fast-forwarded").expect("Failed to set target");
        println!("Fast-forwarded to {}", oid);
        true
    }).expect("Failed to fetchhead foreach");
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
                println!("No relevant file system changes detected, skipping commit.");
                continue;
            }
            println!("File system change detected, committing...");

            tx.send(EventKind::Fs(())).expect("Failed to send file system event");
        }
    });
}

fn commit_worktree(
    repo: &Repository,
    push_options: &mut PushOptions,
) -> Result<(), git2::Error> {
    let sig = repo.signature().expect("Failed to get commited signature");
    let status = repo.statuses(Some(
        StatusOptions::new()
            .include_ignored(false)
            .include_unmodified(false)
            .include_unreadable(false),
    ))?;
    if status.iter().len() == 0 {
        return Ok(());
    }

    let mut index = repo.index()?;
    for entry in status.iter() {
        match entry.status() {
            Status::WT_NEW | Status::WT_MODIFIED | Status::INDEX_MODIFIED | Status::INDEX_NEW => {
                index.add_path(Path::new(entry.path().expect("Failed to get status path")))?;
            }
            Status::WT_DELETED | Status::INDEX_DELETED => {
                index.remove_path(Path::new(entry.path().expect("Failed to get status path")))?;
            }
            _ => continue,
        }
    }

    let _ = index.write()?;
    let current_head = repo.find_commit(repo.head()?.target().expect("No target"))?;
    let _ = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Autocommit",
        &repo.find_tree(index.write_tree()?)?,
        &[&current_head],
    )?;
    repo.find_remote("origin")?
        .push(&["refs/heads/master"], Some(push_options))
}
