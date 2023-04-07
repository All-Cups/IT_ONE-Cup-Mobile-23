fn builder() -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(log::LevelFilter::Info);
    builder.format_timestamp_millis();
    builder.parse_env("LOG");
    builder
}

pub fn init() {
    builder().init();
}

#[cfg(test)]
pub fn init_for_tests() {
    let _ = builder().is_test(true).try_init();
}
