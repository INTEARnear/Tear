use serde::Deserialize;

#[derive(Deserialize)]
struct CargoToml {
    package: Package,
}

#[derive(Deserialize)]
struct Package {
    version: String,
}

fn main() {
    let cargo_toml = include_str!("near-cli-rs/Cargo.toml");
    let cargo_toml: CargoToml = toml::from_str(cargo_toml).unwrap();
    let near_cli_rs_version = cargo_toml.package.version;
    println!("cargo:rustc-env=NEAR_CLI_VERSION={near_cli_rs_version}");
}
