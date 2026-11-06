fn boat_local_world(folder: &str) {
    super::boats::terraforged_boat::join_local_world(&saves().join(folder));
}
