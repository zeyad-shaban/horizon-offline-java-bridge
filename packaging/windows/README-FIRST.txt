Horizon Offline Java Bridge
===========================

This is the Horizon Offline Launcher (Minecraft: Java Edition 26.3 rewritten in Rust, no Microsoft
account needed), extended so that it plays *your* Minecraft: Java Edition install: your worlds, your
options, your mods.

Install
-------

  powershell -ExecutionPolicy Bypass -File install.ps1 -Exe .\horizon-launcher.exe

No administrator rights. It puts the launcher in %LOCALAPPDATA%\Programs\HorizonOfflineLauncher,
with a Start menu and a desktop shortcut. Keep this folder: the zip is what to share.

Play Minecraft with your own mods (Fabric 26.3, Sodium, Fabric API, and the rest)
--------------------------------------------------------------------------------

1. Open the launcher.
2. New instance, "Modded", Minecraft 26.3, Fabric.
3. Under "Game folder", click "Play in my .minecraft". Your own %APPDATA%\.minecraft is used in
   place: nothing is copied, nothing is deleted, and the launcher never needs a Microsoft account.
4. Press Play. The first Play only fetches the small 26.3 version file from Mojang (your client jar
   is already there) and, if needed, the Java Mojang ships for this version.

From a terminal, the same thing:

  horizon-launcher instance create Mine --minecraft 26.3 --loader fabric --home "$env:APPDATA\.minecraft"
  horizon-launcher instance select Mine
  horizon-launcher play

CurseForge modpacks
-------------------

  horizon-launcher instance curseforge "C:\Users\you\Downloads\SomePack.zip"

Reads the pack's manifest.json, makes an instance for its Minecraft version and loader, and downloads
every required mod from CurseForge's CDN. No account, no API key. Add --home to drop the pack into an
existing install instead of a new folder.

What it does not do
-------------------

- Running Fabric mods *inside* Horizon's Rust engine is the other half of this fork's name. It is not
  in this build: Horizon instances still play without Java mods, and Java instances play real
  Minecraft. See docs/FORK.md in the repository.
- Updates come from the fork's own releases, signed with its own key; this locally built launcher has
  no such release to update to, so it stays as it is.
