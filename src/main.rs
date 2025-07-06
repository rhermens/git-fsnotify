use std::{
    env,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use clap::Parser;
use git2::{Cred, PushOptions, RemoteCallbacks, Repository, Signature, Status, StatusOptions};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[arg(short, long)]
    path: PathBuf,
}

fn main() {
    let args = Args::parse();

    let repo = Repository::open(&args.path).expect("Failed to open repository");
    let sig = repo.signature().expect("Failed to get commited signature");
    let mut push_options = PushOptions::new();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, cred, _allowed| {
        println!("Getting credentials");
        Cred::ssh_key(
            cred.expect("Failed to get username from url"),
            None,
            Path::new(&format!(
                "{}/.ssh/id_ed25519",
                env::var("HOME").expect("Failed to get home directory")
            )),
            None,
        )
    });
    push_options.remote_callbacks(callbacks);

    let (tx, rx) = mpsc::channel();

    let mut debouncer =
        new_debouncer(Duration::from_secs(10), None, tx).expect("Failed to create debouncer");

    debouncer
        .watch(&args.path, RecursiveMode::Recursive)
        .expect("failed to watch");

    for result in rx {
        if result
            .expect("Failed to read events")
            .iter()
            .filter(|e| e.kind.is_remove() || e.kind.is_create() || e.kind.is_modify())
            .count()
            == 0
        {
            continue;
        }

        match commit_from_events(&repo, &sig, &mut push_options) {
            Ok(_) => continue,
            Err(err) => println!("Error while commiting: {:?}", err),
        }
    }
}

fn commit_from_events(
    repo: &Repository,
    sig: &Signature,
    push_options: &mut PushOptions,
) -> Result<(), git2::Error> {
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
