use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use directories::UserDirs;
use std::collections::HashMap;
use steamid_ng::SteamID;
use steamlocate::SteamDir;

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref BUILT_IN_APPS: HashMap<u64, &'static str> = HashMap::from([
        (0, "Unknown"),
        (5, "Dedicated Server"),
        (7, "Steam Client"),
        (910, "Steam Media Player"),
    ]);
}

/// Symlink your Steam games' screenshot directories into your Pictures folder
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Specify the name of the directory to manage inside your Pictures folder
    #[arg(short, long, default_value = "Steam Screenshots")]
    pictures_directory_name: String,

    /// Specify one specific user by 64-bit Steam ID. NOTE: Skips creating user-named folders inside the managed directory.
    #[arg(short, long)]
    single_user_id64: Option<u64>,

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

fn process_appid_for_screenshot_paths(
    steam_dir: &mut SteamDir,
    appid: u64,
    steam_app_screenshot_path: &std::path::Path,
    target_screenshots_dir: &std::path::Path,
) -> Result<()> {
    use std::ffi::OsString;

    let steam_apps = steam_dir.apps().to_owned();
    let steam_shortcuts = steam_dir.shortcuts();

    let symlink_name = if let Some(app_name) = BUILT_IN_APPS.get(&appid) {
        OsString::from(app_name)
    } else if let Some(Some(app)) = steam_apps.get(&(appid as u32)) {
        app.path
            .file_name()
            .with_context(|| "Failed to retrieve file name from install path")?
            .to_os_string()
    } else if let Some(shortcut) = steam_shortcuts.iter().find(|shortcut| {
        u64::from(shortcut.appid & 0x7fffff) == appid || shortcut.steam_id() == appid
    }) {
        OsString::from(&shortcut.app_name)
    } else {
        OsString::from(&appid.to_string())
    };

    let target_symlink_path = target_screenshots_dir.join(symlink_name);

    println!(
        "[a{:20}] {:?} -> {:?}",
        appid, steam_app_screenshot_path, target_symlink_path,
    );

    if target_symlink_path.is_symlink() {
        match symlink::remove_symlink_auto(&target_symlink_path) {
            Ok(_) => {}
            Err(error) => {
                println!("Error unlinking {:?}: {}", target_symlink_path, error)
            }
        };
    }

    match symlink::symlink_dir(steam_app_screenshot_path, &target_symlink_path) {
        Ok(_) => {}
        Err(error) => println!(
            "Error symlinking {:?} to {:?}: {}",
            steam_app_screenshot_path, target_symlink_path, error
        ),
    };

    Ok(())
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

            for (steamid64_str, userinfo) in users_list.iter() {
                let steamid = SteamID::try_from(steamid64_str.parse::<u64>()?)?;

                if let Some(single_user_id64) = args.single_user_id64 {
                    if single_user_id64 != steamid.into() {
                        println!("[u{}] Skipping mismatching user", steamid64_str);
                        continue;
                    }
                }

                let steamid_steam_user_data_dir =
                    steam_user_data_dir.join(steamid.account_id().to_string());

                let steam_user_screenshots_dir =
                    steamid_steam_user_data_dir.join("760").join("remote");

                // If there's no screenshot folder, just move on to the next user
                if !steam_user_screenshots_dir.is_dir() {
                    println!(
                        "[u{}] User does not have a Steam screenshot folder!",
                        steamid64_str
                    );
                    continue;
                }

                println!(
                    "[u{}] Found Steam screenshot folder {:?}",
                    steamid64_str, steam_user_screenshots_dir
                );

                let mut target_screenshots_dir = screenshots_dir.clone();

                if args.single_user_id64.is_none() {
                    let name = userinfo
                        .get("PersonaName")
                        .with_context(|| {
                            format!(
                                "Failed to retrieve account PersonaName for {}",
                                steamid64_str
                            )
                        })?
                        .as_str()
                        .with_context(|| {
                            format!(
                                "Failed to convert PersonaName for {} into a string",
                                steamid64_str
                            )
                        })?;

                    println!("[u{}] Display name: {:?}", steamid64_str, name);

                    target_screenshots_dir = target_screenshots_dir.join(name);
                }

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

                    process_appid_for_screenshot_paths(
                        &mut steam_dir,
                        appid,
                        &steam_app_screenshot_path,
                        &target_screenshots_dir,
                    )?;
                }

                // Cleanup phase: remove any app ID-based symlinks for which we currently know the app's name
                for entry in target_screenshots_dir.read_dir()? {
                    let entry = entry?;
                    let filename = entry.file_name();

                    let appid_str = filename
                        .to_str()
                        .with_context(|| "Failed to retrieve an app id")?;

                    if let Ok(appid) = appid_str.parse::<u64>() {
                        println!("[u{}] Cleaning appid dir: {}", steamid64_str, appid);

                        if steam_apps.contains_key(&(appid as u32))
                            || steam_shortcuts.iter().any(|shortcut| {
                                u64::from(shortcut.appid & 0x7fffff) == appid
                                    || shortcut.steam_id() == appid
                            })
                        {
                            let entry_symlink_path = entry.path();

                            if entry_symlink_path.is_symlink() {
                                println!(
                                    "[u{}] App {} is installed! We don't need this symlink",
                                    steamid64_str, appid
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
                                    "[u{}] App {} is installed, but the matching item is not a symlink; skipping!",
                                    steamid64_str, appid
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

            let mut debouncer = new_debouncer(std::time::Duration::from_secs(5), transmit_channel)?;

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

                    let (steam_account_id_from_dir, appid) = {
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
                        "[a{:20}] Change detected for user {}",
                        appid, steam_account_id_from_dir
                    );

                    let users_list =
                        steamy_vdf::load(steam_dir.path.join("config").join("loginusers.vdf"))?
                            .get("users")
                            .with_context(|| "Failed to find any Steam users")?
                            .as_table()
                            .with_context(|| "Failed to find any Steam users")?
                            .to_owned();

                    let (steamid64_str, name) =
                        match users_list.iter().find(|(steamid64_str, _userinfo)| {
                            let steamid =
                                SteamID::try_from(steamid64_str.parse::<u64>().unwrap_or(0))
                                    .unwrap();

                            u64::from(steamid.account_id()) == steam_account_id_from_dir
                        }) {
                            Some((steamid64_str, userinfo)) => Some((
                                steamid64_str,
                                userinfo
                                    .get("PersonaName")
                                    .with_context(|| {
                                        format!(
                                            "Failed to retrieve account PersonaName for {}",
                                            steam_account_id_from_dir
                                        )
                                    })?
                                    .as_str()
                                    .with_context(|| {
                                        format!(
                                            "Failed to convert PersonaName for {} into a string",
                                            steam_account_id_from_dir
                                        )
                                    })?,
                            )),
                            None => None,
                        }
                        .with_context(|| {
                            format!(
                                "Failed to get account information for {}",
                                steam_account_id_from_dir
                            )
                        })?;

                    let mut target_screenshots_dir = screenshots_dir.clone();

                    if let Some(single_user_id64) = args.single_user_id64 {
                        if single_user_id64 != steamid64_str.parse::<u64>().unwrap_or(0) {
                            println!(
                                "[a{:20}][u{}] Skipping mismatching user",
                                appid, steamid64_str
                            );
                            continue;
                        }
                    } else {
                        println!(
                            "[a{:20}][u{}] Display name: {:?}",
                            appid, steamid64_str, name
                        );

                        target_screenshots_dir = target_screenshots_dir.join(name);
                    }

                    if !target_screenshots_dir.is_dir() {
                        std::fs::create_dir_all(target_screenshots_dir.clone())?;
                    }

                    let steam_account_id_str = steam_account_id_from_dir.to_string();

                    let steamid_steam_user_data_dir =
                        steam_user_data_dir.join(&steam_account_id_str);

                    let steam_user_screenshots_dir =
                        steamid_steam_user_data_dir.join("760").join("remote");

                    // If there's no screenshot folder, just move on to the next event
                    if !steam_user_screenshots_dir.is_dir() {
                        println!(
                            "[u{}] User does not have a Steam screenshot folder at {:?}!",
                            steamid64_str, steam_user_screenshots_dir
                        );
                        continue;
                    }

                    println!(
                        "[a{:20}][u{}] Found Steam screenshot folder {:?} for user {:?}",
                        appid, steamid64_str, steam_user_screenshots_dir, name
                    );

                    let appid_str = appid.to_string();

                    let steam_app_screenshot_path = steam_user_screenshots_dir
                        .join(&appid_str)
                        .join("screenshots");

                    process_appid_for_screenshot_paths(
                        &mut steam_dir,
                        appid,
                        &steam_app_screenshot_path,
                        &target_screenshots_dir,
                    )?;
                }
            }
        }
    };

    Ok(())
}
