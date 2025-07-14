use std::path::Path;

use git2::{PushOptions, Repository, Status, StatusOptions};

pub fn push_worktree(repo: &Repository, push_options: &mut PushOptions) -> Result<(), git2::Error> {
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
