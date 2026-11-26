fn main() {
    #[cfg(target_os = "macos")]
    bureau::serveur::main();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("web_bureau ne fonctionne que sur macOS (capture et contrôle du Mac).");
        std::process::exit(3);
    }
}
