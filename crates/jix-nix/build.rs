fn main() {
    println!("cargo:rerun-if-changed=src/roots.cc");
    let library = pkg_config::Config::new()
        .probe("nix-store")
        .expect("pinned Nix development libraries required");
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
