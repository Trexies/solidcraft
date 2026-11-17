//! Saving a Ledger product: Save writes the design Ledger opened back into Ledger's own folder.
//!
//! When Ledger opens a product it *claims* the tab it opened it in (`ledger.claim`, by the session's `uid`): the tab then
//! knows its product's code and is named after it, and Save (the button, Ctrl+S) writes `designs/<code>.solidcraft` beside
//! Ledger's database without a Save As box - the first time too. Closing a product tab, or quitting, saves it under its OWN
//! code, never under whichever item Ledger opened last. A tab Ledger did not open (a blank one, a file the operator opened)
//! is SolidCraft's as before and asks. Save As still makes a copy elsewhere; the tab is then that copy and lets go of the
//! product.
//!
//! When the product is in production, Save asks first: the run is a frozen copy and does not follow the design. After a
//! product save the picture is taken again and Ledger is told, so its audit and "Changed since moved" are current at once.
//! Ledger's half arrives through [`set_hooks`] when SolidCraft runs inside Ledger's window; a SolidCraft Ledger launched on
//! its own has no hooks, so it neither warns nor notifies.

use crate::SolveApp;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// What Ledger answers for SolidCraft.
pub struct Hooks {
    /// Why saving this product needs a word first (it is in production), or `None` to save straight away.
    pub production_note: Box<dyn Fn(&str) -> Option<String> + Send + Sync>,
    /// A product's design was just saved into Ledger's folder.
    pub saved: Box<dyn Fn(&str) + Send + Sync>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Ledger's answers, set once by the host before the editor starts.
pub fn set_hooks(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// The product each claimed tab holds, by session uid: `(code, name)`.
fn claims() -> &'static Mutex<HashMap<u64, (String, String)>> {
    static CLAIMS: OnceLock<Mutex<HashMap<u64, (String, String)>>> = OnceLock::new();
    CLAIMS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The product a session holds, if Ledger opened it there.
pub fn claim_of(uid: u64) -> Option<(String, String)> {
    claims().lock().ok()?.get(&uid).cloned()
}

/// The shown tab holds this product now (the control channel's `ledger.claim`). Another tab that held it lets go: one
/// product, one tab that saves it.
pub fn claim(app: &mut SolveApp, code: &str, name: &str) {
    if let Ok(mut map) = claims().lock() {
        map.retain(|_, (held, _)| held != code);
        map.insert(app.session.uid, (code.to_owned(), name.to_owned()));
    }
}

fn unclaim(uid: u64) {
    if let Ok(mut map) = claims().lock() {
        map.remove(&uid);
    }
}

/// Whether a tab holds this product (Ledger opened it there).
pub fn is_claimed(code: &str) -> bool {
    claims().lock().is_ok_and(|map| map.values().any(|(held, _)| held == code))
}

/// Ledger deleted this product: close the tab that holds it WITHOUT saving - its claimed tab, or a tab opened from its
/// design file - and let go of it, so nothing writes the deleted product's design back into Ledger's folder (closing or
/// quitting saved a product tab under its code, and left an orphan design behind). Answers whether a tab was closed.
pub fn drop_product(app: &mut SolveApp, code: &str) -> bool {
    let path = design_path(code);
    let holds = |session: &solidcraft_engine::Session| {
        claim_of(session.uid).is_some_and(|(held, _)| held == code)
            || session.path.as_deref().is_some_and(|own| same_file(std::path::Path::new(own), &path))
    };
    let found = (0..app.docs.count()).find(|&i| crate::documents::session_of(app, i).is_some_and(holds));
    if let Ok(mut map) = claims().lock() {
        map.retain(|_, (held, _)| held != code);
    }
    if let Ok(mut slot) = pending().lock() {
        if slot.as_ref().is_some_and(|wait| wait.code == code) {
            *slot = None;
        }
    }
    let Some(i) = found else { return false };
    crate::documents::close(app, i, true);
    true
}

fn owned() -> bool {
    std::env::var_os("LEDGER_OWNED").is_some()
}

/// A code as a file name, the rule Ledger uses too.
pub fn stem_of(code: &str) -> String {
    code.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

/// Where a product's design lives in Ledger's folder.
pub fn design_path(code: &str) -> PathBuf {
    crate::ledger_boards::designs_dir().join(format!("{}.solidcraft", stem_of(code)))
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.to_string_lossy().replace('/', "\\").eq_ignore_ascii_case(&b.to_string_lossy().replace('/', "\\"))
}

/// Where this session is saved without asking: its product's design when Ledger opened it here, else its own file when
/// that file already lives in Ledger's folder. `None`: it is the operator's design, and asks.
pub fn owned_path(session: &solidcraft_engine::Session) -> Option<PathBuf> {
    if !owned() {
        return None;
    }
    if let Some((code, _)) = claim_of(session.uid) {
        return Some(design_path(&code));
    }
    let path = PathBuf::from(session.path.as_deref()?);
    let dir = crate::ledger_boards::designs_dir();
    path.parent().is_some_and(|parent| same_file(parent, &dir)).then_some(path)
}

/// The tab's name: a product's tab shows its code (not "Untitled", nor the name stored inside its file).
pub fn tab_name(session: &solidcraft_engine::Session) -> String {
    // A product's tab is named by its code, saved or not: a copy made from an earlier save carries the original's name
    // inside its file, and a tab named after the original is the wrong product to the eye.
    match claim_of(session.uid) {
        Some((code, _)) => code,
        None => session.doc.name.clone(),
    }
}

/// How many earlier saves of a product are kept (the owner, 2026-11-16: "the last 20 changes").
pub const HISTORY_KEEP: usize = 20;

/// Where a product's earlier saves are kept: `designs/history/<stem>/`, one `<ms>.solidcraft` per save (named by the time
/// that version was saved, in milliseconds since 1970) with its picture `<ms>.png` beside it. Ledger reads the same folder.
pub fn history_dir(code: &str) -> PathBuf {
    crate::ledger_boards::designs_dir().join("history").join(stem_of(code))
}

/// Before a product's design is written again: copy the one on disk - and its picture - into the product's history, then
/// keep only the newest [`HISTORY_KEEP`]. Nothing to keep (never saved yet) is not an error; a copy that fails is only
/// reported, since refusing the save over it would lose the newer work instead.
pub fn keep_version(app: &mut SolveApp, code: &str) {
    if let Err(error) = keep_version_of(code) {
        app.set_status(format!("saving {code}: its earlier version could not be kept ({error})"), true);
    }
}

fn keep_version_of(code: &str) -> std::io::Result<()> {
    let design = design_path(code);
    let Ok(meta) = std::fs::metadata(&design) else { return Ok(()) };
    let saved = meta.modified()?.duration_since(std::time::UNIX_EPOCH).map(|at| at.as_millis()).unwrap_or(0);
    let dir = history_dir(code);
    std::fs::create_dir_all(&dir)?;
    let kept = dir.join(format!("{saved}.solidcraft"));
    // The same version kept already (saved twice with nothing written between): once is enough.
    if !kept.exists() {
        std::fs::copy(&design, &kept)?;
        let picture = crate::ledger_boards::file().with_file_name("shots").join(format!("{}.png", stem_of(code)));
        if picture.is_file() {
            let _ = std::fs::copy(&picture, dir.join(format!("{saved}.png")));
        }
    }
    prune(&dir, HISTORY_KEEP);
    Ok(())
}

/// Keep the newest `keep` versions in `dir`, each with its picture.
pub fn prune(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut versions: Vec<u128> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension()? == "solidcraft").then_some(())?;
            path.file_stem()?.to_str()?.parse().ok()
        })
        .collect();
    versions.sort_unstable_by(|a, b| b.cmp(a));
    for old in versions.into_iter().skip(keep) {
        let _ = std::fs::remove_file(dir.join(format!("{old}.solidcraft")));
        let _ = std::fs::remove_file(dir.join(format!("{old}.png")));
    }
}

