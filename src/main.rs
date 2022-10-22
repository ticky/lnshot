use anyhow::{Context, Result};
use clap::Parser;
use crc::{Algorithm, Crc};
use directories::UserDirs;
use steam_shortcuts_util::shortcut::Shortcut;
use steamid_ng::SteamID;
use steamlocate::SteamDir;

/// The CRC32 Algorithm used by Steam Big Picture shortcuts.
/// Not intended to be used directly; call [`calculate_shortcut_id`](self::calculate_shortcut_id) instead.
const BIGPICTURE_ALGORITHM: Algorithm<u32> = Algorithm {
    width: 32,
    poly: 0x04C11DB7,
    init: 0xffffffff,
    refin: true,
    refout: true,
    xorout: 0xffffffff,
    check: 0x00000000,
    residue: 0x00000000,
};

/// Computes the Steam Big Picture-compatible shortcut ID, as used by
/// screenshots, for a given Steam Shortcut object.
///
/// Based upon documentation of the algorithm from
/// <https://gaming.stackexchange.com/a/386883>
fn calculate_shortcut_id(shortcut: &Shortcut) -> u64 {
    let checksum = {
        let crc = Crc::<u32>::new(&BIGPICTURE_ALGORITHM);
        let mut digest = crc.digest();
        digest.update(format!("{}{}", shortcut.exe, shortcut.app_name).as_bytes());
        digest.finalize()
    };

    let top_32 = checksum | 0x80000000;
    ((top_32 as u64) << 32) | 0x02000000
}

/// Symlink your Steam games' screenshot directories into your Pictures folder
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Specify the name of the directory to manage inside your Pictures folder
    #[arg(short, long, default_value = "Steam Screenshots")]
    pictures_directory_name: String,
}

/// I am the `main` function, with [`anyhow`](anyhow) result magic.
fn main() -> Result<()> {
    let args = Args::parse();

    let screenshots_dir = UserDirs::new()
        .with_context(|| "Failed to fetch user directory information")?
        .picture_dir()
        .with_context(|| "Failed to find picture directory")?
        .join(args.pictures_directory_name);

    let mut steam_dir =
        SteamDir::locate().with_context(|| "Failed to locate Steam on this computer")?;

    let steam_installed_apps = steam_dir.apps().to_owned();

    let users_list = steamy_vdf::load(steam_dir.path.join("config").join("loginusers.vdf"))?
        .get("users")
        .with_context(|| "Failed to find any Steam users")?
        .as_table()
        .with_context(|| "Failed to find any Steam users")?
        .to_owned();

    for (steamid_str, userinfo) in users_list.iter() {
        let steamid = SteamID::from(steamid_str.parse::<u64>()?);

        println!("[{}] Processing user", steamid_str);

        let steam_user_data_dir = steam_dir
            .path
            .join("userdata")
            .join(steamid.account_id().to_string());

        let steam_user_screenshots_dir = steam_user_data_dir.join("760").join("remote");

        // TODO: ignore this
        let shortcuts_data =
            std::fs::read(steam_user_data_dir.join("config").join("shortcuts.vdf"))?;
        let shortcuts_list = match steam_shortcuts_util::parse_shortcuts(&shortcuts_data) {
            Ok(list) => list,
            Err(error) => {
                println!("Error parsing shortcuts list: {}", error);
                vec![]
            }
        };

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

            let appid = appid_str.parse::<u64>()?;

            println!(
                "[{}; {:20}] Found app screenshot folder: {:?}",
                steamid_str, appid, steam_app_screenshot_path
            );

            let symlink_name = if let Some(Some(app)) = steam_installed_apps.get(&(appid as u32)) {
                app.path
                    .file_name()
                    .with_context(|| "Failed to retrieve file name from install path")?
            } else if let Some(shortcut) = shortcuts_list.iter().find(|shortcut| {
                u64::from(shortcut.app_id & 0x7fffff) == appid
                    || calculate_shortcut_id(shortcut) == appid
            }) {
                std::ffi::OsStr::new(shortcut.app_name)
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

            if let Ok(appid) = appid_str.parse::<u64>() {
                println!("[{}] Cleanup found dir with app id: {}", steamid_str, appid);

                if steam_installed_apps.contains_key(&(appid as u32))
                    || shortcuts_list.iter().any(|shortcut| {
                        u64::from(shortcut.app_id & 0x7fffff) == appid
                            || calculate_shortcut_id(shortcut) == appid
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_calculate_shortcut_id() {
        use crate::calculate_shortcut_id;
        use steam_shortcuts_util::Shortcut;

        let shortcut_second_life = Shortcut {
            order: "0",
            app_id: 2931025216,
            app_name: "Second Life",
            exe: "\"/Applications/Second Life Viewer.app\"",
            start_dir: "\"/Applications/\"",
            icon: "",
            shortcut_path: "",
            launch_options: "",
            is_hidden: false,
            allow_desktop_config: false,
            allow_overlay: false,
            open_vr: 0,
            dev_kit: 0,
            dev_kit_game_id: "",
            dev_kit_overrite_app_id: 0,
            last_play_time: 1666334099,
            tags: vec![],
        };

        assert_eq!(
            calculate_shortcut_id(&shortcut_second_life),
            18291777663678808064
        );

        let shortcut_nfs_most_wanted = Shortcut {
            order: "19",
            app_id: 3127109556,
            app_name: "Need for Speed: Most Wanted",
            exe: "\"/home/deck/.local/share/Steam/steamapps/compatdata/3127109556/pfx/drive_c/Program Files (x86)/EA GAMES/Need for Speed Most Wanted/speed.exe\"",
            start_dir: "\"/home/deck/.local/share/Steam/steamapps/compatdata/3127109556/pfx/drive_c/Program Files (x86)/EA GAMES/Need for Speed Most Wanted/\"",
            icon: "/home/deck/.steam/steam/userdata/36075541/config/grid/3127109556_icon.png",
            shortcut_path: "",
            launch_options: "WINEDLLOVERRIDES=\"dinput8=n,b\" %command%",
            is_hidden: false,
            allow_desktop_config: false,
            allow_overlay: false,
            open_vr: 0,
            dev_kit: 0,
            dev_kit_game_id: "",
            dev_kit_overrite_app_id: 0,
            last_play_time: 1665898298,
            tags: vec![
                "Racing & Driving Games",
            ],
        };

        assert_eq!(
            calculate_shortcut_id(&shortcut_nfs_most_wanted),
            14897979843084812288
        );
    }
}
