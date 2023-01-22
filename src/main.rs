use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use directories::UserDirs;
use steamid_ng::SteamID;
use steamlocate::{SteamDir};

/// Symlink your Steam games' screenshot directories into your Pictures folder
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Specify the name of the directory to manage inside your Pictures folder
    #[arg(short, long, default_value = "Steam Screenshots")]
    pictures_directory_name: String,

    #[command(subcommand)]
    action: Option<Action>,
}

/// Action to perform when running from the command line
#[derive(Subcommand, Debug)]
enum Action {
    /// Runs once, symlinking directories for games with screenshot directories on-disk.
    /// If no command is explicitly specified, this is the default behaviour.
    Go,

    /// Keeps running, watching Steam's screenshot directories for newly-added game directories.
    ///
    /// Intended for use as a user-level background process.
    Daemon,
}

/// I am the `main` function, with [`anyhow`](anyhow) result magic.
fn main() -> Result<()> {
    let args = Args::parse();

    let mut steam_dir =
        SteamDir::locate().with_context(|| "Failed to locate Steam on this computer")?;

    let steam_user_data_dir = steam_dir.path.join("userdata");

    let screenshots_dir = UserDirs::new()
        .with_context(|| "Failed to fetch user directory information")?
        .picture_dir()
        .with_context(|| "Failed to find picture directory")?
        .join(args.pictures_directory_name);

    match args.action.unwrap_or(Action::Go) {
        Action::Go => {
            let steam_apps = steam_dir.apps().to_owned();
            let steam_shortcuts = steam_dir.shortcuts().to_owned();

            let users_list =
                steamy_vdf::load(steam_dir.path.join("config").join("loginusers.vdf"))?
                    .get("users")
                    .with_context(|| "Failed to find any Steam users")?
                    .as_table()
                    .with_context(|| "Failed to find any Steam users")?
                    .to_owned();

            for (steamid_str, userinfo) in users_list.iter() {
                let steamid = SteamID::from(steamid_str.parse::<u64>()?);

                println!("[{}] Processing user", steamid_str);

                let steamid_steam_user_data_dir =
                    steam_user_data_dir.join(steamid.account_id().to_string());

                let steam_user_screenshots_dir =
                    steamid_steam_user_data_dir.join("760").join("remote");

                // If there's no screenshot folder, just move on to the next user
                if !steam_user_screenshots_dir.is_dir() {
                    println!(
                        "[{}] User does not have a Steam screenshot folder!",
                        steamid_str
                    );
                    continue;
                }

                println!(
                    "[{}] Found Steam screenshot folder {:?}",
                    steamid_str, steam_user_screenshots_dir
                );

                let name = userinfo
                    .get("PersonaName")
                    .with_context(|| {
                        format!("Failed to retrieve account PersonaName for {}", steamid_str)
                    })?
                    .as_str()
                    .with_context(|| {
                        format!(
                            "Failed to convert PersonaName for {} into a string",
                            steamid_str
                        )
                    })?;

                println!("[{}] Found display name {:?} for user", steamid_str, name);

                let target_screenshots_dir = screenshots_dir.join(name);
                if !target_screenshots_dir.is_dir() {
                    std::fs::create_dir_all(target_screenshots_dir.clone())?;
                }

                for entry in steam_user_screenshots_dir.read_dir()? {
                    let entry = entry?;

                    if !entry.path().is_dir() {
                        continue;
                    }

                    let steam_app_screenshot_path = entry.path().join("screenshots");
                    let filename = entry.file_name();

                    let appid_str = filename
                        .to_str()
                        .with_context(|| "Failed to retrieve app id")?;

                    let appid = appid_str.parse::<u64>()?;

                    println!(
                        "[{}; {:20}] Found app screenshot folder: {:?}",
                        steamid_str, appid, steam_app_screenshot_path
                    );

                    let symlink_name = if let Some(Some(app)) = steam_apps.get(&(appid as u32)) {
                        app.path
                            .file_name()
                            .with_context(|| "Failed to retrieve file name from install path")?
                    } else if let Some(shortcut) = steam_shortcuts.iter().find(|shortcut| {
                        u64::from(shortcut.appid & 0x7fffff) == appid
                            || shortcut.steam_id() == appid
                    }) {
                        std::ffi::OsStr::new(&shortcut.app_name)
                    } else {
                        std::ffi::OsStr::new(appid_str)
                    };

                    let target_symlink_path = target_screenshots_dir.join(symlink_name);

                    println!(
                        "[{}; {:20}] target_symlink_path: {:?}",
                        steamid_str, appid, target_symlink_path
                    );

                    if target_symlink_path.is_symlink() {
                        match symlink::remove_symlink_auto(&target_symlink_path) {
                            Ok(_) => {}
                            Err(error) => {
                                println!("Error unlinking {:?}: {}", target_symlink_path, error)
                            }
                        };
                    }

                    match symlink::symlink_dir(&steam_app_screenshot_path, &target_symlink_path) {
                        Ok(_) => {}
                        Err(error) => println!(
                            "Error symlinking {:?} to {:?}: {}",
                            steam_app_screenshot_path, target_symlink_path, error
                        ),
                    };
                }

                // Cleanup phase: remove any app ID-based symlinks for which we currently know the app's name
                for entry in target_screenshots_dir.read_dir()? {
                    let entry = entry?;
                    let filename = entry.file_name();

                    let appid_str = filename
                        .to_str()
                        .with_context(|| "Failed to retrieve an app id")?;

                    if let Ok(appid) = appid_str.parse::<u64>() {
                        println!("[{}] Cleanup found dir with app id: {}", steamid_str, appid);

                        if steam_apps.contains_key(&(appid as u32))
                            || steam_shortcuts.iter().any(|shortcut| {
                                u64::from(shortcut.appid & 0x7fffff) == appid
                                    || shortcut.steam_id() == appid
                            })
                        {
                            let entry_symlink_path = entry.path();

                            if entry_symlink_path.is_symlink() {
                                println!(
                                    "[{}] App {} is installed! We don't need this symlink",
                                    steamid_str, appid
                                );

                                match symlink::remove_symlink_auto(&entry_symlink_path) {
                                    Ok(_) => {}
                                    Err(error) => {
                                        println!(
                                            "Error unlinking {:?}: {}",
                                            entry_symlink_path, error
                                        )
                                    }
                                };
                            } else {
                                println!(
                                    "[{}] App {} is installed, but the matching item is not a symlink; skipping!",
                                    steamid_str, appid
                                );
                            }
                        }
                    }
                }
            }
        }
        Action::Daemon => {
            use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
            use path_matchers::PathMatcher;

            println!("Setting up file system watcher thread...");

            let (transmit_channel, receive_channel) = std::sync::mpsc::channel();

            let mut debouncer =
                new_debouncer(std::time::Duration::from_secs(5), None, transmit_channel)?;

            debouncer
                .watcher()
                .watch(&steam_user_data_dir, RecursiveMode::Recursive)?;

            let glob_path = steam_user_data_dir
                .join("*")
                .join("760")
                .join("remote")
                .join("*");
            let glob_str = glob_path
                .to_str()
                .with_context(|| "Unable to format file path matcher")?;
            println!(
                "Watching path at {:?}, with glob {:?}",
                steam_user_data_dir, glob_str
            );
            let glob_filter = path_matchers::glob(glob_str)?;

            for events in receive_channel.into_iter().flatten() {
                for event in events {
                    if !glob_filter.matches(&event.path) || !event.path.exists() {
                        continue;
                    }

                    let (steamid_from_dir, appid) = {
                        let mut path_components = event
                            .path
                            .strip_prefix(&steam_user_data_dir)?
                            .components()
                            .filter_map(|component| match component {
                                std::path::Component::Normal(name) => Some(name),
                                _ => None,
                            });

                        (
                            path_components
                                .next()
                                .with_context(|| "Unable to find required user ID component")?
                                .to_str()
                                .with_context(|| "Unable to find required user ID component")?
                                .parse::<u64>()?,
                            path_components
                                .nth(2)
                                .with_context(|| "Unable to find required app ID component")?
                                .to_str()
                                .with_context(|| "Unable to find required app ID component")?
                                .parse::<u64>()?,
                        )
                    };

                    println!(
                        "[{}; {:20}] Change detected in screenshot dir for app",
                        steamid_from_dir, appid
                    );

                    let users_list =
                        steamy_vdf::load(steam_dir.path.join("config").join("loginusers.vdf"))?
                            .get("users")
                            .with_context(|| "Failed to find any Steam users")?
                            .as_table()
                            .with_context(|| "Failed to find any Steam users")?
                            .to_owned();

                    let name = match users_list.iter().find(|(steamid_str, _userinfo)| {
                        let steamid = SteamID::from(steamid_str.parse::<u64>().unwrap_or(0));

                        u64::from(steamid.account_id()) == steamid_from_dir
                    }) {
                        Some((_steamid_str, userinfo)) => Some(
                            userinfo
                                .get("PersonaName")
                                .with_context(|| {
                                    format!(
                                        "Failed to retrieve account PersonaName for {}",
                                        steamid_from_dir
                                    )
                                })?
                                .as_str()
                                .with_context(|| {
                                    format!(
                                        "Failed to convert PersonaName for {} into a string",
                                        steamid_from_dir
                                    )
                                })?,
                        ),
                        None => None,
                    }
                    .with_context(|| {
                        format!("Failed to get account information for {}", steamid_from_dir)
                    })?;

                    println!(
                        "[{}; {:20}] Found display name {:?} for user",
                        steamid_from_dir, appid, name
                    );

                    let target_screenshots_dir = screenshots_dir.join(name);
                    if !target_screenshots_dir.is_dir() {
                        std::fs::create_dir_all(target_screenshots_dir.clone())?;
                    }

                    let steamid_str = steamid_from_dir.to_string();

                    let steamid_steam_user_data_dir = steam_user_data_dir.join(&steamid_str);

                    let steam_user_screenshots_dir =
                        steamid_steam_user_data_dir.join("760").join("remote");

                    // If there's no screenshot folder, just move on to the next event
                    if !steam_user_screenshots_dir.is_dir() {
                        println!(
                            "[{}] User does not have a Steam screenshot folder!",
                            steamid_str
                        );
                        continue;
                    }

                    println!(
                        "[{}; {:20}] Found Steam screenshot folder {:?} for user {:?}",
                        steamid_from_dir, appid, steam_user_screenshots_dir, name
                    );

                    let appid_str = appid.to_string();

                    let steam_app_screenshot_path = steam_user_screenshots_dir
                        .join(&appid_str)
                        .join("screenshots");

                    println!(
                        "[{}; {:20}] Found app screenshot folder: {:?}",
                        steamid_str, appid, steam_app_screenshot_path
                    );

                    let steam_apps = steam_dir.apps().to_owned();
                    let steam_shortcuts = steam_dir.shortcuts();

                    let symlink_name = if let Some(Some(app)) = steam_apps.get(&(appid as u32)) {
                        app.path
                            .file_name()
                            .with_context(|| "Failed to retrieve file name from install path")?
                    } else if let Some(shortcut) = steam_shortcuts.iter().find(|shortcut| {
                        u64::from(shortcut.appid & 0x7fffff) == appid
                            || shortcut.steam_id() == appid
                    }) {
                        std::ffi::OsStr::new(&shortcut.app_name)
                    } else {
                        std::ffi::OsStr::new(&appid_str)
                    };

                    let target_symlink_path = target_screenshots_dir.join(symlink_name);

                    println!(
                        "[{}; {:20}] target_symlink_path: {:?}",
                        steamid_str, appid, target_symlink_path
                    );

                    if target_symlink_path.is_symlink() {
                        match symlink::remove_symlink_auto(&target_symlink_path) {
                            Ok(_) => {}
                            Err(error) => {
                                println!("Error unlinking {:?}: {}", target_symlink_path, error)
                            }
                        };
                    }

                    match symlink::symlink_dir(&steam_app_screenshot_path, &target_symlink_path) {
                        Ok(_) => {}
                        Err(error) => println!(
                            "Error symlinking {:?} to {:?}: {}",
                            steam_app_screenshot_path, target_symlink_path, error
                        ),
                    };
                }
            }
        }
    };

    Ok(())
}
