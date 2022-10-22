# `lnshot`

🧖🏻‍♀️ Symlink your Steam games' screenshot directories into your Pictures folder

## About

This is a little utility to work around a bugbear of mine with the Steam client. Steam provides a pretty handy in-game screenshot tool, but the screenshots are stored deep in Steam's own folder hierarchy, despite lacking any cloud sync function other than uploading them manually.

Running `lnshot` will generate a set of [symbolic links](https://en.wikipedia.org/wiki/Symbolic_link) (basically, folders which are shortcuts) for each game's screenshot directory within your normal Pictures folder. This means you get this:

```
📂 ~/Pictures/Steam Screenshots
└ 📂 Ticky
  ├ 📂 Hardspace Shipbreaker
  │ └ 🌌 20221020102933_1.jpg
  ├ 📂 Need for Speed: Most Wanted
  │ └ 🌃 20221005164632_1.jpg
  └ 📂 The Big Con
    └ 🏞 20221005164632_1.jpg
```

Instead of this:

```
📂 ~/.local/share/Steam/userdata
└ 📂 69420691
  └ 📂 760
    └ 📂 remote
      ├ 📂 1139280
      │ └ 📂 screenshots
      │   └ 🏞 20221005164632_1.jpg
      ├ 📂 1161580
      │ └ 📂 screenshots
      │   └ 🌌 20221020102933_1.jpg
      └ 📂 6547380
        └ 📂 screenshots
          └ 🌃 20221005164632_1.jpg
```

`lnshot` can detect Steam's installation directory, and automatically find your Pictures folder across all three supported Steam platforms.

User folders are generated for each Steam user logged into your system (filtering is not yet supported). Game folders will be named after your game title for non-Steam shortcuts, and named the same as the `steamapps/common` installation folder for games managed by Steam, which is usually a reasonable name. This may change to use the full Steam app name in the future.

`lnshot` does this offline, using only the metadata Steam already has stored on your hard disk.

## Installation

Builds are not currently provided, so it's currently expected that you know your way around the Rust compiler.

Clone this repository and run `cargo install --path .` inside it.

## Usage

Run `lnshot` to automatically symlink to `Steam Screenshots` within your Pictures folder.

`lnshot --help` provides information about other options, including using a different name for the `Steam Screenshots` folder.

### Automation

I am currently investigating ways to automate running this (i.e. when new game screenshot folders are created), in particular on SteamOS. The command-line interface may change to accommodate this if necessary.
