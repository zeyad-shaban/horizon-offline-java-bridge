//! A Java instance that plays in the `.minecraft` a player already has: its worlds, options and
//! mods stay there, and deleting the instance leaves that folder alone (only the launcher's own
//! folder for the instance goes to the Trash).

/// One test: the library reads where the game and the launcher keep their folders once per
/// process, so the cases share them instead of racing for the variables.
#[test]
fn an_existing_minecraft_becomes_an_instance_that_outlives_it() {
    let root = std::env::temp_dir().join(format!(
        "horizon-adopt-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let minecraft = root.join("minecraft");
    std::fs::create_dir_all(minecraft.join("mods")).unwrap();
    std::fs::write(minecraft.join("mods/example.jar"), b"not really a jar").unwrap();
    std::fs::write(minecraft.join("options.txt"), "fov:0.0\n").unwrap();
    unsafe {
        std::env::set_var("HORIZON_HOME", root.join("game"));
        std::env::set_var("HORIZON_LAUNCHER_HOME", root.join("launcher"));
        std::env::set_var("HORIZON_LAUNCHER_TRASH", root.join("trash"));
    }

    // A folder that isn't there is refused, not made.
    let gone = root.join("nowhere");
    let err =
        horizon_launcher::instances::create_java_at("Mine", "26.3", &gone, None, None).unwrap_err();
    assert!(err.contains("no Minecraft folder"), "{err}");

    // The player's folder becomes the instance's game folder, its files (mods and all) in place.
    let made =
        horizon_launcher::instances::create_java_at("Mine", "26.3", &minecraft, None, None)
            .unwrap();
    assert_eq!(made.home, minecraft);
    let java = made.java.clone().expect("a Java instance");
    assert_eq!(java.minecraft, "26.3");
    assert_eq!(java.files.as_deref(), Some(minecraft.as_path()));
    let listed = horizon_launcher::instances::find("Mine").unwrap();
    assert_eq!(listed.home, minecraft);
    assert!(horizon_launcher::instances::create_java_at(
        "Mine",
        "26.3",
        &minecraft,
        None,
        None
    )
    .is_err());

    // Game files may be elsewhere: a linked layout beside the folder it plays in.
    let files = root.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let split = horizon_launcher::instances::create_java_at(
        "Split",
        "26.3",
        &minecraft,
        Some(&files),
        None,
    )
    .unwrap();
    assert_eq!(
        split.java.expect("a Java instance").files.as_deref(),
        Some(files.as_path())
    );

    // Deleting an instance takes the launcher's folder, never the player's game folder.
    horizon_launcher::instances::delete("Mine").unwrap();
    assert!(minecraft.join("mods/example.jar").is_file());
    assert!(minecraft.join("options.txt").is_file());
    assert!(horizon_launcher::instances::find("Mine").is_err());
    let _ = std::fs::remove_dir_all(&root);
}