/// A save waiting for the operator's word (the product is in production).
#[derive(Clone)]
struct Pending {
    uid: u64,
    code: String,
    note: String,
}

fn pending() -> &'static Mutex<Option<Pending>> {
    static PENDING: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

/// Save (the button, Ctrl+S) on a product tab: into Ledger's folder, with no Save As box. `false`: not a product tab, so
/// SolidCraft's own Save goes ahead.
pub fn save(app: &mut SolveApp) -> bool {
    if !owned() {
        return false;
    }
    let Some((code, _)) = claim_of(app.session.uid) else { return false };
    if let Some(note) = HOOKS.get().and_then(|hooks| (hooks.production_note)(&code)) {
        if let Ok(mut slot) = pending().lock() {
            *slot = Some(Pending { uid: app.session.uid, code, note });
        }
        return true;
    }
    start(app, &code);
    true
}

fn start(app: &mut SolveApp, code: &str) {
    keep_version(app, code);
    let path = design_path(code);
    if let Some(dir) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(dir) {
            app.set_status(format!("could not make {}: {error}", dir.display()), true);
            return;
        }
    }
    crate::save_job::start(app, "file.save_as", json!({ "path": path.to_string_lossy(), "overwrite": true }));
}

/// The "this is in production" question, while one is waiting.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(wait) = pending().lock().ok().and_then(|slot| slot.clone()) else { return };
    let clear = || {
        if let Ok(mut slot) = pending().lock() {
            *slot = None;
        }
    };
    // The tab it was asked about went away (closed, switched): nothing to save from here.
    if wait.uid != app.session.uid {
        clear();
        return;
    }
    let title = format!("Save {}?", wait.code);
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new("sc_ledger_production_save"), "status_warning", &title)
        .tone(crate::ui_kit::Tone::Warning)
        .actions("Save", "Cancel", None)
        .show(ctx, true, |ui| {
            crate::ui_kit::text(ui, &wait.note, crate::ui_kit::Text::Report("ledger_production_save", false));
        });
    match action {
        crate::ui_kit::Action::Ok => {
            clear();
            start(app, &wait.code);
        }
        crate::ui_kit::Action::Cancel => clear(),
        _ if closed => clear(),
        _ => {}
    }
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        clear();
        app.esc_handled = true;
    }
}

