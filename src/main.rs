use anyhow::{Context, Result};
use clap::Parser;
use directories::UserDirs;
use steamid_ng::SteamID;
use steamlocate::SteamDir;

/// Symlink your Steam games' screenshot directories into your Pictures folder
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Specify the name of the directory to manage inside your Pictures folder
    #[arg(short, long, default_value = "Steam Screenshots")]
    pictures_directory_name: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let screenshots_dir = UserDirs::new()
        .with_context(|| "Failed to fetch user directory information")?
        .picture_dir()
        .with_context(|| "Failed to find picture directory")?
        .join(args.pictures_directory_name);

    let mut steam_dir =
        SteamDir::locate().with_context(|| "Failed to locate Steam on this computer")?;

    // TODO: Does this include non-Steam shortcuts?
    // ANSWER: NO, non-Steam shortcuts have 64-bit IDs, lol
    let steam_installed_apps = steam_dir.apps().to_owned();

    let users_list = steamy_vdf::load(steam_dir.path.join("config").join("loginusers.vdf"))?
        .get("users")
        .with_context(|| "Failed to find any  Steam users")?
        .as_table()
        .with_context(|| "Failed to find any  Steam users")?
        .to_owned();

    for (steamid_str, userinfo) in users_list.iter() {
        let steamid = SteamID::from(steamid_str.parse::<u64>()?);

        println!("[{}] Processing user", steamid_str);

        let steam_user_screenshots_dir = steam_dir
            .path
            .join("userdata")
            .join(steamid.account_id().to_string())
            .join("760")
            .join("remote");

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
            .with_context(|| format!("Failed to retrieve account PersonaName for {}", steamid_str))?
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

            let steam_app_screenshot_path = entry.path().join("screenshots");
            let filename = entry.file_name();

            let appid_str = filename
                .to_str()
                .with_context(|| "Failed to retrieve app id")?;

            let appid = appid_str.parse::<u32>()?;

            println!(
                "[{}; {:10}] Found app screenshot folder: {:?}",
                steamid_str, appid, steam_app_screenshot_path
            );

            let symlink_name = match steam_installed_apps.get(&appid) {
                Some(Some(app)) => app
                    .path
                    .file_name()
                    .with_context(|| "Failed to retrieve file name from install path")?,
                Some(&None) | None => std::ffi::OsStr::new(appid_str),
            };

            let target_symlink_path = target_screenshots_dir.join(symlink_name);

            println!(
                "[{}; {:10}] target_symlink_path: {:?}",
                steamid_str, appid, target_symlink_path
            );

            if target_symlink_path.is_symlink() {
                match symlink::remove_symlink_auto(&target_symlink_path) {
                    Ok(_) => {}
                    Err(error) => println!("Error unlinking {:?}: {}", target_symlink_path, error),
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

            if let Ok(appid) = appid_str.parse::<u32>() {
                println!("[{}] Cleanup found dir with app id: {}", steamid_str, appid);

                if steam_installed_apps.contains_key(&appid) {
                    let entry_symlink_path = entry.path();

                    if entry_symlink_path.is_symlink() {
                        println!(
                            "[{}] App {} is installed! We don't need this symlink",
                            steamid_str, appid
                        );

                        match symlink::remove_symlink_auto(&entry_symlink_path) {
                            Ok(_) => {}
                            Err(error) => {
                                println!("Error unlinking {:?}: {}", entry_symlink_path, error)
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

    Ok(())
}
