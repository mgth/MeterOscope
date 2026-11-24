//! Windows : intègre l'icône et les informations de version dans l'exécutable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/meteroscope.ico");
    // Le script tourne sur l'hôte : on teste la cible, pas `cfg!(windows)`.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icon/meteroscope.ico")
        .set("ProductName", "MeterOscope")
        // ASCII seulement : rc.exe lit le .rc généré dans la page de code du système.
        .set("FileDescription", "MeterOscope - image measurement")
        .set("OriginalFilename", "MeterOscope.exe")
        .set("LegalCopyright", "Copyright (C) Mathieu GRENET - GPL-3.0-or-later");
    // windres de mingw lors d'une compilation croisée depuis Linux.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") && !cfg!(windows) {
        res.set_toolkit_path("/usr/bin").set_windres_path("x86_64-w64-mingw32-windres").set_ar_path("x86_64-w64-mingw32-ar");
    }
    res.compile().expect("compilation des ressources Windows");
}
