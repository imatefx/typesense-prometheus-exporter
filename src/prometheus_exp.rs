use prometheus::{Counter, Encoder, Opts, Registry, TextEncoder};

// #[tokio::main]
pub(crate) async fn main_server() {
    // Create a Counter.
    let counter_opts = Opts::new("test_counter", "test counter help");
    let counter = Counter::with_opts(counter_opts).unwrap();

    // Create a Registry and register Counter.
    let r = Registry::new();
    r.register(Box::new(counter.clone())).unwrap();

    // Inc.
    counter.inc();

    // Gather the metrics.
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metric_families = r.gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    // Output to the standard output.
    println!("{}", String::from_utf8(buffer).unwrap());
}
