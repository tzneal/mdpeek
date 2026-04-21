use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{cache, db, repo};

#[derive(Debug, Default)]
pub struct IgnoreArgs {
    pub add: Vec<String>,
    #[allow(dead_code)]
    pub list: bool,
    pub remove: Option<String>,
    pub clear: bool,
}

pub fn run(args: IgnoreArgs, cwd: &std::path::Path, json: bool) -> Result<()> {
    let root = repo::find_root(cwd)?;
    let paths = cache::paths_for(&root)?;
    let conn = db::open(&paths.db)?;

    if args.clear {
        db::clear_user_ignores(&conn)?;
        emit(&[], json, "cleared all user-ignore patterns");
        return Ok(());
    }
    if let Some(p) = args.remove.as_deref() {
        let removed = db::remove_user_ignore(&conn, p)?;
        let msg = if removed {
            format!("removed: {p}")
        } else {
            format!("no such pattern: {p}")
        };
        emit(&db::list_user_ignores(&conn)?, json, &msg);
        return Ok(());
    }
    if !args.add.is_empty() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        for p in &args.add {
            db::add_user_ignore(&conn, p, now)?;
        }
    }
    // Default action (no flags) is to list.
    let patterns = db::list_user_ignores(&conn)?;
    let msg = if args.add.is_empty() {
        String::new()
    } else {
        format!("added {} pattern(s)", args.add.len())
    };
    emit(&patterns, json, &msg);
    Ok(())
}

fn emit(patterns: &[String], json: bool, message: &str) {
    if json {
        let obj = serde_json::json!({
            "patterns": patterns,
            "message": message,
        });
        println!("{obj}");
    } else {
        if !message.is_empty() {
            println!("{message}");
        }
        for p in patterns {
            println!("{p}");
        }
    }
}
