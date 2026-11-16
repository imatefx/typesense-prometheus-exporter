//! Precedence with real environment variables. Kept in its own test binary
//! (and a single test) because it changes the process environment.

use typesense_prometheus_exporter::cli::CliArgs;

#[test]
fn flag_beats_environment_beats_file_beats_default() {
    let path = std::env::temp_dir().join(format!("ts-config-env-{}.toml", std::process::id()));
    std::fs::write(
        &path,
        "typesense_host = \"filehost\"\ntypesense_port = 1111\ntypesense_timeout = 7\ntypesense_protocol = \"https\"\n",
    )
    .unwrap();
    let config = format!("--config={}", path.display());
    std::env::set_var("TYPESENSE_PORT", "2222");
    std::env::set_var("TYPESENSE_HOST", "envhost");

    let args = CliArgs::load_from(["exporter", &config]).unwrap();
    assert_eq!(args.typesense_port, 2222, "environment beats the file");
    assert_eq!(args.typesense_host.as_deref(), Some("envhost"));
    assert_eq!(args.typesense_timeout, 7, "file beats the default");
    assert_eq!(args.typesense_protocol, "https");

    let args = CliArgs::load_from(["exporter", &config, "--typesense-port=3333"]).unwrap();
    assert_eq!(args.typesense_port, 3333, "flag beats the environment");

    std::env::remove_var("TYPESENSE_PORT");
    std::env::remove_var("TYPESENSE_HOST");
    let args = CliArgs::load_from(["exporter", &config]).unwrap();
    assert_eq!(args.typesense_port, 1111);
    assert_eq!(args.typesense_host.as_deref(), Some("filehost"));

    std::env::set_var("TYPESENSE_EXPORTER_CONFIG", &path);
    let args = CliArgs::load_from(["exporter"]).unwrap();
    assert_eq!(
        args.typesense_port, 1111,
        "--config is read from its variable"
    );
    std::env::remove_var("TYPESENSE_EXPORTER_CONFIG");
    std::fs::remove_file(&path).unwrap();
}
