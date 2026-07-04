//! Private directory handling (.ssh / .gnupg), yadm's private_dirs family.

use std::path::Path;

use crate::context::Context;
use crate::os;
use crate::paths;
use crate::util;

/// yadm's private_dirs("all"): .ssh plus the gnupg dir.
pub fn private_dirs_all(ctx: &Context) -> Vec<String> {
    vec![".ssh".to_string(), gnupg_dir(ctx)]
}

/// yadm's private_dirs (default fetch): the gnupg dir only.
pub fn gnupg_dir(ctx: &Context) -> String {
    match std::env::var("GNUPGHOME") {
        Ok(v) if !v.is_empty() => paths::relative_path(&ctx.work, &v),
        _ => ".gnupg".to_string(),
    }
}

pub fn assert_private_dirs(ctx: &Context, dirs: &[String]) {
    use std::os::unix::fs::PermissionsExt;
    for private_dir in dirs {
        let full = format!("{}/{}", ctx.work, private_dir);
        if !Path::new(&full).is_dir() {
            util::debug(ctx, &format!("Creating {full}"));
            // mkdir -m 0700 -p
            if std::fs::create_dir_all(&full).is_ok() {
                let _ = std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
}

/// Debug-only report of private dir permissions (caller guards on ctx.debug).
pub fn display_private_perms(ctx: &Context, when: &str) {
    for private_dir in private_dirs_all(ctx) {
        let full = format!("{}/{}", ctx.work, private_dir);
        if Path::new(&full).is_dir() {
            let private_perms = os::capture("ls", &["-ld", &full]);
            util::debug(ctx, &format!("{when} private dir perms {private_perms}"));
        }
    }
}
