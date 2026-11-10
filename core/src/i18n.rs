//! Langue des messages du moteur (français par défaut, anglais en option).

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set_english(en: bool) {
    EN.store(en, Ordering::Relaxed);
}

pub fn is_en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// `tr!("texte {x}", "text {x}", ...)` : choisit la version française ou anglaise.
#[macro_export]
macro_rules! tr {
    ($fr:literal, $en:literal $(, $a:expr)* $(,)?) => {
        if $crate::i18n::is_en() { format!($en $(, $a)*) } else { format!($fr $(, $a)*) }
    };
}
