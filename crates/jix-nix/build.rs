fn main() {
    println!("cargo:rerun-if-changed=nix-version");
    println!("cargo:rerun-if-changed=src/roots.cc");
    let library = pkg_config::Config::new()
        .probe("nix-store")
        .expect("pinned Nix development libraries required");
    let expected = include_str!("nix-version").trim();
    assert_eq!(
        library.version, expected,
        "Jix's native adapter and nix-bindings-rust are validated with Nix {expected}; update and test the backend pins together"
    );
    let mut build = cc::Build::new();
    build.cpp(true).std("c++23").file("src/roots.cc");
    for path in library.include_paths {
        build.include(path);
    }
    for package in ["nix-expr-c", "nix-expr", "nix-fetchers", "bdw-gc"] {
        for path in pkg_config::Config::new()
            .probe(package)
            .expect("Nix development dependency")
            .include_paths
        {
            build.include(path);
        }
    }
    build.opt_level(1);
    build.compile("jix-nix-roots");
}
