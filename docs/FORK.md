# This fork: offline Horizon, Java Fabric, CurseForge

Base: **[Horizon Offline Launcher](https://github.com/gerdaroot/Horizon-Offline-Launcher)**
(`gerdaroot`), a fork of Horizon that plays without a Microsoft account.
Mixing in: **[Horizon Java Bridge](https://github.com/iaxivers/horizon-java-bridge)**
(`iaxivers`), which runs real Fabric jars inside Horizon's Rust engine.

Nothing here removes what those two do; this page says what this fork adds and what is still
missing.

## What works now

### 1. Playing real Minecraft: Java Edition offline, with the mods you already have

The launcher can already start the real game (a "Java instance"), but it asked for a Microsoft
account that owns it and gave the instance an empty folder. Both are gone:

- `account::java_player` falls back to an offline name: the UUID vanilla derives from the name
  (`OfflinePlayer:<name>`) and no token. Single player works; so do servers with
  `online-mode=false`. Online-mode servers still check a real licence and don't.
- `instances::create_java_at` makes a Java instance that **plays in a game folder you already
  have** — your `%APPDATA%\.minecraft`, worlds, options and `mods/` in place — and takes
  `versions/`, `libraries/` and `assets/` from that install (`JavaGame::files`).
- `backend::java_game` no longer needs Mojang's download when the files are already there: it
  reads the version file from Mojang (public metadata, sha1-checked) and copies the client jar
  from the launcher folder that already has it (sha1-checked). Nothing of your install is moved;
  only `versions/<version>/` gains the two files a launcher expects.
- `backend::fetch_libraries` fills in the libraries the classpath names but the folder lacks, from
  the public host each version file names (`libraries.minecraft.net`, or the loader's Maven), so an
  install another launcher keeps its libraries for still starts here.
- `java_launch::library_path`/`library_url` read a library named by its Maven coordinates
  (`net.fabricmc:fabric-loader:0.20.1` with a `url` and no `downloads`), which is how Fabric,
  Quilt and Forge profiles name theirs — without it the loader jar itself was missing from the
  classpath.
- Deleting the instance takes the launcher's own folder only, never your `.minecraft`.

```powershell
# Your existing install, with its mods, as an instance called "Mine":
horizon-launcher instance create Mine --minecraft 26.3 --loader fabric `
  --home "$env:APPDATA\.minecraft"

horizon-launcher instance select Mine
horizon-launcher play
```

In the launcher's own New instance dialog the same thing is a click: pick Modded (or Vanilla),
Minecraft 26.3, then "Play in my .minecraft" under Folder (`JavaChoice::play_in`). The row appears
when `backend::default_minecraft_dir` finds an install, and only for a new instance.

The Fabric loader version is the one its metadata service calls newest for that Minecraft
version; pin another one with `--loader-version`, or set it later with
`horizon-launcher instance loader Mine fabric --version <v>`.

### 2. CurseForge modpacks

`curseforge::import` reads a pack's `manifest.json` (Minecraft version, loader, required files),
makes a Java instance for it, and fetches every required jar from CurseForge's CDN. The pack's
`overrides/` and `client-overrides/` files (configs, quests, resource packs) land in the instance's
game folder; in an existing install your `options.txt`, `servers.dat` and `saves/` are left alone.

No API key is needed: CurseForge's official API wants one, but its public website API answers the
same two ids (`projectID`/`fileID`) and the CDN path is built from the file id and name. Optional
files are named and left out — a launcher can't guess which optional a player wants.

```powershell
horizon-launcher instance curseforge .\MyPack-1.4.2.zip
# or into your own install:
horizon-launcher instance curseforge .\MyPack-1.4.2.zip --home "$env:APPDATA\.minecraft"
```

## Building it for Windows

`pack.ps1` builds the release launcher and packs `dist/horizon-launcher-windows-x64.zip`
(`horizon-launcher.exe`, `install.ps1`, `README-FIRST.txt`); `install.ps1 -Exe` installs from that
folder, without administrator rights, and creates the Start menu and desktop shortcuts.
`packaging/windows/setup.iss` (Inno Setup 6) makes the `Horizon-Offline-Launcher-Setup.exe` the
upstream workflow ships, for whoever has Inno Setup.

## What is not done yet

### Fabric jars inside Horizon (the bridge)

`iaxivers/horizon-java-bridge` is not launcher code only: it is a **game** build. A JVM runs
inside Horizon, Fabric Loader and the real 26.3 classes are loaded against Horizon's mirrored
server, and mod content is converted once per jar set. Its launcher side is six files
(`java_mods`, `java_mods_page`, `java_release`, `java_variant`, `java_bug_report`, plus hooks in
`main.rs`, `home_tabs`, `glass_home`, `instances`, `cli`, `backend`) and a signed game build.

To bring it here:

1. **A game build.** `java-mods` releases carry `horizon-windows-x64-java.zip` /
   `horizon-linux-x86_64-java.tar.gz`, signed with Horizon's own key (`horizon-release-sign`'s
   `release-keys.txt`). This fork's launcher must trust that key as well as the offline fork's,
   and `backend::REPO`/the java release source must point at where those builds live.
2. **The launcher side.** Port the six files and their hook sites onto this (newer) launcher, and
   drop the sign-in gate that build's launcher puts in front of the download.
3. **A game build we control**, if the above dries up: merge the java bridge's game changes with
   this fork's two game patches (`horizon-protocol/src/frame.rs`, offline skins) and build it.
   That needs Rust + the MSVC toolchain, and a release signing key.

What the bridge gives once wired: Fabric 26.3 jars run *inside Horizon* (its world, mobs,
redstone, renderer); about two thirds of the 60 most-downloaded mods work fully, the rest partly,
and renderer replacements (Sodium, Iris) have nothing to replace. What it does not give: the real
Java game. For that — and for mods the bridge doesn't support yet — use a Java instance (above).

## Provenance

- Base and most of the launcher: [Horizon Offline Launcher] and [Horizon], MIT.
- Java bridge design and the six launcher files: [Horizon Java Bridge], MIT.
- This fork adds the offline Java identity, playing in an existing `.minecraft`, the version
  seeding from a local install, and CurseForge pack import.
- Mods and Minecraft files are never included; you bring your own jars and your own copy of the
  game.

[Horizon Offline Launcher]: https://github.com/gerdaroot/Horizon-Offline-Launcher
[Horizon]: https://github.com/horizon-rust/horizon
[Horizon Java Bridge]: https://github.com/iaxivers/horizon-java-bridge
