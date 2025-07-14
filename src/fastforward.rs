use git2::{FetchOptions, ObjectType, Repository};

pub fn fast_forward(repo: &Repository, fetch_options: &mut FetchOptions) {
    let mut remote = repo.find_remote("origin").expect("Failed to get remote");
    let refspec: &[&str; 0] = &[];
    let _ = remote
        .fetch(refspec, Some(fetch_options), None)
        .expect("Failed to fetch remote");

    repo.fetchhead_foreach(|_ref_name, _remote_url, oid, _is_merge| {
        println!("Checking fast-forward for {}", oid);

        let commit = repo
            .find_annotated_commit(oid.to_owned())
            .expect("Cannot lookup commit");
        let mut current_head = repo.head().expect("Failed to get head");
        let (merge_analysis, _merge_preference) = repo
            .merge_analysis_for_ref(&current_head, &[&commit])
            .expect("failed to get merge analysis");

        if merge_analysis.is_up_to_date() || !merge_analysis.is_fast_forward() {
            return true;
        }

        let tree = repo
            .find_object(oid.to_owned(), Some(ObjectType::Commit))
            .expect("Failed to find object");
        repo.checkout_tree(&tree, None)
            .expect("Failed to checkout tree");

        current_head
            .set_target(oid.to_owned(), "Fast-forwarded")
            .expect("Failed to set target");
        println!("Fast-forwarded to {}", oid);
        true
    })
    .expect("Failed to fetchhead foreach");
}
