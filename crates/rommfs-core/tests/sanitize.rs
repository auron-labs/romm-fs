//! Behavioral tests for remote-name validation (PRD R2): each rule class —
//! traversal/separator/drive/control rejection, invalid-char mapping,
//! reserved DOS names, trailing dots/spaces, and deterministic
//! collision disambiguation.

use rommfs_core::sanitize::{disambiguate, sanitize_component, ComponentName};

#[test]
fn ordinary_names_are_clean() {
    assert_eq!(
        sanitize_component("Example Game.nes"),
        Some(ComponentName::Clean("Example Game.nes".into()))
    );
    assert_eq!(
        sanitize_component("Another Game.sfc"),
        Some(ComponentName::Clean("Another Game.sfc".into()))
    );
    // Unicode names Windows supports stay clean.
    assert_eq!(
        sanitize_component("Pokémon.gb"),
        Some(ComponentName::Clean("Pokémon.gb".into()))
    );
}

#[test]
fn traversal_absolute_and_separator_names_are_rejected() {
    for bad in [
        "..",
        ".",
        "",
        "   ",
        "../evil.nes",
        "a/b.nes",
        "a\\b.nes",
        "C:\\roms\\x.nes",
        "\\\\server\\share\\x.nes",
    ] {
        assert_eq!(sanitize_component(bad), None, "expected {bad:?} rejected");
    }
}

#[test]
fn control_characters_are_rejected() {
    assert_eq!(sanitize_component("a\u{0000}b.nes"), None);
    assert_eq!(sanitize_component("a\u{0007}b.nes"), None);
    assert_eq!(sanitize_component("a\u{001f}b.nes"), None);
    assert_eq!(sanitize_component("a\u{007f}b.nes"), None);
}

#[test]
fn windows_invalid_characters_are_mapped() {
    assert_eq!(
        sanitize_component("a<b>.nes"),
        Some(ComponentName::Adjusted("a_b_.nes".into()))
    );
    assert_eq!(
        sanitize_component("a:b|c?d*e\"f.nes"),
        Some(ComponentName::Adjusted("a_b_c_d_e_f.nes".into()))
    );
    // A bare "C:name" is a Windows drive-relative path — the ':' maps away,
    // leaving an ordinary safe name.
    assert_eq!(
        sanitize_component("C:x.nes"),
        Some(ComponentName::Adjusted("C_x.nes".into()))
    );
}

#[test]
fn reserved_dos_names_are_adjusted() {
    // Reserved stems get a deterministic suffix, case-insensitively.
    assert_eq!(
        sanitize_component("CON.nes"),
        Some(ComponentName::Adjusted("CON_.nes".into()))
    );
    assert_eq!(
        sanitize_component("nul"),
        Some(ComponentName::Adjusted("nul_".into()))
    );
    assert_eq!(
        sanitize_component("com1.TXT"),
        Some(ComponentName::Adjusted("com1_.TXT".into()))
    );
    assert_eq!(
        sanitize_component("LPT9.sfc"),
        Some(ComponentName::Adjusted("LPT9_.sfc".into()))
    );
    assert_eq!(
        sanitize_component("COM¹.nes"),
        Some(ComponentName::Adjusted("COM¹_.nes".into()))
    );
    assert_eq!(
        sanitize_component("lpt².Game Boy.rom"),
        Some(ComponentName::Adjusted("lpt²_.Game Boy.rom".into()))
    );
    assert_eq!(
        sanitize_component("com³.TXT"),
        Some(ComponentName::Adjusted("com³_.TXT".into()))
    );
    // Not actually reserved: stems that merely start with a device name.
    assert_eq!(
        sanitize_component("CONSOLE.nes"),
        Some(ComponentName::Clean("CONSOLE.nes".into()))
    );
    assert_eq!(
        sanitize_component("aux2.gb"),
        Some(ComponentName::Clean("aux2.gb".into()))
    );
    assert_eq!(
        sanitize_component("com10.gb"),
        Some(ComponentName::Clean("com10.gb".into()))
    );
}

#[test]
fn trailing_dots_and_spaces_are_stripped() {
    assert_eq!(
        sanitize_component("name."),
        Some(ComponentName::Adjusted("name".into()))
    );
    assert_eq!(
        sanitize_component("name "),
        Some(ComponentName::Adjusted("name".into()))
    );
    assert_eq!(
        sanitize_component("name.nes."),
        Some(ComponentName::Adjusted("name.nes".into()))
    );
    // Nothing usable remains.
    assert_eq!(sanitize_component("..."), None);
    assert_eq!(sanitize_component(". "), None);
}

#[test]
fn disambiguate_returns_free_names_and_suffixes_collisions() {
    let taken = |n: &str| n == "game.nes" || n == "game (2).nes" || n == "readme";

    // Free name is returned unchanged.
    assert_eq!(disambiguate("free.nes", &|_| false), "free.nes");
    // Collision -> "name (2).ext", then "(3)", ... preserving the extension.
    assert_eq!(disambiguate("game.nes", &taken), "game (3).nes");
    // No extension -> suffix on the whole name.
    assert_eq!(disambiguate("README", &taken), "README (2)");
    // Case-insensitive view: "DATA.bin" collides with a taken "data.bin".
    let ci = |n: &str| n == "data.bin";
    assert_eq!(disambiguate("DATA.bin", &ci), "DATA (2).bin");
}