/// A save has finished (`ok`): a product saved into Ledger's folder gets its picture taken again and Ledger is told. A
/// product tab Saved As somewhere else is now that copy, and lets go of the product.
pub fn after_save(app: &mut SolveApp, ok: bool) {
    if !ok || !owned() {
        return;
    }
    let Some((code, name)) = claim_of(app.session.uid) else { return };
    let Some(path) = app.session.path.clone() else { return };
    if !same_file(std::path::Path::new(&path), &design_path(&code)) {
        unclaim(app.session.uid);
        return;
    }
    match crate::ledger_boards::picture_of(app, &code, &name) {
        Ok(_) => app.set_status(format!("saved {code} into Ledger, picture taken again"), false),
        Err(error) => app.set_status(format!("saved {code} into Ledger; the picture failed: {error}"), true),
    }
    if let Some(hooks) = HOOKS.get() {
        (hooks.saved)(&code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest_twenty_with_their_pictures() {
        let dir = std::env::temp_dir().join(format!("sc-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for at in 1..=25u128 {
            std::fs::write(dir.join(format!("{at}.solidcraft")), b"d").unwrap();
            std::fs::write(dir.join(format!("{at}.png")), b"p").unwrap();
        }
        prune(&dir, HISTORY_KEEP);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left.len(), 40, "20 designs and their 20 pictures: {left:?}");
        assert!(!dir.join("5.solidcraft").exists() && !dir.join("5.png").exists(), "the oldest went, picture too");
        assert!(dir.join("6.solidcraft").exists() && dir.join("25.solidcraft").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_code_becomes_the_same_file_name_ledger_uses() {
        assert_eq!(stem_of("SOL-DKS-2448-3D (1)"), "SOL-DKS-2448-3D--1-");
        assert_eq!(stem_of("BASE_CAB-36"), "BASE_CAB-36");
    }

    #[test]
    fn a_claim_follows_the_tab_and_one_product_has_one_tab() {
        let mut h = crate::scenario::Harness::new();
        claim(&mut h.app, "T-CLAIM-1", "Table");
        let first = h.app.session.uid;
        assert_eq!(claim_of(first).map(|c| c.0), Some("T-CLAIM-1".to_owned()));
        assert_eq!(tab_name(&h.app.session), "T-CLAIM-1", "an unsaved product tab is named by its code");
        h.app.session.path = Some("elsewhere/Original.solidcraft".into());
        assert_eq!(tab_name(&h.app.session), "T-CLAIM-1", "and so is one with a file whose design is named otherwise");
        h.app.session.path = None;
        // The same product claimed again (opened again into another tab) moves the claim.
        h.app.session = solidcraft_engine::Session::default();
        claim(&mut h.app, "T-CLAIM-1", "Table");
        assert!(claim_of(first).is_none(), "the old tab let go");
        assert!(claim_of(h.app.session.uid).is_some());
        unclaim(h.app.session.uid);
    }
}
